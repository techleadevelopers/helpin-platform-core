use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{
    domain::seed_posts,
    error::ApiError,
    services::notifications::{PushPlatform, PushSubscription, RescueAlert},
    state::AppState,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationItem {
    pub id: String,
    pub title: String,
    pub body: String,
    pub read: bool,
    pub kind: &'static str,
    pub post_id: Option<String>,
    pub image_url: Option<String>,
    pub distance_km: Option<f64>,
    pub critical: bool,
    pub deeplink: Option<String>,
    pub dedupe_key: Option<String>,
    pub ttl_seconds: Option<u32>,
    pub category: Option<&'static str>,
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

pub async fn list_notifications(State(state): State<AppState>) -> Json<Vec<NotificationItem>> {
    let mut items: Vec<_> = state
        .notifications
        .list_recent_alerts()
        .into_iter()
        .flat_map(|alert| {
            if alert.recipients.is_empty() {
                return vec![NotificationItem {
                    id: alert.id,
                    title: alert.title,
                    body: alert.body,
                    read: false,
                    kind: "rescue_alert",
                    post_id: Some(alert.post_id.clone()),
                    image_url: alert.image_url,
                    distance_km: None,
                    critical: alert.critical,
                    deeplink: Some(format!("zoohelp://post/{}", alert.post_id)),
                    dedupe_key: Some(format!("rescue:{}", alert.post_id)),
                    ttl_seconds: Some(900),
                    category: Some("rescue"),
                    payload: None,
                    created_at: alert.created_at,
                }];
            }

            alert
                .recipients
                .into_iter()
                .map(|recipient| NotificationItem {
                    id: format!("{}:{}", alert.id, recipient.user_id),
                    title: alert.title.clone(),
                    body: alert.body.clone(),
                    read: false,
                    kind: "rescue_alert",
                    post_id: Some(alert.post_id.clone()),
                    image_url: alert.image_url.clone(),
                    distance_km: Some(recipient.distance_km),
                    critical: alert.critical,
                    deeplink: Some(format!("zoohelp://post/{}", alert.post_id)),
                    dedupe_key: Some(format!("rescue:{}", alert.post_id)),
                    ttl_seconds: Some(900),
                    category: Some("rescue"),
                    payload: None,
                    created_at: alert.created_at.clone(),
                })
                .collect()
        })
        .collect();

    if items.is_empty() {
        items.push(NotificationItem {
            id: "notif-dev".to_string(),
            title: "Alertas de resgate perto de voce".to_string(),
            body: "Quando houver emergencia no seu raio, ela aparece aqui e tambem vai para push."
                .to_string(),
            read: false,
            kind: "system",
            post_id: None,
            image_url: None,
            distance_km: None,
            critical: false,
            deeplink: None,
            dedupe_key: Some("system:rescue-alerts".to_string()),
            ttl_seconds: None,
            category: Some("system"),
            payload: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        });
    }

    Json(items)
}

pub async fn register_push_token(
    State(state): State<AppState>,
    Json(payload): Json<RegisterPushTokenRequest>,
) -> Result<(StatusCode, Json<RegisterPushTokenResponse>), ApiError> {
    payload
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;

    let subscribers = state.notifications.upsert_subscription(PushSubscription {
        user_id: payload.user_id,
        push_token: payload.push_token,
        platform: payload.platform,
        lat: payload.lat,
        lng: payload.lng,
        radius_km: payload.radius_km.unwrap_or(8.0).clamp(1.0, 50.0),
        critical_alerts: payload.critical_alerts.unwrap_or(false),
        updated_at: chrono::Utc::now().to_rfc3339(),
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(RegisterPushTokenResponse {
            status: "registered",
            subscribers,
        }),
    ))
}

pub async fn preview_rescue_alert(
    State(state): State<AppState>,
    Path(post_id): Path<String>,
) -> Result<Json<AlertDispatchResponse>, ApiError> {
    let post = seed_posts()
        .into_iter()
        .find(|post| post.id == post_id)
        .ok_or(ApiError::NotFound)?;
    let alert = state.notifications.dispatch_rescue_alert(&post, 5.0);
    Ok(Json(AlertDispatchResponse { alert }))
}

pub async fn mark_as_read(Path(_id): Path<String>) -> Json<NotificationActionResponse> {
    Json(NotificationActionResponse { status: "read" })
}

pub async fn ack(Path(_id): Path<String>) -> Json<NotificationActionResponse> {
    Json(NotificationActionResponse {
        status: "acknowledged",
    })
}
