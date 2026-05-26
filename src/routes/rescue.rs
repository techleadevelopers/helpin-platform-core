use std::time::Duration as StdDuration;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
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
    domain::AccountType,
    error::ApiError,
    services::{auth as auth_service, rate_limit, rescue_fanout},
    state::{AppState, RescueEvent},
};

const ACTIVE_RESCUE_LIMIT: i64 = 200;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RescueSession {
    pub id: String,
    pub post_id: String,
    pub reporter_user_id: Option<String>,
    pub reporter_name: Option<String>,
    pub reporter_email: Option<String>,
    pub reporter_role: Option<String>,
    pub status: String,
    pub lat: f64,
    pub lng: f64,
    pub accuracy: Option<f64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct WsAuthQuery {
    pub access_token: Option<String>,
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

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct RescueResponseRequest {
    #[serde(default = "default_response_action")]
    #[validate(length(min = 1, max = 40))]
    pub action: String,
    #[serde(default = "default_response_status")]
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
pub struct RescueResponseAck {
    pub response: rescue_fanout::RescueResponseRecord,
}

fn default_response_action() -> String {
    "going".to_string()
}

fn default_response_status() -> String {
    "confirmed".to_string()
}

pub async fn list_active(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Json<Vec<RescueSession>>, ApiError> {
    authenticate_admin(&state, &headers)?;

    let rows = sqlx::query(
        r#"
        SELECT
          rs.id,
          rs.post_id,
          rs.reporter_user_id,
          u.name AS reporter_name,
          u.email::text AS reporter_email,
          u.account_type::text AS reporter_role,
          rs.status,
          rs.lat,
          rs.lng,
          rs.accuracy,
          rs.created_at,
          rs.updated_at
        FROM rescue_sessions rs
        LEFT JOIN users u ON u.id = rs.reporter_user_id
        WHERE rs.status = 'active'
        ORDER BY rs.updated_at DESC
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
    let reporter_user_id = authenticate_user(&state, &headers, None)?;
    rate_limit::check_key(
        &state,
        &format!("rescue:trigger:{reporter_user_id}"),
        state.config.throttle_limit,
        StdDuration::from_secs(state.config.throttle_ttl_seconds),
    )
    .await?;
    let post_id = Uuid::parse_str(&payload.post_id).map_err(|_| ApiError::NotFound)?;
    let confirmed_location: Option<(f64, f64)> = sqlx::query_as(
        r#"
        SELECT latitude, longitude
        FROM posts
        WHERE id = $1
          AND author_id = $2
          AND (urgent = true OR post_type::text = 'emergency')
          AND geo_status = 'confirmed'
          AND latitude IS NOT NULL
          AND longitude IS NOT NULL
        "#,
    )
        .bind(post_id)
        .bind(reporter_user_id)
        .fetch_optional(&state.db)
        .await?;
    let (latitude, longitude) = confirmed_location.ok_or(ApiError::Forbidden)?;

    let mut tx = state.db.begin().await?;
    let row = sqlx::query(
        r#"
        INSERT INTO rescue_sessions (id, post_id, reporter_user_id, status, lat, lng, accuracy)
        VALUES ($1, $2, $3, 'active', $4, $5, $6)
        RETURNING id, post_id, reporter_user_id, status, lat, lng, accuracy, created_at, updated_at
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(post_id)
    .bind(reporter_user_id)
    .bind(latitude)
    .bind(longitude)
    .bind(payload.accuracy)
    .fetch_one(&mut *tx)
    .await?;

    let rescue = row_to_rescue(row);
    insert_location_point(&mut tx, &rescue.id, rescue.lat, rescue.lng, rescue.accuracy).await?;
    if let Ok(post_id) = Uuid::parse_str(&rescue.post_id) {
        sqlx::query(
            r#"
            UPDATE posts
            SET rescue_status = 'active',
                resolved_at = NULL
            WHERE id = $1
            "#,
        )
        .bind(post_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    let rescue_uuid = Uuid::parse_str(&rescue.id).ok();
    let _ = rescue_fanout::create_fanout_state_for_post(&state.db, post_id, rescue_uuid).await?;
    broadcast_rescue_event(&state, &rescue);
    Ok((StatusCode::CREATED, Json(RescueResponse { rescue })))
}

pub async fn update_location(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<LocationUpdateRequest>,
) -> Result<Json<RescueResponse>, ApiError> {
    payload
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;
    let claims = authenticate_claims(&state, &headers, None)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    let is_admin = matches!(claims.account_type, AccountType::Admin);
    rate_limit::check_key(
        &state,
        &format!("rescue:location:{user_id}"),
        state.config.throttle_limit * 6,
        StdDuration::from_secs(state.config.throttle_ttl_seconds),
    )
    .await?;

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
          AND (reporter_user_id = $5 OR $6::boolean = true)
        RETURNING id, post_id, reporter_user_id, status, lat, lng, accuracy, created_at, updated_at
        "#,
    )
    .bind(id)
    .bind(payload.lat)
    .bind(payload.lng)
    .bind(payload.accuracy)
    .bind(user_id)
    .bind(is_admin)
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
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<RescueResponse>, ApiError> {
    let claims = authenticate_claims(&state, &headers, None)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    let is_admin = matches!(claims.account_type, AccountType::Admin);
    rate_limit::check_key(
        &state,
        &format!("rescue:end:{user_id}"),
        state.config.throttle_limit,
        StdDuration::from_secs(state.config.throttle_ttl_seconds),
    )
    .await?;
    let mut tx = state.db.begin().await?;
    let row = sqlx::query(
        r#"
        UPDATE rescue_sessions
        SET status = 'ended',
            updated_at = now(),
            ended_at = COALESCE(ended_at, now())
        WHERE id = $1
          AND status = 'active'
          AND (reporter_user_id = $2 OR $3::boolean = true)
        RETURNING id, post_id, reporter_user_id, status, lat, lng, accuracy, created_at, updated_at
        "#,
    )
    .bind(id)
    .bind(user_id)
    .bind(is_admin)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApiError::NotFound)?;

    let rescue = row_to_rescue(row);
    if let Ok(post_id) = Uuid::parse_str(&rescue.post_id) {
        sqlx::query(
            r#"
            UPDATE posts
            SET rescue_status = 'resolved',
                resolved_at = COALESCE(resolved_at, now())
            WHERE id = $1
            "#,
        )
        .bind(post_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    broadcast_rescue_event(&state, &rescue);
    Ok(Json(RescueResponse { rescue }))
}

pub async fn incident(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<IncidentRequest>,
) -> Result<(StatusCode, Json<IncidentResponse>), ApiError> {
    payload
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;
    let user_id = authenticate_user(&state, &headers, None)?;
    rate_limit::check_key(
        &state,
        &format!("rescue:incident:{user_id}"),
        state.config.throttle_limit,
        StdDuration::from_secs(state.config.throttle_ttl_seconds),
    )
    .await?;

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

pub async fn respond(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<RescueResponseRequest>,
) -> Result<Json<RescueResponseAck>, ApiError> {
    payload
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;
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
    let user_id = authenticate_user(&state, &headers, None)?;
    rate_limit::check_key(
        &state,
        &format!("rescue:response:{id}:{user_id}"),
        state.config.throttle_limit * 3,
        StdDuration::from_secs(state.config.throttle_ttl_seconds),
    )
    .await?;

    let post_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT post_id FROM rescue_sessions WHERE id = $1 AND status = 'active'",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;

    let response = rescue_fanout::upsert_rescue_response(
        &state.db,
        post_id,
        Some(id),
        user_id,
        &payload.action,
        &payload.status,
        payload.lat,
        payload.lng,
        payload.eta_seconds,
    )
    .await?;

    Ok(Json(RescueResponseAck { response }))
}

pub async fn rescue_ws(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<WsAuthQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    authenticate_user(&state, &headers, query.access_token.as_deref())?;
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
        post_id: row
            .try_get::<Uuid, _>("post_id")
            .map(|value| value.to_string())
            .or_else(|_| row.try_get::<String, _>("post_id"))
            .unwrap_or_default(),
        reporter_user_id: optional_uuid(&row, "reporter_user_id"),
        reporter_name: optional_string(&row, "reporter_name"),
        reporter_email: optional_string(&row, "reporter_email"),
        reporter_role: optional_string(&row, "reporter_role").map(|role| match role.as_str() {
            "admin" => "ADMIN".to_string(),
            "ong" => "ONG".to_string(),
            "vet" => "VET".to_string(),
            _ => "USER".to_string(),
        }),
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

fn optional_uuid(row: &sqlx::postgres::PgRow, column: &str) -> Option<String> {
    row.try_get::<Option<Uuid>, _>(column)
        .ok()
        .flatten()
        .map(|value| value.to_string())
}

fn optional_string(row: &sqlx::postgres::PgRow, column: &str) -> Option<String> {
    row.try_get::<Option<String>, _>(column).ok().flatten()
}

fn broadcast_rescue_event(state: &AppState, rescue: &RescueSession) {
    let event = RescueEvent {
        rescue_id: rescue.id.clone(),
        post_id: rescue.post_id.clone(),
        status: rescue.status.clone(),
        lat: rescue.lat,
        lng: rescue.lng,
        accuracy: rescue.accuracy,
        updated_at: rescue.updated_at.clone(),
    };
    let _ = state.rescue_tx.send(event.clone());
    let bus = state.event_bus.clone();
    tokio::spawn(async move {
        bus.publish_rescue(&event).await;
    });
}

fn authenticate_user(
    state: &AppState,
    headers: &HeaderMap,
    query_token: Option<&str>,
) -> Result<Uuid, ApiError> {
    authenticate_claims(state, headers, query_token)
        .and_then(|claims| Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized))
}

fn authenticate_claims(
    state: &AppState,
    headers: &HeaderMap,
    query_token: Option<&str>,
) -> Result<auth_service::AccessClaims, ApiError> {
    let token = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
        })
        .or_else(|| query_token.filter(|value| !value.trim().is_empty()))
        .ok_or(ApiError::Unauthorized)?;

    auth_service::verify_access_token(&state.config, token).map_err(|_| ApiError::Unauthorized)
}

fn authenticate_admin(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<auth_service::AccessClaims, ApiError> {
    let token = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
        })
        .ok_or(ApiError::Unauthorized)?;

    let claims = auth_service::verify_access_token(&state.config, token)
        .map_err(|_| ApiError::Unauthorized)?;
    if !matches!(claims.account_type, AccountType::Admin) {
        return Err(ApiError::Forbidden);
    }

    Ok(claims)
}
