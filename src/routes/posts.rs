use std::{collections::HashMap, time::Duration as StdDuration};

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
    domain::{
        AccountType, AnimalType, Author, Post, PostMedia, PostType, RescueOperationalSummary,
    },
    error::ApiError,
    routes::auth::audit_event,
    services::notifications::RescueAlert,
    services::rescue_fanout::{
        create_fanout_state_for_post, upsert_rescue_response, RescueResponseRecord,
    },
    services::{auth as auth_service, fraud, rate_limit},
    state::{AppState, FeedEvent},
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

pub(crate) fn optional_authenticated_user_id(
    state: &AppState,
    headers: &HeaderMap,
) -> Option<Uuid> {
    authenticate_request(state, headers)
        .ok()
        .and_then(|claims| Uuid::parse_str(&claims.sub).ok())
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
        "admin" => AccountType::Admin,
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
    pub location_address: Option<PostLocationAddress>,
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
    pub geo_source: Option<String>,
    pub route_public: Option<bool>,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct PostLocationAddress {
    #[validate(length(min = 1, max = 120))]
    pub street: String,
    #[validate(length(min = 1, max = 30))]
    pub number: String,
    #[validate(length(min = 1, max = 120))]
    pub neighborhood: String,
    #[validate(length(min = 1, max = 120))]
    pub city: String,
    #[validate(length(min = 2, max = 2))]
    pub state: String,
    #[validate(length(max = 120))]
    pub complement: Option<String>,
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
    pub rescue_fanout_state_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LikeResponse {
    pub post_id: String,
    pub liked: bool,
    pub likes: u32,
}

#[derive(Serialize)]
pub struct DeletePostResponse {
    pub status: &'static str,
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
    pub created_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostCommentResponse {
    pub id: String,
    pub post_id: String,
    pub body: String,
    pub created_at: String,
    pub author: Author,
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

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct RescuePostResponseRequest {
    #[serde(default = "default_rescue_response_action")]
    #[validate(length(min = 1, max = 40))]
    pub action: String,
    #[serde(default = "default_rescue_response_status")]
    #[validate(length(min = 1, max = 40))]
    pub status: String,
    #[validate(range(min = -90.0, max = 90.0))]
    pub lat: Option<f64>,
    #[validate(range(min = -180.0, max = 180.0))]
    pub lng: Option<f64>,
    pub eta_seconds: Option<i32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RescuePostResponseAck {
    pub response: RescueResponseRecord,
}

fn default_rescue_response_action() -> String {
    "going".to_string()
}

fn default_rescue_response_status() -> String {
    "confirmed".to_string()
}

struct ResolvedPostLocation {
    label: String,
    neighborhood: String,
    latitude: Option<f64>,
    longitude: Option<f64>,
    geo_status: &'static str,
    geo_source: Option<&'static str>,
    route_public: bool,
    enqueue_geocode: bool,
}

impl PostLocationAddress {
    fn normalized_state(&self) -> String {
        self.state.trim().to_uppercase()
    }

    fn label(&self) -> String {
        let street = format!("{}, {}", self.street.trim(), self.number.trim());
        let city_state = format!("{}, {}", self.city.trim(), self.normalized_state());
        let mut parts = vec![street, self.neighborhood.trim().to_string(), city_state];
        if let Some(complement) = self
            .complement
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            parts.insert(1, complement.to_string());
        }
        parts.join(", ")
    }

    fn validate_complete(&self) -> Result<(), ApiError> {
        self.validate()
            .map_err(|error| ApiError::Validation(error.to_string()))?;
        if !self
            .normalized_state()
            .chars()
            .all(|ch| ch.is_ascii_alphabetic())
        {
            return Err(ApiError::Validation(
                "locationAddress.state must be a 2-letter UF".into(),
            ));
        }
        Ok(())
    }
}

fn resolve_post_location(payload: &CreatePostRequest) -> Result<ResolvedPostLocation, ApiError> {
    if let Some(address) = payload.location_address.as_ref() {
        address.validate_complete()?;
        let label = address.label();
        return Ok(ResolvedPostLocation {
            label,
            neighborhood: address.neighborhood.trim().to_string(),
            latitude: None,
            longitude: None,
            geo_status: "pending",
            geo_source: None,
            route_public: payload.route_public.unwrap_or(false),
            enqueue_geocode: true,
        });
    }

    if payload.latitude.is_some() ^ payload.longitude.is_some() {
        return Err(ApiError::Validation(
            "latitude and longitude must be sent together".into(),
        ));
    }
    if payload.latitude.is_some() && payload.geo_source.as_deref() != Some("gps_confirmed") {
        return Err(ApiError::Validation(
            "coordinates require geoSource=gps_confirmed".into(),
        ));
    }
    let confirmed = payload.latitude.is_some();

    Ok(ResolvedPostLocation {
        label: payload.location.clone(),
        neighborhood: payload
            .neighborhood
            .clone()
            .unwrap_or_else(|| payload.location.clone()),
        latitude: payload.latitude,
        longitude: payload.longitude,
        geo_status: if confirmed {
            "confirmed"
        } else {
            "unavailable"
        },
        geo_source: confirmed.then_some("gps_confirmed"),
        route_public: confirmed && payload.route_public.unwrap_or(false),
        enqueue_geocode: false,
    })
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
    rate_limit::check_key(
        &state,
        &format!("posts:create:{author_id}"),
        state.config.throttle_limit,
        StdDuration::from_secs(state.config.throttle_ttl_seconds),
    )
    .await?;
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(key) = idempotency_key {
        if let Some(existing_id) = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM posts WHERE author_id = $1 AND idempotency_key = $2",
        )
        .bind(author_id)
        .bind(key)
        .fetch_optional(&state.db)
        .await?
        {
            if let Some(post) = load_post_by_id(&state, existing_id, None).await? {
                return Ok((
                    StatusCode::OK,
                    Json(CreatePostResponse {
                        post,
                        media: Vec::new(),
                        moderation_status: "approved",
                        fraud_risk: 0,
                        rescue_alert: None,
                        rescue_fanout_state_id: None,
                    }),
                ));
            }
        }
    }
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
    let resolved_location = resolve_post_location(&payload)?;

    let name = payload.name.unwrap_or_else(|| "Publicação".into());
    let breed = payload.breed.unwrap_or_default();
    let age = payload.age.unwrap_or_default();
    let description = payload.description;
    let location = resolved_location.label;
    let neighborhood = resolved_location.neighborhood;
    let contact = payload.contact.unwrap_or_default();
    let tags = payload.tags.unwrap_or_default();
    let latitude = resolved_location.latitude;
    let longitude = resolved_location.longitude;
    let geo_status = resolved_location.geo_status;
    let geo_source = resolved_location.geo_source;
    let route_public = resolved_location.route_public
        && matches!(
            post_type,
            PostType::Emergency | PostType::Lost | PostType::Found
        );
    let text_only = media.is_empty() && payload.image.is_none();
    let geo_ready_for_alert = requires_geo_alert && geo_status == "confirmed";
    let initial_rescue_status = if geo_ready_for_alert {
        "active"
    } else {
        "open"
    };

    let mut tx = state.db.begin().await?;
    for (index, item) in media.iter().enumerate() {
        let upload_ref = payload
            .images
            .get(index)
            .and_then(|image| {
                image
                    .object_key
                    .as_ref()
                    .or(image.public_url.as_ref())
                    .or(image.url.as_ref())
            })
            .unwrap_or(&item.url);
        let result = sqlx::query(
            r#"
            UPDATE media_upload_intents
            SET consumed_at = COALESCE(consumed_at, now())
            WHERE user_id = $1
              AND consumed_at IS NULL
              AND expires_at > now()
              AND (object_key = $2 OR public_url = $2)
            "#,
        )
        .bind(author_id)
        .bind(upload_ref)
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() == 0 {
            return Err(ApiError::Validation(
                "media must come from an unconsumed upload intent owned by the authenticated user"
                    .into(),
            ));
        }
    }

    let insert_sql = if state.config.postgis_enabled {
        r#"
        INSERT INTO posts (
            author_id, post_type, animal_type, name, breed, age, description,
            latitude, longitude, location_label, neighborhood, contact, tags,
            urgent, rescue_status, text_only, moderation_status, fraud_risk, geo, idempotency_key,
            geo_status, geo_source, route_public, geo_provider, geo_confidence, geo_resolved_at
        )
        VALUES (
            $1, $2::post_type, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
            $14, $15, $16, 'approved', $17,
            CASE WHEN $8::double precision IS NULL OR $9::double precision IS NULL THEN NULL
              ELSE ST_SetSRID(ST_MakePoint($9, $8), 4326)::geography END,
            $18, $19, $20, $21,
            CASE WHEN $19 = 'confirmed' THEN 'device' ELSE NULL END,
            CASE WHEN $19 = 'confirmed' THEN 1.0 ELSE NULL END,
            CASE WHEN $19 = 'confirmed' THEN now() ELSE NULL END
        )
        RETURNING id
        "#
    } else {
        r#"
        INSERT INTO posts (
            author_id, post_type, animal_type, name, breed, age, description,
            latitude, longitude, location_label, neighborhood, contact, tags,
            urgent, rescue_status, text_only, moderation_status, fraud_risk, idempotency_key,
            geo_status, geo_source, route_public, geo_provider, geo_confidence, geo_resolved_at
        )
        VALUES (
            $1, $2::post_type, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
            $14, $15, $16, 'approved', $17, $18, $19, $20, $21,
            CASE WHEN $19 = 'confirmed' THEN 'device' ELSE NULL END,
            CASE WHEN $19 = 'confirmed' THEN 1.0 ELSE NULL END,
            CASE WHEN $19 = 'confirmed' THEN now() ELSE NULL END
        )
        RETURNING id
        "#
    };

    let post_id: Uuid = sqlx::query_scalar(insert_sql)
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
        .bind(initial_rescue_status)
        .bind(text_only)
        .bind(i16::from(risk))
        .bind(idempotency_key)
        .bind(geo_status)
        .bind(geo_source)
        .bind(route_public)
        .fetch_one(&mut *tx)
        .await?;

    if resolved_location.enqueue_geocode {
        sqlx::query(
            "INSERT INTO post_geocode_jobs (post_id, address_label) VALUES ($1, $2) ON CONFLICT (post_id) DO NOTHING",
        )
        .bind(post_id)
        .bind(&location)
        .execute(&mut *tx)
        .await?;
    }

    let mut stored_media = Vec::with_capacity(media.len());
    for (index, item) in media.iter().enumerate() {
        let media_id: Uuid = sqlx::query_scalar(
            r#"
            INSERT INTO post_media (
                post_id, provider, resource_type, object_key, public_url, content_type,
                width, height, size_bytes, sort_order, moderation_status
            )
            VALUES ($1, 'cloudinary', $2, $3, $4, $5, $6, $7, $8, $9, 'approved')
            RETURNING id
            "#,
        )
        .bind(post_id)
        .bind(if item.content_type.starts_with("video/") {
            "video"
        } else {
            "image"
        })
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
        liked_by_me: false,
        comments: 0,
        shares: 0,
        urgent: is_urgent,
        rescue_status: initial_rescue_status.to_string(),
        resolved_at: None,
        created_at: "agora".into(),
        contact,
        tags,
        latitude,
        longitude,
        geo_status: geo_status.to_string(),
        geo_source: geo_source.map(str::to_string),
        route_public,
        rescue_operational: geo_ready_for_alert.then(|| RescueOperationalSummary {
            fanout_phase: Some(1),
            help_going_count: 0,
            help_arrived_count: 0,
            operational_label: "Precisa de ajuda".to_string(),
        }),
        rescue_final_report: None,
    };

    let rescue_fanout_state_id = if geo_ready_for_alert {
        let state_id = create_fanout_state_for_post(&state.db, post_id, None).await?;
        tracing::info!(
            post_id = %post.id,
            fanout_state_id = %state_id,
            "rescue fanout queued"
        );
        Some(state_id.to_string())
    } else {
        None
    };
    broadcast_feed_event(&state, &post);

    Ok((
        StatusCode::CREATED,
        Json(CreatePostResponse {
            post,
            media: stored_media,
            moderation_status: "approved",
            fraud_risk: risk,
            rescue_alert: None,
            rescue_fanout_state_id,
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
                moderation_status: "approved".into(),
            })
        })
        .collect()
}

pub async fn get_post(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Post>, ApiError> {
    let post_id = Uuid::parse_str(&id).map_err(|_| ApiError::NotFound)?;
    let viewer_id = optional_authenticated_user_id(&state, &headers);
    load_post_by_id(&state, post_id, viewer_id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn delete_post(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DeletePostResponse>, ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    let post_id = Uuid::parse_str(&id).map_err(|_| ApiError::NotFound)?;

    let row = sqlx::query("SELECT author_id FROM posts WHERE id = $1")
        .bind(post_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;
    let author_id: Uuid = row.get("author_id");
    if author_id != user_id && !matches!(claims.account_type, AccountType::Admin) {
        return Err(ApiError::Forbidden);
    }

    let deleted = sqlx::query("DELETE FROM posts WHERE id = $1")
        .bind(post_id)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }

    audit_event(
        &state,
        Some(user_id),
        "post.deleted",
        serde_json::json!({ "postId": post_id }),
    )
    .await;

    Ok(Json(DeletePostResponse { status: "deleted" }))
}

pub async fn like_post(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<LikeResponse>, ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    rate_limit::check_key(
        &state,
        &format!("posts:like:{user_id}"),
        state.config.throttle_limit * 6,
        StdDuration::from_secs(state.config.throttle_ttl_seconds),
    )
    .await?;
    let post_id = Uuid::parse_str(&id).map_err(|_| ApiError::NotFound)?;
    ensure_post_exists(&state, post_id).await?;

    let mut tx = state.db.begin().await?;
    sqlx::query(
        "INSERT INTO post_likes (post_id, user_id) VALUES ($1, $2) ON CONFLICT (post_id, user_id) DO NOTHING",
    )
        .bind(post_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let likes = sqlx::query_scalar::<_, i32>(
        r#"
        UPDATE posts
        SET likes_count = (
          SELECT COUNT(*)::int FROM post_likes WHERE post_id = $1
        )
        WHERE id = $1
        RETURNING likes_count
        "#,
    )
    .bind(post_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(LikeResponse {
        post_id: id,
        liked: true,
        likes: likes.max(0) as u32,
    }))
}

pub async fn unlike_post(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<LikeResponse>, ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    rate_limit::check_key(
        &state,
        &format!("posts:unlike:{user_id}"),
        state.config.throttle_limit * 6,
        StdDuration::from_secs(state.config.throttle_ttl_seconds),
    )
    .await?;
    let post_id = Uuid::parse_str(&id).map_err(|_| ApiError::NotFound)?;
    ensure_post_exists(&state, post_id).await?;

    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM post_likes WHERE post_id = $1 AND user_id = $2")
        .bind(post_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let likes = sqlx::query_scalar::<_, i32>(
        r#"
        UPDATE posts
        SET likes_count = (
          SELECT COUNT(*)::int FROM post_likes WHERE post_id = $1
        )
        WHERE id = $1
        RETURNING likes_count
        "#,
    )
    .bind(post_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(LikeResponse {
        post_id: id,
        liked: false,
        likes: likes.max(0) as u32,
    }))
}

pub async fn create_comment(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<CommentRequest>,
) -> Result<(StatusCode, Json<CommentResponse>), ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    let claims = authenticate_request(&state, &headers)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    rate_limit::check_key(
        &state,
        &format!("posts:comment:{user_id}"),
        state.config.throttle_limit,
        StdDuration::from_secs(state.config.throttle_ttl_seconds),
    )
    .await?;
    let post_id = Uuid::parse_str(&id).map_err(|_| ApiError::NotFound)?;
    ensure_post_exists(&state, post_id).await?;

    let mut tx = state.db.begin().await?;
    let row = sqlx::query(
        r#"
        INSERT INTO post_comments (post_id, user_id, body)
        VALUES ($1, $2, $3)
        RETURNING id, created_at
        "#,
    )
    .bind(post_id)
    .bind(user_id)
    .bind(&payload.body)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        r#"
        UPDATE posts
        SET comments_count = (
          SELECT COUNT(*)::int FROM post_comments WHERE post_id = $1
        )
        WHERE id = $1
        "#,
    )
    .bind(post_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok((
        StatusCode::CREATED,
        Json(CommentResponse {
            id: row.get::<Uuid, _>("id").to_string(),
            post_id: id,
            body: payload.body,
            created_at: row
                .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
                .to_rfc3339(),
        }),
    ))
}

pub async fn list_comments(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<PostCommentResponse>>, ApiError> {
    let post_id = Uuid::parse_str(&id).map_err(|_| ApiError::NotFound)?;
    ensure_post_exists(&state, post_id).await?;

    let rows = sqlx::query(
        r#"
        SELECT
          c.id::text AS id,
          c.post_id::text AS post_id,
          c.body,
          c.created_at,
          u.id::text AS author_id,
          u.name AS author_name,
          u.avatar_url AS author_avatar,
          u.verified AS author_verified,
          u.account_type::text AS author_account_type
        FROM post_comments c
        JOIN users u ON u.id = c.user_id
        WHERE c.post_id = $1
          AND c.moderation_status <> 'rejected'
        ORDER BY c.created_at ASC
        LIMIT 50
        "#,
    )
    .bind(post_id)
    .fetch_all(&state.db)
    .await?;

    let comments = rows
        .into_iter()
        .map(|row| PostCommentResponse {
            id: row.get("id"),
            post_id: row.get("post_id"),
            body: row.get("body"),
            created_at: row
                .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
                .to_rfc3339(),
            author: Author {
                id: row.get("author_id"),
                name: row.get("author_name"),
                avatar: row.get("author_avatar"),
                verified: row.get("author_verified"),
                account_type: account_type_from_str(row.get::<&str, _>("author_account_type")),
            },
        })
        .collect();

    Ok(Json(comments))
}

pub async fn report_post(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<ReportRequest>,
) -> Result<(StatusCode, Json<ReportResponse>), ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    let claims = authenticate_request(&state, &headers)?;
    let reporter_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    rate_limit::check_key(
        &state,
        &format!("posts:report:{reporter_id}"),
        state.config.throttle_limit.max(2) / 2,
        StdDuration::from_secs(state.config.throttle_ttl_seconds * 5),
    )
    .await?;
    let post_id = Uuid::parse_str(&id).map_err(|_| ApiError::NotFound)?;
    ensure_post_exists(&state, post_id).await?;

    let severity = report_severity(&payload.reason, payload.details.as_deref());
    let report_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO post_reports (id, post_id, reporter_user_id, reason, details, severity)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(report_id)
    .bind(post_id)
    .bind(reporter_id)
    .bind(&payload.reason)
    .bind(payload.details.as_deref())
    .bind(severity)
    .execute(&state.db)
    .await?;

    audit_event(
        &state,
        Some(reporter_id),
        "trust.post.reported",
        serde_json::json!({ "postId": post_id, "reportId": report_id, "severity": severity }),
    )
    .await;

    Ok((
        StatusCode::CREATED,
        Json(ReportResponse {
            id: report_id.to_string(),
            post_id: id,
            status: "queued_review",
        }),
    ))
}

pub async fn rescue_response(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<RescuePostResponseRequest>,
) -> Result<Json<RescuePostResponseAck>, ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    if !matches!(
        payload.action.as_str(),
        "going" | "remote_support" | "unavailable"
    ) {
        return Err(ApiError::Validation(
            "invalid rescue response action".into(),
        ));
    }
    if !matches!(
        payload.status.as_str(),
        "confirmed" | "cancelled" | "arrived"
    ) {
        return Err(ApiError::Validation(
            "invalid rescue response status".into(),
        ));
    }

    let claims = authenticate_request(&state, &headers)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    rate_limit::check_key(
        &state,
        &format!("posts:rescue-response:{id}:{user_id}"),
        state.config.throttle_limit * 3,
        StdDuration::from_secs(state.config.throttle_ttl_seconds),
    )
    .await?;

    let post_id = Uuid::parse_str(&id).map_err(|_| ApiError::NotFound)?;
    ensure_post_exists(&state, post_id).await?;
    let rescue_session_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM rescue_sessions WHERE post_id = $1 AND status = 'active' ORDER BY created_at DESC LIMIT 1",
    )
    .bind(post_id)
    .fetch_optional(&state.db)
    .await?;

    let response = upsert_rescue_response(
        &state.db,
        post_id,
        rescue_session_id,
        user_id,
        &payload.action,
        &payload.status,
        payload.lat,
        payload.lng,
        payload.eta_seconds,
    )
    .await?;

    Ok(Json(RescuePostResponseAck { response }))
}

fn broadcast_feed_event(state: &AppState, post: &Post) {
    let event = FeedEvent {
        post_id: post.id.clone(),
        post_type: post_type_as_str(&post.post_type).to_string(),
        urgent: post.urgent,
        rescue_status: post.rescue_status.clone(),
        lat: post.latitude,
        lng: post.longitude,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let _ = state.feed_tx.send(event.clone());
    let bus = state.event_bus.clone();
    tokio::spawn(async move {
        bus.publish_feed(&event).await;
    });
}

async fn ensure_post_exists(state: &AppState, post_id: Uuid) -> Result<(), ApiError> {
    let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM posts WHERE id = $1")
        .bind(post_id)
        .fetch_optional(&state.db)
        .await?;
    exists.map(|_| ()).ok_or(ApiError::NotFound)
}

pub(crate) async fn load_post_by_id(
    state: &AppState,
    post_id: Uuid,
    viewer_id: Option<Uuid>,
) -> Result<Option<Post>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
            p.id::text AS id,
            p.post_type::text AS post_type,
            p.animal_type,
            COALESCE(p.name, 'Publicação') AS name,
            COALESCE(p.breed, '') AS breed,
            COALESCE(p.age, '') AS age,
            p.description,
            CASE WHEN p.route_public THEN COALESCE(p.location_label, '') ELSE COALESCE(p.neighborhood, '') END AS location,
            COALESCE(p.neighborhood, p.location_label, '') AS neighborhood,
            (
                SELECT pm.public_url
                FROM post_media pm
                WHERE pm.post_id = p.id
                ORDER BY pm.sort_order ASC, pm.created_at ASC
                LIMIT 1
            ) AS image,
            p.text_only,
            p.likes_count,
            ($2::uuid IS NOT NULL AND EXISTS (
                SELECT 1 FROM post_likes pl WHERE pl.post_id = p.id AND pl.user_id = $2
            )) AS liked_by_me,
            p.comments_count,
            p.shares_count,
            p.urgent,
            p.rescue_status,
            p.resolved_at,
            p.created_at,
            CASE WHEN p.route_public THEN p.contact ELSE '' END AS contact,
            p.tags,
            CASE WHEN p.route_public AND p.geo_status = 'confirmed' THEN p.latitude ELSE NULL END AS latitude,
            CASE WHEN p.route_public AND p.geo_status = 'confirmed' THEN p.longitude ELSE NULL END AS longitude,
            p.geo_status,
            p.geo_source,
            p.route_public,
            fs.current_phase AS fanout_phase,
            COALESCE(fs.confirmed_count, 0) AS help_going_count,
            COALESCE(fs.arrived_count, 0) AS help_arrived_count,
            u.id::text AS author_id,
            u.name AS author_name,
            u.avatar_url AS author_avatar,
            u.verified AS author_verified,
            u.account_type::text AS author_type
        FROM posts p
        INNER JOIN users u ON u.id = p.author_id
        LEFT JOIN rescue_fanout_states fs ON fs.post_id = p.id
        WHERE p.id = $1
          AND p.moderation_status = 'approved'
          AND u.deleted_at IS NULL
        "#,
    )
    .bind(post_id)
    .bind(viewer_id)
    .fetch_optional(&state.db)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let mut media_by_post = load_post_media(state, &[post_id]).await?;

    Ok(Some({
        let author_type = match row.get::<&str, _>("author_type") {
            "ong" => AccountType::Ong,
            "vet" => AccountType::Vet,
            "admin" => AccountType::Admin,
            _ => AccountType::Person,
        };
        let id: String = row.get("id");
        let mut post = Post {
            id: id.clone(),
            post_type: post_type_from_str(row.get::<&str, _>("post_type")),
            animal_type: animal_type_from_str(row.get::<&str, _>("animal_type")),
            name: row.get("name"),
            breed: row.get("breed"),
            age: row.get("age"),
            description: row.get("description"),
            location: row.get("location"),
            neighborhood: row.get("neighborhood"),
            image: row.get("image"),
            images: Vec::new(),
            text_only: row.get("text_only"),
            author: Author {
                id: row.get("author_id"),
                name: row.get("author_name"),
                avatar: row.get("author_avatar"),
                verified: row.get("author_verified"),
                account_type: author_type,
            },
            likes: row.get::<i32, _>("likes_count").max(0) as u32,
            liked_by_me: row.get("liked_by_me"),
            comments: row.get::<i32, _>("comments_count").max(0) as u32,
            shares: row.get::<i32, _>("shares_count").max(0) as u32,
            urgent: row.get("urgent"),
            rescue_status: row.get("rescue_status"),
            resolved_at: row
                .get::<Option<chrono::DateTime<chrono::Utc>>, _>("resolved_at")
                .map(|value| value.to_rfc3339()),
            created_at: row
                .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
                .to_rfc3339(),
            contact: row.get("contact"),
            tags: row.get("tags"),
            latitude: row.get("latitude"),
            longitude: row.get("longitude"),
            geo_status: row.get("geo_status"),
            geo_source: row.get("geo_source"),
            route_public: row.get("route_public"),
            rescue_operational: rescue_operational_from_row(&row),
            rescue_final_report: super::rescue::load_published_final_report_for_post(
                state, post_id,
            )
            .await?,
        };
        post.images = media_by_post.remove(&id).unwrap_or_default();
        if post.image.is_none() {
            post.image = post.images.first().map(|image| image.url.clone());
        }
        post
    }))
}

pub(crate) async fn load_post_media(
    state: &AppState,
    post_ids: &[Uuid],
) -> Result<HashMap<String, Vec<PostMedia>>, sqlx::Error> {
    if post_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query(
        r#"
        SELECT
            post_id::text AS post_id,
            id::text AS id,
            public_url,
            content_type,
            width,
            height,
            size_bytes,
            moderation_status::text AS moderation_status
        FROM post_media
        WHERE post_id = ANY($1)
        ORDER BY post_id, sort_order ASC, created_at ASC
        "#,
    )
    .bind(post_ids)
    .fetch_all(&state.db)
    .await?;

    let mut media_by_post: HashMap<String, Vec<PostMedia>> = HashMap::new();
    for row in rows {
        let post_id: String = row.get("post_id");
        media_by_post.entry(post_id).or_default().push(PostMedia {
            id: row.get("id"),
            url: row.get("public_url"),
            content_type: row.get("content_type"),
            width: row
                .get::<Option<i32>, _>("width")
                .and_then(|value| u32::try_from(value).ok()),
            height: row
                .get::<Option<i32>, _>("height")
                .and_then(|value| u32::try_from(value).ok()),
            size_bytes: row
                .get::<Option<i64>, _>("size_bytes")
                .and_then(|value| u64::try_from(value).ok()),
            moderation_status: row.get("moderation_status"),
        });
    }

    Ok(media_by_post)
}

fn rescue_operational_from_row(row: &sqlx::postgres::PgRow) -> Option<RescueOperationalSummary> {
    let phase = row.try_get::<Option<i32>, _>("fanout_phase").ok().flatten();
    let going = row.try_get::<i32, _>("help_going_count").unwrap_or(0);
    let arrived = row.try_get::<i32, _>("help_arrived_count").unwrap_or(0);
    phase.map(|fanout_phase| RescueOperationalSummary {
        fanout_phase: Some(fanout_phase),
        help_going_count: going,
        help_arrived_count: arrived,
        operational_label: if arrived > 0 {
            "Ajuda no local".to_string()
        } else if going == 1 {
            "1 pessoa a caminho".to_string()
        } else if going > 1 {
            format!("{going} pessoas a caminho")
        } else if fanout_phase >= 9 {
            "Acionando apoio ambiental".to_string()
        } else if fanout_phase >= 6 {
            "Buscando apoio regional".to_string()
        } else {
            "Precisa de ajuda".to_string()
        },
    })
}

fn report_severity(reason: &str, details: Option<&str>) -> &'static str {
    let text = format!("{} {}", reason, details.unwrap_or_default()).to_lowercase();
    if [
        "maus-tratos",
        "trafico",
        "tráfico",
        "violencia",
        "golpe",
        "scam",
    ]
    .iter()
    .any(|needle| text.contains(needle))
    {
        "high"
    } else {
        "normal"
    }
}
