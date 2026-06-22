use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use validator::Validate;

use crate::{
    error::ApiError, routes::auth::authenticate_request, services::rate_limit, state::AppState,
};

const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_VIDEO_BYTES: u64 = 100 * 1024 * 1024;
const UPLOAD_TTL_SECONDS: u32 = 900;
const ALLOWED_IMAGE_TYPES: &[&str] = &["image/jpeg", "image/png", "image/webp"];
const ALLOWED_VIDEO_TYPES: &[&str] = &["video/mp4", "video/quicktime", "video/webm"];

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct CreateUploadIntentRequest {
    #[validate(length(min = 1, max = 180))]
    pub file_name: String,
    pub content_type: String,
    pub size_bytes: u64,
    pub purpose: Option<String>,
    pub checksum_sha256: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadIntentResponse {
    pub provider: &'static str,
    pub upload_id: String,
    pub object_key: String,
    pub upload_url: String,
    pub public_url: String,
    pub resource_type: &'static str,
    pub expires_in_seconds: u32,
    pub max_size_bytes: u64,
    pub allowed_content_types: Vec<&'static str>,
    pub cloudinary: CloudinaryUploadFields,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudinaryUploadFields {
    pub cloud_name: String,
    pub api_key: String,
    pub signature: String,
    pub timestamp: i64,
    pub folder: String,
    pub public_id: String,
}

pub async fn create_upload_intent(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<CreateUploadIntentRequest>,
) -> Result<(StatusCode, Json<UploadIntentResponse>), ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let claims = authenticate_request(&state, &headers)?;
    let user_id = uuid::Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;

    rate_limit::check_user(
        &state,
        &user_id.to_string(),
        "media:upload-intent",
        30,
        std::time::Duration::from_secs(60 * 60),
    )
    .await?;

    let media_kind = media_kind(&payload.content_type).ok_or_else(|| {
        ApiError::Validation(format!(
            "unsupported media content type: {}",
            payload.content_type
        ))
    })?;
    let max_upload_bytes = if media_kind == "video" {
        MAX_VIDEO_BYTES
    } else {
        MAX_IMAGE_BYTES
    };

    if payload.size_bytes == 0 || payload.size_bytes > max_upload_bytes {
        return Err(ApiError::Validation(format!(
            "sizeBytes must be between 1 and {max_upload_bytes}"
        )));
    }
    if matches!(
        payload.purpose.as_deref(),
        Some("profile-avatar" | "ong-logo")
    ) && media_kind != "image"
    {
        return Err(ApiError::Validation(
            "profile-avatar and ong-logo uploads must be images".into(),
        ));
    }
    let active_intents: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::bigint
        FROM media_upload_intents
        WHERE user_id = $1
          AND consumed_at IS NULL
          AND expires_at > now()
        "#,
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;
    if active_intents >= 30 {
        return Err(ApiError::TooManyRequests);
    }

    let api_key = state
        .config
        .cloudinary_api_key
        .clone()
        .ok_or(ApiError::Internal)?;
    let api_secret = state
        .config
        .cloudinary_api_secret
        .as_deref()
        .ok_or(ApiError::Internal)?;
    let cloud_name = state.config.cloudinary_cloud_name.clone();

    if let Some(checksum) = &payload.checksum_sha256 {
        let valid_sha256 =
            checksum.len() == 64 && checksum.chars().all(|ch| ch.is_ascii_hexdigit());
        if !valid_sha256 {
            return Err(ApiError::Validation(
                "checksumSha256 must be a 64-char hex string".into(),
            ));
        }
    }

    let upload_id = uuid::Uuid::now_v7();
    let safe_file_name = payload
        .file_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    let folder = format!(
        "zoohelp/{}/{media_kind}",
        upload_purpose(payload.purpose.as_deref())
    );
    let public_id = format!("{upload_id}-{safe_file_name}");
    let object_key = format!("{folder}/{public_id}");
    let timestamp = chrono::Utc::now().timestamp();
    let signature_payload =
        format!("folder={folder}&public_id={public_id}&timestamp={timestamp}{api_secret}");
    let signature = format!("{:x}", Sha1::digest(signature_payload.as_bytes()));

    let upload_url = format!("https://api.cloudinary.com/v1_1/{cloud_name}/{media_kind}/upload");
    let public_url =
        format!("https://res.cloudinary.com/{cloud_name}/{media_kind}/upload/{object_key}");
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(i64::from(UPLOAD_TTL_SECONDS));

    sqlx::query(
        r#"
        INSERT INTO media_upload_intents (
          id, user_id, provider, resource_type, object_key, file_name, content_type,
          size_bytes, checksum_sha256, upload_url, public_url, expires_at
        )
        VALUES ($1, $2, 'cloudinary', $3, $4, $5, $6, $7, $8, $9, $10, $11)
        "#,
    )
    .bind(upload_id)
    .bind(user_id)
    .bind(media_kind)
    .bind(&object_key)
    .bind(&payload.file_name)
    .bind(&payload.content_type)
    .bind(payload.size_bytes as i64)
    .bind(payload.checksum_sha256.as_deref())
    .bind(&upload_url)
    .bind(&public_url)
    .bind(expires_at)
    .execute(&state.db)
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(UploadIntentResponse {
            provider: "cloudinary",
            upload_id: upload_id.to_string(),
            upload_url,
            public_url,
            object_key,
            resource_type: media_kind,
            expires_in_seconds: UPLOAD_TTL_SECONDS,
            max_size_bytes: max_upload_bytes,
            allowed_content_types: allowed_content_types(),
            cloudinary: CloudinaryUploadFields {
                cloud_name,
                api_key,
                signature,
                timestamp,
                folder,
                public_id,
            },
        }),
    ))
}

fn media_kind(content_type: &str) -> Option<&'static str> {
    if ALLOWED_IMAGE_TYPES.contains(&content_type) {
        return Some("image");
    }
    if ALLOWED_VIDEO_TYPES.contains(&content_type) {
        return Some("video");
    }
    None
}

fn upload_purpose(purpose: Option<&str>) -> &'static str {
    match purpose {
        Some("ong-logo") => "ong-logos",
        Some("kyb-document") => "kyb-documents",
        Some("profile-avatar") => "profile-avatars",
        _ => "posts",
    }
}

fn allowed_content_types() -> Vec<&'static str> {
    ALLOWED_IMAGE_TYPES
        .iter()
        .chain(ALLOWED_VIDEO_TYPES.iter())
        .copied()
        .collect()
}
