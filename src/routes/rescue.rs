use std::time::{Duration as StdDuration, Instant};

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
use serde_json::json;
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
const FINAL_REPORT_SCHEMA_VERSION: &str = "1.0.0";
const FINAL_REPORT_PROMPT_VERSION: &str = "rescue-final-report-v1";

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

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RescueFinalStatus {
    Rescued,
    NotFound,
    Died,
    Referred,
    Cancelled,
    FalseAlarm,
}

impl RescueFinalStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Rescued => "rescued",
            Self::NotFound => "not_found",
            Self::Died => "died",
            Self::Referred => "referred",
            Self::Cancelled => "cancelled",
            Self::FalseAlarm => "false_alarm",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "rescued" => Some(Self::Rescued),
            "not_found" => Some(Self::NotFound),
            "died" => Some(Self::Died),
            "referred" => Some(Self::Referred),
            "cancelled" => Some(Self::Cancelled),
            "false_alarm" => Some(Self::FalseAlarm),
            _ => None,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RescueFinalReport {
    pub id: String,
    pub rescue_id: Option<String>,
    pub post_id: String,
    pub status: String,
    pub summary: String,
    pub public_update: String,
    pub generated_by_ai: bool,
    pub publication_status: String,
    pub rejection_reason: Option<String>,
    pub approved_by: Option<String>,
    pub approved_at: Option<String>,
    pub rejected_by: Option<String>,
    pub rejected_at: Option<String>,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
    pub admin_notes: Option<String>,
    pub ai_model: Option<String>,
    pub ai_latency_ms: Option<i32>,
    pub ai_cost_cents: Option<i32>,
    pub prompt_version: Option<String>,
    pub schema_version: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RescueFinalReportPublic {
    pub status: String,
    pub public_update: String,
}

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct GenerateFinalReportRequest {
    pub status: Option<RescueFinalStatus>,
}

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ApproveFinalReportRequest {
    pub status: Option<RescueFinalStatus>,
    #[validate(length(min = 1, max = 280))]
    pub summary: Option<String>,
    #[validate(length(min = 1, max = 140))]
    pub public_update: Option<String>,
}

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct RejectFinalReportRequest {
    #[validate(length(min = 1, max = 240))]
    pub rejection_reason: String,
}

#[derive(Deserialize)]
struct AiFinalReportResponse {
    status_suggestion: RescueFinalStatus,
    summary: String,
    public_update: String,
    generated_by_ai: bool,
    ai_model: Option<String>,
    ai_latency_ms: Option<i32>,
    ai_cost_cents: Option<i32>,
    prompt_version: Option<String>,
    schema_version: Option<String>,
}

struct FinalReportDraft {
    status: RescueFinalStatus,
    summary: String,
    public_update: String,
    generated_by_ai: bool,
    ai_model: Option<String>,
    ai_latency_ms: Option<i32>,
    ai_cost_cents: Option<i32>,
    prompt_version: Option<String>,
    schema_version: String,
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
    if let Some(rescue_uuid) = rescue_uuid {
        if let Err(error) = insert_rescue_event(
            &state,
            rescue_uuid,
            post_id,
            "rescue_started",
            Some(reporter_user_id),
            Some("Resgate iniciado pelo autor do caso"),
            json!({
                "lat": rescue.lat,
                "lng": rescue.lng,
                "accuracy": rescue.accuracy
            }),
        )
        .await
        {
            tracing::warn!(?error, rescue_id = %rescue.id, "failed to persist rescue_started event");
        }
    }
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

    if let Ok(post_id) = Uuid::parse_str(&rescue.post_id) {
        if let Err(error) = insert_rescue_event(
            &state,
            id,
            post_id,
            "location_updated",
            Some(user_id),
            Some("Localizacao do resgate atualizada"),
            json!({
                "lat": rescue.lat,
                "lng": rescue.lng,
                "accuracy": rescue.accuracy
            }),
        )
        .await
        {
            tracing::warn!(?error, rescue_id = %id, "failed to persist location_updated event");
        }
    }
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
    if let Ok(post_id) = Uuid::parse_str(&rescue.post_id) {
        if let Err(error) = insert_rescue_event(
            &state,
            id,
            post_id,
            "rescue_ended",
            Some(user_id),
            Some("Resgate encerrado e marcado como resolvido"),
            json!({ "status": "resolved", "finalStatus": "rescued" }),
        )
        .await
        {
            tracing::warn!(?error, rescue_id = %id, "failed to persist rescue_ended event");
        }
    }
    let auto_state = state.clone();
    tokio::spawn(async move {
        if let Err(error) =
            create_or_update_final_report(&auto_state, id, Some(RescueFinalStatus::Rescued), None)
                .await
        {
            tracing::warn!(
                ?error,
                rescue_id = %id,
                "failed to auto-generate rescue final report after rescue end"
            );
        }
    });
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

    let post_id = sqlx::query_scalar::<_, Uuid>("SELECT post_id FROM rescue_sessions WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;

    let post_id = post_id.ok_or(ApiError::NotFound)?;

    let incident_id = Uuid::now_v7();
    let attachments_count = payload.attachments.len();
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

    if let Err(error) = insert_rescue_event(
        &state,
        id,
        post_id,
        "incident_reported",
        Some(user_id),
        Some("Incidente reportado durante o resgate"),
        json!({ "incidentId": incident_id, "attachmentsCount": attachments_count }),
    )
    .await
    {
        tracing::warn!(?error, rescue_id = %id, "failed to persist incident_reported event");
    }

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

    let event_type = match response.status.as_str() {
        "arrived" => "volunteer_arrived",
        "cancelled" => "volunteer_cancelled",
        _ => "volunteer_confirmed",
    };
    if let Err(error) = insert_rescue_event(
        &state,
        id,
        post_id,
        event_type,
        Some(user_id),
        Some("Resposta de voluntario registrada"),
        json!({
            "action": payload.action,
            "status": payload.status,
            "etaSeconds": payload.eta_seconds
        }),
    )
    .await
    {
        tracing::warn!(?error, rescue_id = %id, "failed to persist rescue response event");
    }

    Ok(Json(RescueResponseAck { response }))
}

pub async fn generate_final_report(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    payload: Option<Json<GenerateFinalReportRequest>>,
) -> Result<(StatusCode, Json<RescueFinalReport>), ApiError> {
    let claims = authenticate_claims(&state, &headers, None)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    let is_admin = matches!(claims.account_type, AccountType::Admin);
    rate_limit::check_key(
        &state,
        &format!("rescue:final-report:generate:{id}:{user_id}"),
        state.config.throttle_limit,
        StdDuration::from_secs(state.config.throttle_ttl_seconds),
    )
    .await?;
    ensure_rescue_manager(&state, id, user_id, is_admin).await?;

    if let Some(Json(payload)) = payload.as_ref() {
        payload
            .validate()
            .map_err(|error| ApiError::Validation(error.to_string()))?;
    }
    let requested_status = payload.and_then(|Json(value)| value.status);
    let report = create_or_update_final_report(&state, id, requested_status, Some(user_id)).await?;
    Ok((StatusCode::CREATED, Json(report)))
}

pub async fn get_final_report(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let report = load_final_report_by_rescue(&state, id)
        .await?
        .ok_or(ApiError::NotFound)?;

    if let Ok(claims) = authenticate_claims(&state, &headers, None) {
        let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
        let is_admin = matches!(claims.account_type, AccountType::Admin);
        if can_manage_rescue(&state, id, user_id, is_admin).await? {
            return Ok(Json(
                serde_json::to_value(report).map_err(|_| ApiError::Internal)?,
            ));
        }
    }

    if report.publication_status == "published" {
        return Ok(Json(json!(RescueFinalReportPublic {
            status: report.status,
            public_update: report.public_update,
        })));
    }

    Err(ApiError::NotFound)
}

pub async fn approve_final_report(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<ApproveFinalReportRequest>,
) -> Result<Json<RescueFinalReport>, ApiError> {
    payload
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;
    let claims = authenticate_claims(&state, &headers, None)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    let is_admin = matches!(claims.account_type, AccountType::Admin);
    rate_limit::check_key(
        &state,
        &format!("rescue:final-report:approve:{id}:{user_id}"),
        state.config.throttle_limit,
        StdDuration::from_secs(state.config.throttle_ttl_seconds),
    )
    .await?;
    ensure_rescue_manager(&state, id, user_id, is_admin).await?;

    let report = sqlx::query(
        r#"
        UPDATE rescue_final_reports
        SET status = COALESCE($2, status),
            summary = COALESCE($3, summary),
            public_update = COALESCE($4, public_update),
            publication_status = 'published',
            approved_by = $5,
            approved_at = now(),
            updated_by = $5,
            updated_at = now()
        WHERE rescue_id = $1
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(payload.status.map(|status| status.as_str()))
    .bind(payload.summary)
    .bind(payload.public_update)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .map(row_to_final_report)
    .ok_or(ApiError::NotFound)?;

    tracing::info!(
        rescue_id = %id,
        report_id = %report.id,
        approved_by = %user_id,
        "rescue final report published"
    );

    Ok(Json(report))
}

pub async fn reject_final_report(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<RejectFinalReportRequest>,
) -> Result<Json<RescueFinalReport>, ApiError> {
    payload
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;
    let reason = payload.rejection_reason.trim();
    if reason.is_empty() {
        return Err(ApiError::Validation("rejectionReason is required".into()));
    }
    let claims = authenticate_claims(&state, &headers, None)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    let is_admin = matches!(claims.account_type, AccountType::Admin);
    rate_limit::check_key(
        &state,
        &format!("rescue:final-report:reject:{id}:{user_id}"),
        state.config.throttle_limit,
        StdDuration::from_secs(state.config.throttle_ttl_seconds),
    )
    .await?;
    ensure_rescue_manager(&state, id, user_id, is_admin).await?;

    let report = sqlx::query(
        r#"
        UPDATE rescue_final_reports
        SET publication_status = 'rejected',
            rejection_reason = $2,
            rejected_by = $3,
            rejected_at = now(),
            updated_by = $3,
            updated_at = now()
        WHERE rescue_id = $1
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(reason)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .map(row_to_final_report)
    .ok_or(ApiError::NotFound)?;

    tracing::info!(
        rescue_id = %id,
        report_id = %report.id,
        rejected_by = %user_id,
        "rescue final report rejected"
    );

    Ok(Json(report))
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

fn row_to_final_report(row: sqlx::postgres::PgRow) -> RescueFinalReport {
    RescueFinalReport {
        id: row.get::<Uuid, _>("id").to_string(),
        rescue_id: row
            .get::<Option<Uuid>, _>("rescue_id")
            .map(|value| value.to_string()),
        post_id: row.get::<Uuid, _>("post_id").to_string(),
        status: row.get("status"),
        summary: row.get("summary"),
        public_update: row.get("public_update"),
        generated_by_ai: row.get("generated_by_ai"),
        publication_status: row.get("publication_status"),
        rejection_reason: row.get("rejection_reason"),
        approved_by: optional_uuid(&row, "approved_by"),
        approved_at: optional_datetime(&row, "approved_at"),
        rejected_by: optional_uuid(&row, "rejected_by"),
        rejected_at: optional_datetime(&row, "rejected_at"),
        created_by: optional_uuid(&row, "created_by"),
        updated_by: optional_uuid(&row, "updated_by"),
        admin_notes: row.get("admin_notes"),
        ai_model: row.get("ai_model"),
        ai_latency_ms: row.get("ai_latency_ms"),
        ai_cost_cents: row.get("ai_cost_cents"),
        prompt_version: row.get("prompt_version"),
        schema_version: row.get("schema_version"),
        created_at: row
            .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
            .to_rfc3339(),
        updated_at: row
            .get::<chrono::DateTime<chrono::Utc>, _>("updated_at")
            .to_rfc3339(),
    }
}

fn optional_datetime(row: &sqlx::postgres::PgRow, column: &str) -> Option<String> {
    row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(column)
        .ok()
        .flatten()
        .map(|value| value.to_rfc3339())
}

pub(crate) async fn load_published_final_report_for_post(
    state: &AppState,
    post_id: Uuid,
) -> Result<Option<crate::domain::RescueFinalReportPublic>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT status, public_update
        FROM rescue_final_reports
        WHERE post_id = $1
          AND publication_status = 'published'
        "#,
    )
    .bind(post_id)
    .fetch_optional(&state.db)
    .await?;

    Ok(row.map(|row| crate::domain::RescueFinalReportPublic {
        status: row.get("status"),
        public_update: row.get("public_update"),
    }))
}

pub(crate) async fn load_published_final_reports_for_posts(
    state: &AppState,
    post_ids: &[Uuid],
) -> Result<std::collections::HashMap<String, crate::domain::RescueFinalReportPublic>, sqlx::Error>
{
    if post_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let rows = sqlx::query(
        r#"
        SELECT post_id::text AS post_id, status, public_update
        FROM rescue_final_reports
        WHERE post_id = ANY($1)
          AND publication_status = 'published'
        "#,
    )
    .bind(post_ids)
    .fetch_all(&state.db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| {
            (
                row.get("post_id"),
                crate::domain::RescueFinalReportPublic {
                    status: row.get("status"),
                    public_update: row.get("public_update"),
                },
            )
        })
        .collect())
}

async fn load_final_report_by_rescue(
    state: &AppState,
    rescue_id: Uuid,
) -> Result<Option<RescueFinalReport>, sqlx::Error> {
    sqlx::query("SELECT * FROM rescue_final_reports WHERE rescue_id = $1")
        .bind(rescue_id)
        .fetch_optional(&state.db)
        .await
        .map(|row| row.map(row_to_final_report))
}

async fn ensure_rescue_manager(
    state: &AppState,
    rescue_id: Uuid,
    user_id: Uuid,
    is_admin: bool,
) -> Result<(), ApiError> {
    if can_manage_rescue(state, rescue_id, user_id, is_admin).await? {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}

async fn can_manage_rescue(
    state: &AppState,
    rescue_id: Uuid,
    user_id: Uuid,
    is_admin: bool,
) -> Result<bool, ApiError> {
    if is_admin {
        return Ok(true);
    }

    let owns = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
          SELECT 1
          FROM rescue_sessions rs
          INNER JOIN posts p ON p.id = rs.post_id
          WHERE rs.id = $1
            AND p.author_id = $2
        )
        "#,
    )
    .bind(rescue_id)
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;

    Ok(owns)
}

async fn create_or_update_final_report(
    state: &AppState,
    rescue_id: Uuid,
    requested_status: Option<RescueFinalStatus>,
    created_by: Option<Uuid>,
) -> Result<RescueFinalReport, ApiError> {
    if let Some(existing) = load_final_report_by_rescue(state, rescue_id).await? {
        if matches!(
            existing.publication_status.as_str(),
            "pending_approval" | "published"
        ) {
            return Ok(existing);
        }
    }

    let post_id =
        sqlx::query_scalar::<_, Uuid>("SELECT post_id FROM rescue_sessions WHERE id = $1")
            .bind(rescue_id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(ApiError::NotFound)?;

    let draft = generate_final_report_draft(state, rescue_id, post_id, requested_status).await;
    let draft = draft.unwrap_or_else(|error| {
        tracing::warn!(?error, %rescue_id, %post_id, "AI final report failed; using deterministic fallback");
        deterministic_final_report(requested_status.unwrap_or(RescueFinalStatus::Rescued))
    });

    let row = sqlx::query(
        r#"
        INSERT INTO rescue_final_reports (
          id, rescue_id, post_id, status, summary, public_update, generated_by_ai,
          publication_status, created_by, updated_by, ai_model, ai_latency_ms,
          ai_cost_cents, prompt_version, schema_version
        )
        VALUES (
          $1, $2, $3, $4, $5, $6, $7, 'pending_approval', $8, $8, $9, $10, $11, $12, $13
        )
        ON CONFLICT (post_id) DO UPDATE
        SET rescue_id = EXCLUDED.rescue_id,
            status = EXCLUDED.status,
            summary = EXCLUDED.summary,
            public_update = EXCLUDED.public_update,
            generated_by_ai = EXCLUDED.generated_by_ai,
            publication_status = 'pending_approval',
            rejection_reason = NULL,
            updated_by = EXCLUDED.updated_by,
            ai_model = EXCLUDED.ai_model,
            ai_latency_ms = EXCLUDED.ai_latency_ms,
            ai_cost_cents = EXCLUDED.ai_cost_cents,
            prompt_version = EXCLUDED.prompt_version,
            schema_version = EXCLUDED.schema_version,
            updated_at = now()
        WHERE rescue_final_reports.publication_status NOT IN ('pending_approval', 'published')
        RETURNING *
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(rescue_id)
    .bind(post_id)
    .bind(draft.status.as_str())
    .bind(draft.summary)
    .bind(draft.public_update)
    .bind(draft.generated_by_ai)
    .bind(created_by)
    .bind(draft.ai_model)
    .bind(draft.ai_latency_ms)
    .bind(draft.ai_cost_cents)
    .bind(draft.prompt_version)
    .bind(draft.schema_version)
    .fetch_optional(&state.db)
    .await?;

    if let Some(row) = row {
        return Ok(row_to_final_report(row));
    }

    load_final_report_by_rescue(state, rescue_id)
        .await?
        .ok_or(ApiError::Conflict("final report already exists".into()))
}

async fn generate_final_report_draft(
    state: &AppState,
    rescue_id: Uuid,
    post_id: Uuid,
    requested_status: Option<RescueFinalStatus>,
) -> Result<FinalReportDraft, ApiError> {
    let fallback_status = requested_status.unwrap_or(RescueFinalStatus::Rescued);
    let worker_url = state.config.ai_worker_url.trim().trim_end_matches('/');
    if worker_url.is_empty() {
        return Ok(deterministic_final_report(fallback_status));
    }
    let ai_context = build_final_report_ai_context(state, rescue_id, post_id, requested_status).await?;

    let started = Instant::now();
    let response = reqwest::Client::new()
        .post(format!("{worker_url}/ai/final-rescue-report"))
        .json(&ai_context)
        .timeout(StdDuration::from_secs(3))
        .send()
        .await
        .map_err(|_| ApiError::ServiceUnavailable)?;

    if !response.status().is_success() {
        return Err(ApiError::ServiceUnavailable);
    }

    let ai = response
        .json::<AiFinalReportResponse>()
        .await
        .map_err(|_| ApiError::ServiceUnavailable)?;

    tracing::info!(
        rescue_id = %rescue_id,
        post_id = %post_id,
        generated_by_ai = ai.generated_by_ai,
        latency_ms = started.elapsed().as_millis() as u64,
        model = ai.ai_model.as_deref().unwrap_or("fallback"),
        "rescue final report worker response"
    );

    Ok(FinalReportDraft {
        status: ai.status_suggestion,
        summary: trim_report_text(ai.summary, 280),
        public_update: trim_report_text(ai.public_update, 140),
        generated_by_ai: ai.generated_by_ai,
        ai_model: ai.ai_model,
        ai_latency_ms: ai
            .ai_latency_ms
            .or_else(|| Some(started.elapsed().as_millis().min(i32::MAX as u128) as i32)),
        ai_cost_cents: ai.ai_cost_cents,
        prompt_version: ai
            .prompt_version
            .or_else(|| Some(FINAL_REPORT_PROMPT_VERSION.into())),
        schema_version: ai
            .schema_version
            .unwrap_or_else(|| FINAL_REPORT_SCHEMA_VERSION.to_string()),
    })
}

async fn build_final_report_ai_context(
    state: &AppState,
    rescue_id: Uuid,
    post_id: Uuid,
    requested_status: Option<RescueFinalStatus>,
) -> Result<serde_json::Value, ApiError> {
    let post = sqlx::query(
        r#"
        SELECT id::text, post_type::text, animal_type, name, description,
               neighborhood, location_label, rescue_status, created_at, resolved_at
        FROM posts
        WHERE id = $1
        "#,
    )
    .bind(post_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;

    let rescue = sqlx::query(
        r#"
        SELECT id::text, status, lat, lng, accuracy, created_at, updated_at, ended_at
        FROM rescue_sessions
        WHERE id = $1
        "#,
    )
    .bind(rescue_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;

    let events = sqlx::query(
        r#"
        SELECT type, actor_id::text AS actor_id, message, metadata, created_at
        FROM rescue_events
        WHERE rescue_id = $1
        ORDER BY created_at ASC
        LIMIT 80
        "#,
    )
    .bind(rescue_id)
    .fetch_all(&state.db)
    .await?;

    let responses = sqlx::query(
        r#"
        SELECT action, status, eta_seconds, created_at, updated_at
        FROM rescue_responses
        WHERE rescue_session_id = $1 OR post_id = $2
        ORDER BY updated_at ASC
        LIMIT 80
        "#,
    )
    .bind(rescue_id)
    .bind(post_id)
    .fetch_all(&state.db)
    .await?;

    let incidents = sqlx::query(
        r#"
        SELECT description, status, created_at
        FROM rescue_incidents
        WHERE rescue_id = $1
        ORDER BY created_at ASC
        LIMIT 40
        "#,
    )
    .bind(rescue_id)
    .fetch_all(&state.db)
    .await?;

    Ok(json!({
        "rescue_id": rescue_id.to_string(),
        "post_id": post_id.to_string(),
        "requested_status": requested_status.map(|status| status.as_str()),
        "post": {
            "id": post.get::<String, _>("id"),
            "type": post.get::<String, _>("post_type"),
            "animalType": post.get::<String, _>("animal_type"),
            "name": redact_sensitive_text(post.get::<String, _>("name")),
            "description": redact_sensitive_text(post.get::<String, _>("description")),
            "neighborhood": post.get::<Option<String>, _>("neighborhood"),
            "locationLabel": post.get::<Option<String>, _>("location_label"),
            "rescueStatus": post.get::<String, _>("rescue_status"),
            "createdAt": post.get::<chrono::DateTime<chrono::Utc>, _>("created_at").to_rfc3339(),
            "resolvedAt": post.get::<Option<chrono::DateTime<chrono::Utc>>, _>("resolved_at").map(|value| value.to_rfc3339()),
        },
        "rescue": {
            "id": rescue.get::<String, _>("id"),
            "status": rescue.get::<String, _>("status"),
            "lat": rescue.get::<f64, _>("lat"),
            "lng": rescue.get::<f64, _>("lng"),
            "accuracy": rescue.get::<Option<f64>, _>("accuracy"),
            "createdAt": rescue.get::<chrono::DateTime<chrono::Utc>, _>("created_at").to_rfc3339(),
            "updatedAt": rescue.get::<chrono::DateTime<chrono::Utc>, _>("updated_at").to_rfc3339(),
            "endedAt": rescue.get::<Option<chrono::DateTime<chrono::Utc>>, _>("ended_at").map(|value| value.to_rfc3339()),
        },
        "events": events.into_iter().map(|row| json!({
            "type": row.get::<String, _>("type"),
            "actorId": row.get::<Option<String>, _>("actor_id"),
            "message": row.get::<Option<String>, _>("message"),
            "metadata": row.get::<serde_json::Value, _>("metadata"),
            "createdAt": row.get::<chrono::DateTime<chrono::Utc>, _>("created_at").to_rfc3339(),
        })).collect::<Vec<_>>(),
        "rescue_responses": responses.into_iter().map(|row| json!({
            "action": row.get::<String, _>("action"),
            "status": row.get::<String, _>("status"),
            "etaSeconds": row.get::<Option<i32>, _>("eta_seconds"),
            "createdAt": row.get::<chrono::DateTime<chrono::Utc>, _>("created_at").to_rfc3339(),
            "updatedAt": row.get::<chrono::DateTime<chrono::Utc>, _>("updated_at").to_rfc3339(),
        })).collect::<Vec<_>>(),
        "incidents": incidents.into_iter().map(|row| json!({
            "description": redact_sensitive_text(row.get::<String, _>("description")),
            "status": row.get::<String, _>("status"),
            "createdAt": row.get::<chrono::DateTime<chrono::Utc>, _>("created_at").to_rfc3339(),
        })).collect::<Vec<_>>(),
        "chat_summary": null,
    }))
}

fn deterministic_final_report(status: RescueFinalStatus) -> FinalReportDraft {
    let (summary, public_update) = final_report_fallback_text(status);
    FinalReportDraft {
        status,
        summary: summary.to_string(),
        public_update: public_update.to_string(),
        generated_by_ai: false,
        ai_model: None,
        ai_latency_ms: Some(0),
        ai_cost_cents: Some(0),
        prompt_version: Some(FINAL_REPORT_PROMPT_VERSION.into()),
        schema_version: FINAL_REPORT_SCHEMA_VERSION.into(),
    }
}

fn final_report_fallback_text(status: RescueFinalStatus) -> (&'static str, &'static str) {
    match status {
        RescueFinalStatus::Rescued => (
            "Animal localizado e encaminhado para atendimento ou segurança.",
            "Atualização: o animal foi resgatado e está recebendo cuidados.",
        ),
        RescueFinalStatus::NotFound => (
            "A equipe não conseguiu localizar o animal após acompanhamento do caso.",
            "Atualização: o animal ainda não foi localizado.",
        ),
        RescueFinalStatus::Died => (
            "O animal foi encontrado sem vida.",
            "Atualização: o caso foi encerrado após a confirmação do óbito.",
        ),
        RescueFinalStatus::Referred => (
            "O caso foi encaminhado para responsável, ONG, clínica ou órgão competente.",
            "Atualização: o caso foi encaminhado para acompanhamento especializado.",
        ),
        RescueFinalStatus::Cancelled => (
            "O chamado foi cancelado antes da conclusão.",
            "Atualização: o chamado foi cancelado.",
        ),
        RescueFinalStatus::FalseAlarm => (
            "O alerta foi avaliado como falso ou equivocado.",
            "Atualização: o alerta foi encerrado após verificação.",
        ),
    }
}

#[cfg(test)]
mod final_report_tests {
    use super::*;

    #[test]
    fn parses_final_status_values() {
        assert_eq!(
            RescueFinalStatus::from_str("false_alarm"),
            Some(RescueFinalStatus::FalseAlarm)
        );
        assert_eq!(
            RescueFinalStatus::from_str("rescued"),
            Some(RescueFinalStatus::Rescued)
        );
        assert_eq!(RescueFinalStatus::from_str("unknown"), None);
    }

    #[test]
    fn fallback_never_publishes_and_uses_requested_status() {
        let draft = deterministic_final_report(RescueFinalStatus::Cancelled);
        assert!(!draft.generated_by_ai);
        assert_eq!(draft.status, RescueFinalStatus::Cancelled);
        assert_eq!(draft.public_update, "Atualização: o chamado foi cancelado.");
    }
}

fn trim_report_text(value: String, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    trimmed.chars().take(max_chars).collect()
}

fn redact_sensitive_text(value: String) -> String {
    value
        .split_whitespace()
        .map(|token| {
            let digits = token.chars().filter(|char| char.is_ascii_digit()).count();
            if token.contains('@') && token.contains('.') {
                "[email]".to_string()
            } else if digits >= 8 {
                "[telefone]".to_string()
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

async fn insert_rescue_event(
    state: &AppState,
    rescue_id: Uuid,
    post_id: Uuid,
    event_type: &str,
    actor_id: Option<Uuid>,
    message: Option<&str>,
    metadata: serde_json::Value,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"
        INSERT INTO rescue_events (id, rescue_id, post_id, type, actor_id, message, metadata)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(rescue_id)
    .bind(post_id)
    .bind(event_type)
    .bind(actor_id)
    .bind(message)
    .bind(metadata)
    .execute(&state.db)
    .await?;

    Ok(())
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
