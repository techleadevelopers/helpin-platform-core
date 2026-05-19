use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{
    domain::{seed_authors, seed_posts, AnimalType, Post, PostMedia, PostType},
    error::ApiError,
    services::fraud,
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
    State(state): State<AppState>,
    Json(payload): Json<CreatePostRequest>,
) -> Result<(StatusCode, Json<CreatePostResponse>), ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    let media = normalize_media(&payload)?;
    let risk = fraud::score_post_text(&payload.description);
    let author = seed_authors()
        .into_iter()
        .find(|author| author.id == "u5")
        .ok_or(ApiError::Internal)?;
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

    let post = Post {
        id: uuid::Uuid::now_v7().to_string(),
        post_type,
        animal_type: payload.animal_type,
        name: payload.name.unwrap_or_else(|| "Publicação".into()),
        breed: payload.breed.unwrap_or_default(),
        age: payload.age.unwrap_or_default(),
        description: payload.description,
        location: payload.location.clone(),
        neighborhood: payload.neighborhood.unwrap_or(payload.location),
        image: cover_image,
        images: media.clone(),
        text_only: media.is_empty() && payload.image.is_none(),
        author,
        likes: 0,
        comments: 0,
        shares: 0,
        urgent: is_urgent,
        created_at: "agora".into(),
        contact: payload.contact.unwrap_or_default(),
        tags: payload.tags.unwrap_or_default(),
        latitude: payload.latitude.unwrap_or(-23.5505),
        longitude: payload.longitude.unwrap_or(-46.6333),
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
            media,
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
