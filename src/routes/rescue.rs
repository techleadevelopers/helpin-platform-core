use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::Response,
    Json,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;
use validator::Validate;

use crate::{
    error::ApiError,
    services::auth as auth_service,
    state::{AppState, RescueEvent},
};

const ACTIVE_RESCUE_LIMIT: i64 = 200;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RescueSession {
    pub id: String,
    pub post_id: String,
    pub status: String,
    pub lat: f64,
    pub lng: f64,
    pub accuracy: Option<f64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct TriggerRescueRequest {
    #[validate(length(min = 1, max = 120))]
    pub post_id: String,
    #[validate(range(min = -90.0, max = 90.0))]
    pub lat: f64,
    #[validate(range(min = -180.0, max = 180.0))]
    pub lng: f64,
    pub accuracy: Option<f64>,
}

#[derive(Deserialize, Validate)]
pub struct LocationUpdateRequest {
    #[validate(range(min = -90.0, max = 90.0))]
    pub lat: f64,
    #[validate(range(min = -180.0, max = 180.0))]
    pub lng: f64,
    pub accuracy: Option<f64>,
}

#[derive(Deserialize, Validate)]
pub struct IncidentRequest {
    #[validate(length(min = 1, max = 2000))]
    pub description: String,
    #[serde(default)]
    pub attachments: Vec<String>,
}

#[derive(Serialize)]
pub struct RescueResponse {
    pub rescue: RescueSession,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncidentResponse {
    pub id: String,
    pub rescue_id: String,
    pub status: &'static str,
}

pub async fn list_active(
    State(state): State<AppState>,
) -> Result<Json<Vec<RescueSession>>, ApiError> {
    let rows = sqlx::query(
        r#"
        SELECT id, post_id, status, lat, lng, accuracy, created_at, updated_at
        FROM rescue_sessions
        WHERE status = 'active'
        ORDER BY updated_at DESC
        LIMIT $1
        "#,
    )
    .bind(ACTIVE_RESCUE_LIMIT)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows.into_iter().map(row_to_rescue).collect()))
}

pub async fn trigger(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<TriggerRescueRequest>,
) -> Result<(StatusCode, Json<RescueResponse>), ApiError> {
    payload
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;

    let mut tx = state.db.begin().await?;
    let row = sqlx::query(
        r#"
        INSERT INTO rescue_sessions (id, post_id, reporter_user_id, status, lat, lng, accuracy)
        VALUES ($1, $2, $3, 'active', $4, $5, $6)
        RETURNING id, post_id, status, lat, lng, accuracy, created_at, updated_at
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(&payload.post_id)
    .bind(optional_user_id(&state, &headers))
    .bind(payload.lat)
    .bind(payload.lng)
    .bind(payload.accuracy)
    .fetch_one(&mut *tx)
    .await?;

    let rescue = row_to_rescue(row);
    insert_location_point(&mut tx, &rescue.id, rescue.lat, rescue.lng, rescue.accuracy).await?;
    tx.commit().await?;

    broadcast_rescue_event(&state, &rescue);
    Ok((StatusCode::CREATED, Json(RescueResponse { rescue })))
}

pub async fn update_location(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<LocationUpdateRequest>,
) -> Result<Json<RescueResponse>, ApiError> {
    payload
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;

    let mut tx = state.db.begin().await?;
    let row = sqlx::query(
        r#"
        UPDATE rescue_sessions
        SET lat = $2,
            lng = $3,
            accuracy = $4,
            updated_at = now()
        WHERE id = $1
          AND status = 'active'
        RETURNING id, post_id, status, lat, lng, accuracy, created_at, updated_at
        "#,
    )
    .bind(id)
    .bind(payload.lat)
    .bind(payload.lng)
    .bind(payload.accuracy)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApiError::NotFound)?;

    let rescue = row_to_rescue(row);
    insert_location_point(&mut tx, &rescue.id, rescue.lat, rescue.lng, rescue.accuracy).await?;
    tx.commit().await?;

    broadcast_rescue_event(&state, &rescue);
    Ok(Json(RescueResponse { rescue }))
}

pub async fn end(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RescueResponse>, ApiError> {
    let row = sqlx::query(
        r#"
        UPDATE rescue_sessions
        SET status = 'ended',
            updated_at = now(),
            ended_at = COALESCE(ended_at, now())
        WHERE id = $1
        RETURNING id, post_id, status, lat, lng, accuracy, created_at, updated_at
        "#,
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;

    let rescue = row_to_rescue(row);
    broadcast_rescue_event(&state, &rescue);
    Ok(Json(RescueResponse { rescue }))
}

pub async fn incident(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<IncidentRequest>,
) -> Result<(StatusCode, Json<IncidentResponse>), ApiError> {
    payload
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;

    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM rescue_sessions WHERE id = $1)")
            .bind(id)
            .fetch_one(&state.db)
            .await?;

    if !exists {
        return Err(ApiError::NotFound);
    }

    let incident_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO rescue_incidents (id, rescue_id, description, attachments, status)
        VALUES ($1, $2, $3, $4, 'queued_review')
        "#,
    )
    .bind(incident_id)
    .bind(id)
    .bind(payload.description)
    .bind(serde_json::to_value(payload.attachments).unwrap_or_else(|_| serde_json::json!([])))
    .execute(&state.db)
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(IncidentResponse {
            id: incident_id.to_string(),
            rescue_id: id.to_string(),
            status: "queued_review",
        }),
    ))
}

pub async fn rescue_ws(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM rescue_sessions WHERE id = $1)")
            .bind(id)
            .fetch_one(&state.db)
            .await?;

    if !exists {
        return Err(ApiError::NotFound);
    }

    Ok(ws.on_upgrade(move |socket| handle_rescue_socket(state, id, socket)))
}

async fn insert_location_point(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    rescue_id: &str,
    lat: f64,
    lng: f64,
    accuracy: Option<f64>,
) -> Result<(), sqlx::Error> {
    let rescue_uuid = Uuid::parse_str(rescue_id).map_err(|error| {
        sqlx::Error::Protocol(format!("invalid rescue id in persisted row: {error}"))
    })?;

    sqlx::query(
        r#"
        INSERT INTO rescue_location_points (rescue_id, lat, lng, accuracy)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(rescue_uuid)
    .bind(lat)
    .bind(lng)
    .bind(accuracy)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

async fn handle_rescue_socket(state: AppState, rescue_id: Uuid, socket: WebSocket) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.rescue_tx.subscribe();

    loop {
        tokio::select! {
            incoming = receiver.next() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(payload))) => {
                        if sender.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        tracing::debug!(?error, %rescue_id, "rescue websocket receive error");
                        break;
                    }
                }
            }
            event = rx.recv() => {
                match event {
                    Ok(event) if event.rescue_id == rescue_id.to_string() => {
                        let Ok(payload) = serde_json::to_string(&event) else {
                            continue;
                        };
                        if sender.send(Message::Text(payload)).await.is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::debug!(?error, %rescue_id, "rescue broadcast receive error");
                        break;
                    }
                }
            }
        }
    }
}

fn row_to_rescue(row: sqlx::postgres::PgRow) -> RescueSession {
    RescueSession {
        id: row.get::<Uuid, _>("id").to_string(),
        post_id: row.get("post_id"),
        status: row.get("status"),
        lat: row.get("lat"),
        lng: row.get("lng"),
        accuracy: row.get("accuracy"),
        created_at: row
            .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
            .to_rfc3339(),
        updated_at: row
            .get::<chrono::DateTime<chrono::Utc>, _>("updated_at")
            .to_rfc3339(),
    }
}

fn broadcast_rescue_event(state: &AppState, rescue: &RescueSession) {
    let _ = state.rescue_tx.send(RescueEvent {
        rescue_id: rescue.id.clone(),
        post_id: rescue.post_id.clone(),
        status: rescue.status.clone(),
        lat: rescue.lat,
        lng: rescue.lng,
        accuracy: rescue.accuracy,
        updated_at: rescue.updated_at.clone(),
    });
}

fn optional_user_id(state: &AppState, headers: &HeaderMap) -> Option<Uuid> {
    let token = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
        })?;

    auth_service::verify_access_token(&state.config, token)
        .ok()
        .and_then(|claims| Uuid::parse_str(&claims.sub).ok())
}
