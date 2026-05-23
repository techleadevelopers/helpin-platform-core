use std::time::Duration as StdDuration;

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
    domain::{AccountType, AnimalType, Author, Post, PostMedia, PostType},
    error::ApiError,
    routes::auth::audit_event,
    services::notifications::{dispatch_persistent_rescue_alert, RescueAlert},
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
            if let Some(post) = load_post_by_id(&state, existing_id).await? {
                return Ok((
                    StatusCode::OK,
                    Json(CreatePostResponse {
                        post,
                        media: Vec::new(),
                        moderation_status: "approved",
                        fraud_risk: 0,
                        rescue_alert: None,
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
    let initial_rescue_status = if requires_geo_alert { "active" } else { "open" };

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
            urgent, rescue_status, text_only, moderation_status, fraud_risk, geo, idempotency_key
        )
        VALUES (
            $1, $2::post_type, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
            $14, $15, $16, 'approved', $17,
            ST_SetSRID(ST_MakePoint($9, $8), 4326)::geography,
            $18
        )
        RETURNING id
        "#
    } else {
        r#"
        INSERT INTO posts (
            author_id, post_type, animal_type, name, breed, age, description,
            latitude, longitude, location_label, neighborhood, contact, tags,
            urgent, rescue_status, text_only, moderation_status, fraud_risk, idempotency_key
        )
        VALUES (
            $1, $2::post_type, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
            $14, $15, $16, 'approved', $17, $18
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
    sqlx::query("INSERT INTO chat_rooms (post_id) VALUES ($1)")
        .bind(post_id)
        .execute(&mut *tx)
        .await?;
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
        rescue_status: initial_rescue_status.to_string(),
        resolved_at: None,
        created_at: "agora".into(),
        contact,
        tags,
        latitude,
        longitude,
    };

    let rescue_alert = if requires_geo_alert {
        let alert = dispatch_persistent_rescue_alert(&state.db, &post, 5.0).await?;
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
    broadcast_feed_event(&state, &post);

    Ok((
        StatusCode::CREATED,
        Json(CreatePostResponse {
            post,
            media: stored_media,
            moderation_status: "approved",
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
                moderation_status: "approved".into(),
            })
        })
        .collect()
}

pub async fn get_post(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Post>, ApiError> {
    let post_id = Uuid::parse_str(&id).map_err(|_| ApiError::NotFound)?;
    load_post_by_id(&state, post_id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn toggle_like(
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
    let deleted = sqlx::query("DELETE FROM post_likes WHERE post_id = $1 AND user_id = $2")
        .bind(post_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let liked = if deleted.rows_affected() == 0 {
        sqlx::query("INSERT INTO post_likes (post_id, user_id) VALUES ($1, $2)")
            .bind(post_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        true
    } else {
        false
    };
    sqlx::query(
        r#"
        UPDATE posts
        SET likes_count = (
          SELECT COUNT(*)::int FROM post_likes WHERE post_id = $1
        )
        WHERE id = $1
        "#,
    )
    .bind(post_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(LikeResponse { post_id: id, liked }))
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
) -> Result<Option<Post>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
            p.id::text AS id,
            p.post_type::text AS post_type,
            p.animal_type,
            COALESCE(p.name, 'Publicacao') AS name,
            COALESCE(p.breed, '') AS breed,
            COALESCE(p.age, '') AS age,
            p.description,
            COALESCE(p.location_label, '') AS location,
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
            p.comments_count,
            p.shares_count,
            p.urgent,
            p.rescue_status,
            p.resolved_at,
            p.created_at,
            p.contact,
            p.tags,
            COALESCE(p.latitude, -23.5505) AS latitude,
            COALESCE(p.longitude, -46.6333) AS longitude,
            u.id::text AS author_id,
            u.name AS author_name,
            u.avatar_url AS author_avatar,
            u.verified AS author_verified,
            u.account_type::text AS author_type
        FROM posts p
        INNER JOIN users u ON u.id = p.author_id
        WHERE p.id = $1
          AND p.moderation_status = 'approved'
          AND u.deleted_at IS NULL
        "#,
    )
    .bind(post_id)
    .fetch_optional(&state.db)
    .await?;

    Ok(row.map(|row| {
        let author_type = match row.get::<&str, _>("author_type") {
            "ong" => AccountType::Ong,
            "vet" => AccountType::Vet,
            "admin" => AccountType::Admin,
            _ => AccountType::Person,
        };
        Post {
            id: row.get("id"),
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
        }
    }))
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
