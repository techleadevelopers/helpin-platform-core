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
    domain::seed_posts,
    error::ApiError,
    routes::auth::authenticate_request,
    services::notifications::{
        dispatch_persistent_rescue_alert, upsert_persistent_subscription, PushPlatform,
        PushSubscription, RescueAlert,
    },
    state::AppState,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationItem {
    pub id: String,
    pub title: String,
    pub body: String,
    pub read: bool,
    pub kind: String,
    pub post_id: Option<String>,
    pub image_url: Option<String>,
    pub distance_km: Option<f64>,
    pub critical: bool,
    pub deeplink: Option<String>,
    pub dedupe_key: Option<String>,
    pub ttl_seconds: Option<u32>,
    pub category: Option<String>,
    pub payload: Option<serde_json::Value>,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct NotificationActionResponse {
    pub status: &'static str,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct RegisterPushTokenRequest {
    #[validate(length(min = 1, max = 120))]
    pub user_id: String,
    #[validate(length(min = 8, max = 512))]
    pub push_token: String,
    pub platform: PushPlatform,
    #[validate(range(min = -90.0, max = 90.0))]
    pub lat: f64,
    #[validate(range(min = -180.0, max = 180.0))]
    pub lng: f64,
    pub radius_km: Option<f64>,
    pub critical_alerts: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterPushTokenResponse {
    pub status: &'static str,
    pub subscribers: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertDispatchResponse {
    pub alert: RescueAlert,
}

pub async fn list_notifications(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Json<Vec<NotificationItem>>, ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;

    let rows = sqlx::query(
        r#"
        SELECT id, title, body, read_at, kind, post_id, image_url, distance_km, critical,
               deeplink, dedupe_key, ttl_seconds, category, payload, created_at
        FROM notification_events
        WHERE user_id = $1 OR user_id IS NULL
        ORDER BY created_at DESC
        LIMIT 100
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|row| NotificationItem {
                id: row.get::<Uuid, _>("id").to_string(),
                title: row.get("title"),
                body: row.get("body"),
                read: row
                    .get::<Option<chrono::DateTime<chrono::Utc>>, _>("read_at")
                    .is_some(),
                kind: row.get("kind"),
                post_id: row.get("post_id"),
                image_url: row.get("image_url"),
                distance_km: row.get("distance_km"),
                critical: row.get("critical"),
                deeplink: row.get("deeplink"),
                dedupe_key: row.get("dedupe_key"),
                ttl_seconds: row
                    .get::<Option<i32>, _>("ttl_seconds")
                    .map(|value| value as u32),
                category: row.get("category"),
                payload: row.get("payload"),
                created_at: row
                    .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
                    .to_rfc3339(),
            })
            .collect(),
    ))
}

pub async fn register_push_token(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<RegisterPushTokenRequest>,
) -> Result<(StatusCode, Json<RegisterPushTokenResponse>), ApiError> {
    payload
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;
    let claims = authenticate_request(&state, &headers)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    if payload.user_id != claims.sub {
        return Err(ApiError::Forbidden);
    }

    let subscription = PushSubscription {
        user_id: payload.user_id,
        push_token: payload.push_token,
        platform: payload.platform,
        lat: payload.lat,
        lng: payload.lng,
        radius_km: payload.radius_km.unwrap_or(8.0).clamp(1.0, 50.0),
        critical_alerts: payload.critical_alerts.unwrap_or(false),
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    let subscribers = upsert_persistent_subscription(&state.db, user_id, &subscription).await?;
    state.notifications.upsert_subscription(subscription);

    Ok((
        StatusCode::ACCEPTED,
        Json(RegisterPushTokenResponse {
            status: "registered",
            subscribers,
        }),
    ))
}

pub async fn preview_rescue_alert(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(post_id): Path<String>,
) -> Result<Json<AlertDispatchResponse>, ApiError> {
    authenticate_request(&state, &headers)?;
    let post = seed_posts()
        .into_iter()
        .find(|post| post.id == post_id)
        .ok_or(ApiError::NotFound)?;
    let alert = dispatch_persistent_rescue_alert(&state.db, &post, 5.0).await?;
    Ok(Json(AlertDispatchResponse { alert }))
}

pub async fn mark_as_read(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<NotificationActionResponse>, ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    let notification_id = Uuid::parse_str(&id).map_err(|_| ApiError::NotFound)?;
    let result = sqlx::query(
        "UPDATE notification_events SET read_at = now() WHERE id = $1 AND (user_id = $2 OR user_id IS NULL)",
    )
    .bind(notification_id)
    .bind(user_id)
    .execute(&state.db)
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(Json(NotificationActionResponse { status: "read" }))
}

pub async fn ack(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<NotificationActionResponse>, ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    let notification_id = Uuid::parse_str(&id).map_err(|_| ApiError::NotFound)?;
    let result = sqlx::query(
        "UPDATE notification_events SET acked_at = now() WHERE id = $1 AND (user_id = $2 OR user_id IS NULL)",
    )
    .bind(notification_id)
    .bind(user_id)
    .execute(&state.db)
    .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(Json(NotificationActionResponse {
        status: "acknowledged",
    }))
}
