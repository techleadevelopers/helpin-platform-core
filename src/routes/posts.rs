use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;
use validator::Validate;

use crate::{
    domain::{seed_posts, AccountType, AnimalType, Author, Post, PostMedia, PostType},
    error::ApiError,
    services::{auth as auth_service, fraud},
    services::notifications::RescueAlert,
    state::AppState,
};

const MAX_POST_IMAGES: usize = 4;
const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_VIDEO_BYTES: u64 = 100 * 1024 * 1024;
const ALLOWED_MEDIA_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/webp",
    "video/mp4",
    "video/quicktime",
    "video/webm",
];

fn authenticate_request(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<auth_service::AccessClaims, ApiError> {
    let header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::Unauthorized)?;
    let token = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
        .ok_or(ApiError::Unauthorized)?;
    auth_service::verify_access_token(&state.config, token).map_err(|_| ApiError::Unauthorized)
}

pub async fn load_author(state: &AppState, user_id: Uuid) -> Result<Author, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT id::text, name, avatar_url, verified, account_type::text AS account_type
        FROM users
        WHERE id = $1
        "#,
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::Unauthorized)?;

    Ok(Author {
        id: row.get("id"),
        name: row.get("name"),
        avatar: row.get("avatar_url"),
        verified: row.get("verified"),
        account_type: account_type_from_str(row.get::<&str, _>("account_type")),
    })
}

fn account_type_from_str(value: &str) -> AccountType {
    match value {
        "ong" => AccountType::Ong,
        "vet" => AccountType::Vet,
        _ => AccountType::Person,
    }
}

pub fn post_type_as_str(value: &PostType) -> &'static str {
    match value {
        PostType::Adoption => "adoption",
        PostType::Lost => "lost",
        PostType::Found => "found",
        PostType::Emergency => "emergency",
        PostType::Campaign => "campaign",
        PostType::Post => "post",
    }
}

pub fn post_type_from_str(value: &str) -> PostType {
    match value {
        "adoption" => PostType::Adoption,
        "lost" => PostType::Lost,
        "found" => PostType::Found,
        "emergency" => PostType::Emergency,
        "campaign" => PostType::Campaign,
        _ => PostType::Post,
    }
}

fn animal_type_as_str(value: &AnimalType) -> &'static str {
    match value {
        AnimalType::Dog => "dog",
        AnimalType::Cat => "cat",
        AnimalType::Other => "other",
    }
}

pub fn animal_type_from_str(value: &str) -> AnimalType {
    match value {
        "dog" => AnimalType::Dog,
        "cat" => AnimalType::Cat,
        _ => AnimalType::Other,
    }
}

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct CreatePostRequest {
    #[validate(length(min = 1, max = 120))]
    pub name: Option<String>,
    pub post_type: PostType,
    pub animal_type: AnimalType,
    pub breed: Option<String>,
    pub age: Option<String>,
    #[validate(length(min = 1, max = 1200))]
    pub description: String,
    #[validate(length(min = 1, max = 180))]
    pub location: String,
    pub neighborhood: Option<String>,
    pub image: Option<String>,
    #[serde(default)]
    pub images: Vec<CreatePostImage>,
    pub urgent: Option<bool>,
    pub contact: Option<String>,
    pub tags: Option<Vec<String>>,
    #[validate(range(min = -90.0, max = 90.0))]
    pub latitude: Option<f64>,
    #[validate(range(min = -180.0, max = 180.0))]
    pub longitude: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePostImage {
    pub object_key: Option<String>,
    pub public_url: Option<String>,
    pub url: Option<String>,
    pub content_type: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub size_bytes: Option<u64>,
    pub checksum_sha256: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePostResponse {
    pub post: Post,
    pub media: Vec<PostMedia>,
    pub moderation_status: &'static str,
    pub fraud_risk: u8,
    pub rescue_alert: Option<RescueAlert>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LikeResponse {
    pub post_id: String,
    pub liked: bool,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CommentRequest {
    #[validate(length(min = 1, max = 2000))]
    pub body: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentResponse {
    pub id: String,
    pub post_id: String,
    pub body: String,
    pub created_at: &'static str,
}

#[derive(Debug, Deserialize, Validate)]
pub struct ReportRequest {
    #[validate(length(min = 1, max = 120))]
    pub reason: String,
    pub details: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportResponse {
    pub id: String,
    pub post_id: String,
    pub status: &'static str,
}

pub async fn create_post(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<CreatePostRequest>,
) -> Result<(StatusCode, Json<CreatePostResponse>), ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    let claims = authenticate_request(&state, &headers)?;
    let author_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    let media = normalize_media(&payload)?;
    let risk = fraud::score_post_text(&payload.description);
    let author = load_author(&state, author_id).await?;
    let cover_image = media
        .first()
        .map(|image| image.url.clone())
        .or_else(|| payload.image.clone());
    let is_urgent = payload.urgent.unwrap_or(false);
    let post_type = payload.post_type.clone();
    let requires_geo_alert = is_urgent || post_type == PostType::Emergency;
    if requires_geo_alert && (payload.latitude.is_none() || payload.longitude.is_none()) {
        return Err(ApiError::Validation(
            "latitude and longitude are required for emergency rescue alerts".into(),
        ));
    }

    let name = payload.name.unwrap_or_else(|| "Publicacao".into());
    let breed = payload.breed.unwrap_or_default();
    let age = payload.age.unwrap_or_default();
    let description = payload.description;
    let location = payload.location.clone();
    let neighborhood = payload.neighborhood.unwrap_or(payload.location);
    let contact = payload.contact.unwrap_or_default();
    let tags = payload.tags.unwrap_or_default();
    let latitude = payload.latitude.unwrap_or(-23.5505);
    let longitude = payload.longitude.unwrap_or(-46.6333);
    let text_only = media.is_empty() && payload.image.is_none();

    let mut tx = state.db.begin().await?;
    let post_id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO posts (
            author_id, post_type, animal_type, name, breed, age, description,
            latitude, longitude, location_label, neighborhood, contact, tags,
            urgent, text_only, moderation_status, fraud_risk
        )
        VALUES ($1, $2::post_type, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, 'queued', $16)
        RETURNING id
        "#,
    )
    .bind(author_id)
    .bind(post_type_as_str(&post_type))
    .bind(animal_type_as_str(&payload.animal_type))
    .bind(&name)
    .bind(&breed)
    .bind(&age)
    .bind(&description)
    .bind(latitude)
    .bind(longitude)
    .bind(&location)
    .bind(&neighborhood)
    .bind(&contact)
    .bind(&tags)
    .bind(is_urgent)
    .bind(text_only)
    .bind(i16::from(risk))
    .fetch_one(&mut *tx)
    .await?;

    let mut stored_media = Vec::with_capacity(media.len());
    for (index, item) in media.iter().enumerate() {
        let media_id: Uuid = sqlx::query_scalar(
            r#"
            INSERT INTO post_media (
                post_id, provider, resource_type, object_key, public_url, content_type,
                width, height, size_bytes, sort_order, moderation_status
            )
            VALUES ($1, 'cloudinary', $2, $3, $4, $5, $6, $7, $8, $9, 'queued')
            RETURNING id
            "#,
        )
        .bind(post_id)
        .bind(if item.content_type.starts_with("video/") { "video" } else { "image" })
        .bind(&item.url)
        .bind(&item.url)
        .bind(&item.content_type)
        .bind(item.width.map(|value| value as i32))
        .bind(item.height.map(|value| value as i32))
        .bind(item.size_bytes.map(|value| value as i64))
        .bind(index as i16)
        .fetch_one(&mut *tx)
        .await?;

        let mut persisted = item.clone();
        persisted.id = media_id.to_string();
        stored_media.push(persisted);
    }
    tx.commit().await?;

    let post = Post {
        id: post_id.to_string(),
        post_type,
        animal_type: payload.animal_type,
        name,
        breed,
        age,
        description,
        location,
        neighborhood,
        image: cover_image,
        images: stored_media.clone(),
        text_only,
        author,
        likes: 0,
        comments: 0,
        shares: 0,
        urgent: is_urgent,
        created_at: "agora".into(),
        contact,
        tags,
        latitude,
        longitude,
    };

    let rescue_alert = if requires_geo_alert {
        let alert = state.notifications.dispatch_rescue_alert(&post, 5.0);
        tracing::info!(
            post_id = %post.id,
            recipients = alert.recipients.len(),
            critical = alert.critical,
            "rescue alert queued"
        );
        Some(alert)
    } else {
        None
    };

    Ok((
        StatusCode::CREATED,
        Json(CreatePostResponse {
            post,
            media: stored_media,
            moderation_status: "queued",
            fraud_risk: risk,
            rescue_alert,
        }),
    ))
}

fn normalize_media(payload: &CreatePostRequest) -> Result<Vec<PostMedia>, ApiError> {
    if payload.images.len() > MAX_POST_IMAGES {
        return Err(ApiError::Validation(format!(
            "posts support at most {MAX_POST_IMAGES} images"
        )));
    }

    payload
        .images
        .iter()
        .map(|image| {
            let url = image
                .public_url
                .as_ref()
                .or(image.url.as_ref())
                .or(image.object_key.as_ref())
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| ApiError::Validation("image url or objectKey is required".into()))?
                .trim()
                .to_owned();

            let content_type = image
                .content_type
                .clone()
                .unwrap_or_else(|| "image/jpeg".into());
            if !ALLOWED_MEDIA_TYPES.contains(&content_type.as_str()) {
                return Err(ApiError::Validation(format!(
                    "unsupported media content type: {content_type}"
                )));
            }

            let max_size = if content_type.starts_with("video/") {
                MAX_VIDEO_BYTES
            } else {
                MAX_IMAGE_BYTES
            };
            if image.size_bytes.is_some_and(|size| size > max_size) {
                return Err(ApiError::Validation(format!(
                    "media size must be <= {max_size} bytes"
                )));
            }

            if let Some(checksum) = &image.checksum_sha256 {
                let valid_sha256 =
                    checksum.len() == 64 && checksum.chars().all(|ch| ch.is_ascii_hexdigit());
                if !valid_sha256 {
                    return Err(ApiError::Validation(
                        "checksumSha256 must be a 64-char hex string".into(),
                    ));
                }
            }

            Ok(PostMedia {
                id: uuid::Uuid::now_v7().to_string(),
                url,
                content_type,
                width: image.width,
                height: image.height,
                size_bytes: image.size_bytes,
                moderation_status: "queued".into(),
            })
        })
        .collect()
}

pub async fn get_post(Path(id): Path<String>) -> Result<Json<Post>, ApiError> {
    seed_posts()
        .into_iter()
        .find(|post| post.id == id)
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn toggle_like(Path(id): Path<String>) -> Result<Json<LikeResponse>, ApiError> {
    if seed_posts().iter().any(|post| post.id == id) {
        return Ok(Json(LikeResponse {
            post_id: id,
            liked: true,
        }));
    }

    Err(ApiError::NotFound)
}

pub async fn create_comment(
    Path(id): Path<String>,
    Json(payload): Json<CommentRequest>,
) -> Result<(StatusCode, Json<CommentResponse>), ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    if !seed_posts().iter().any(|post| post.id == id) {
        return Err(ApiError::NotFound);
    }

    Ok((
        StatusCode::CREATED,
        Json(CommentResponse {
            id: uuid::Uuid::now_v7().to_string(),
            post_id: id,
            body: payload.body,
            created_at: "agora",
        }),
    ))
}

pub async fn report_post(
    Path(id): Path<String>,
    Json(payload): Json<ReportRequest>,
) -> Result<(StatusCode, Json<ReportResponse>), ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let _ = payload.details.as_deref();

    if !seed_posts().iter().any(|post| post.id == id) {
        return Err(ApiError::NotFound);
    }

    Ok((
        StatusCode::CREATED,
        Json(ReportResponse {
            id: uuid::Uuid::now_v7().to_string(),
            post_id: id,
            status: "queued_review",
        }),
    ))
}
