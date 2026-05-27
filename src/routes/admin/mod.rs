use std::time::Duration as StdDuration;

use axum::{
    extract::{Path, State},
    http::{header::AUTHORIZATION, HeaderMap},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    domain::AccountType,
    error::ApiError,
    routes::auth::audit_event,
    services::{auth as auth_service, rate_limit},
    state::AppState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationStatusRequest {
    pub status: String,
    pub rejection_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAdminUserRequest {
    pub name: Option<String>,
    pub phone: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateKybDocumentRequest {
    pub document_type: String,
    pub object_key: String,
    pub public_url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewKybDocumentRequest {
    pub status: String,
    pub rejection_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewModerationJobRequest {
    pub status: String,
    pub score: Option<i16>,
    pub labels: Option<Vec<String>>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KybDocument {
    pub id: String,
    pub ong_id: String,
    pub document_type: String,
    pub object_key: String,
    pub public_url: String,
    pub status: String,
    pub reviewer_user_id: Option<String>,
    pub rejection_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub reviewed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModerationJob {
    pub id: String,
    pub subject_type: String,
    pub subject_id: String,
    pub image_url: Option<String>,
    pub status: String,
    pub score: Option<i16>,
    pub labels: Vec<String>,
    pub provider: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostReport {
    pub id: String,
    pub post_id: String,
    pub reporter_user_id: String,
    pub reason: String,
    pub details: Option<String>,
    pub severity: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RescueFinalReportAdmin {
    pub id: String,
    pub rescue_id: Option<String>,
    pub post_id: String,
    pub post_title: String,
    pub post_type: String,
    pub status: String,
    pub summary: String,
    pub public_update: String,
    pub generated_by_ai: bool,
    pub publication_status: String,
    pub rejection_reason: Option<String>,
    pub approved_by: Option<String>,
    pub approved_at: Option<DateTime<Utc>>,
    pub rejected_by: Option<String>,
    pub rejected_at: Option<DateTime<Utc>>,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
    pub admin_notes: Option<String>,
    pub ai_model: Option<String>,
    pub ai_latency_ms: Option<i32>,
    pub ai_cost_cents: Option<i32>,
    pub prompt_version: Option<String>,
    pub schema_version: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminOngProfile {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub full_name: String,
    pub legal_name: String,
    pub email: String,
    pub avatar: Option<String>,
    pub phone: Option<String>,
    pub cep: Option<String>,
    pub street: Option<String>,
    pub number: Option<String>,
    pub complement: Option<String>,
    pub neighborhood: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub foundation_year: Option<i32>,
    pub cnpj: Option<String>,
    pub ong_type: Option<String>,
    pub verification_status: String,
    pub rejection_reason: Option<String>,
    pub verified: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub five_star_review_count: i32,
    pub monthly_bookings_count: i32,
    pub total_earnings: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminUserProfile {
    pub id: String,
    pub name: String,
    pub full_name: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub role: String,
    pub status: String,
    pub verified: bool,
    pub verification_status: Option<String>,
    pub phone: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub completed_bookings_count: i32,
    pub total_spent: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueInfo {
    pub name: String,
    pub active: i64,
    pub waiting: i64,
    pub completed: i64,
    pub failed: i64,
    pub delayed: i64,
    pub paused: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueJob {
    pub id: String,
    pub name: String,
    pub data: serde_json::Value,
    pub status: String,
    pub progress: i32,
    pub attempts_made: i32,
    pub failed_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

pub async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<AdminUserProfile>>, ApiError> {
    authenticate_admin(&state, &headers)?;

    let rows = sqlx::query(
        r#"
        SELECT
          u.id,
          u.name,
          u.email::text AS email,
          u.avatar_url,
          u.account_type::text AS account_type,
          u.verified,
          u.created_at,
          op.verification_status,
          op.contact_phone,
          op.city,
          op.state
        FROM users u
        LEFT JOIN ong_profiles op ON op.user_id = u.id
        ORDER BY u.created_at DESC
        LIMIT 500
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows.into_iter().map(row_to_admin_user).collect()))
}

pub async fn get_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<AdminUserProfile>, ApiError> {
    authenticate_admin(&state, &headers)?;

    let row = sqlx::query(
        r#"
        SELECT
          u.id,
          u.name,
          u.email::text AS email,
          u.avatar_url,
          u.account_type::text AS account_type,
          u.verified,
          u.created_at,
          op.verification_status,
          op.contact_phone,
          op.city,
          op.state
        FROM users u
        LEFT JOIN ong_profiles op ON op.user_id = u.id
        WHERE u.id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;

    Ok(Json(row_to_admin_user(row)))
}

pub async fn update_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateAdminUserRequest>,
) -> Result<Json<AdminUserProfile>, ApiError> {
    authenticate_admin(&state, &headers)?;

    if let Some(name) = payload
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sqlx::query("UPDATE users SET name = $1 WHERE id = $2")
            .bind(name)
            .bind(id)
            .execute(&state.db)
            .await?;
    }

    if let Some(phone) = payload.phone.as_deref() {
        sqlx::query("UPDATE ong_profiles SET contact_phone = $1 WHERE user_id = $2")
            .bind(phone.trim())
            .bind(id)
            .execute(&state.db)
            .await?;
    }

    get_user(State(state), headers, Path(id)).await
}

pub async fn delete_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    authenticate_admin(&state, &headers)?;

    let result = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

pub async fn pending_ongs(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<AdminOngProfile>>, ApiError> {
    authenticate_admin(&state, &headers)?;

    let rows = sqlx::query(
        r#"
        SELECT
          op.id,
          op.user_id,
          u.name,
          u.email::text AS email,
          u.avatar_url,
          op.legal_name,
          op.contact_phone,
          op.cep,
          op.street,
          op.number,
          op.complement,
          op.neighborhood,
          op.city,
          op.state,
          op.foundation_year,
          op.cnpj,
          op.area_type,
          op.verification_status,
          op.verification_rejection_reason,
          op.created_at,
          op.updated_at
        FROM ong_profiles op
        JOIN users u ON u.id = op.user_id
        WHERE op.verification_status = 'PENDING_MANUAL_REVIEW'
        ORDER BY op.created_at ASC
        LIMIT 100
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows.into_iter().map(row_to_admin_ong).collect()))
}

pub async fn update_ong_verification_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<VerificationStatusRequest>,
) -> Result<Json<AdminOngProfile>, ApiError> {
    let admin = authenticate_admin(&state, &headers)?;
    let reviewer_id = Uuid::parse_str(&admin.sub).map_err(|_| ApiError::Unauthorized)?;
    let status = normalize_status(&payload.status)
        .ok_or_else(|| ApiError::Validation("invalid verification status".into()))?;
    let rejection_reason = payload
        .rejection_reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if matches!(status, "REJECTED" | "BLOCKED") && rejection_reason.is_none() {
        return Err(ApiError::Validation(
            "rejectionReason is required for rejected or blocked ONGs".into(),
        ));
    }

    let mut tx = state.db.begin().await?;
    let updated = sqlx::query(
        r#"
        UPDATE ong_profiles
        SET verification_status = $1,
            verification_reviewed_at = now(),
            verification_reviewer_user_id = $2,
            verification_rejection_reason = CASE
              WHEN $1 = 'APPROVED' THEN NULL
              ELSE $3
            END,
            verified_at = CASE
              WHEN $1 = 'APPROVED' THEN COALESCE(verified_at, now())
              ELSE NULL
            END,
            updated_at = now()
        WHERE id = $4
        RETURNING user_id
        "#,
    )
    .bind(status)
    .bind(reviewer_id)
    .bind(rejection_reason)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApiError::NotFound)?;

    let user_id: Uuid = updated.get("user_id");
    sqlx::query("UPDATE users SET verified = $1 WHERE id = $2")
        .bind(status == "APPROVED")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    let row = sqlx::query(
        r#"
        SELECT
          op.id,
          op.user_id,
          u.name,
          u.email::text AS email,
          u.avatar_url,
          op.legal_name,
          op.contact_phone,
          op.cep,
          op.street,
          op.number,
          op.complement,
          op.neighborhood,
          op.city,
          op.state,
          op.foundation_year,
          op.cnpj,
          op.area_type,
          op.verification_status,
          op.verification_rejection_reason,
          op.created_at,
          op.updated_at
        FROM ong_profiles op
        JOIN users u ON u.id = op.user_id
        WHERE op.id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;

    Ok(Json(row_to_admin_ong(row)))
}

pub async fn list_kyb_documents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<KybDocument>>, ApiError> {
    authenticate_admin(&state, &headers)?;
    let rows = sqlx::query(
        r#"
        SELECT id, ong_id, document_type, object_key, public_url, status,
               reviewer_user_id, rejection_reason, created_at, reviewed_at
        FROM ong_kyb_documents
        WHERE ong_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows.into_iter().map(row_to_kyb_document).collect()))
}

pub async fn create_kyb_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<CreateKybDocumentRequest>,
) -> Result<Json<KybDocument>, ApiError> {
    authenticate_admin(&state, &headers)?;
    let row = sqlx::query(
        r#"
        INSERT INTO ong_kyb_documents (ong_id, document_type, object_key, public_url)
        VALUES ($1, $2, $3, $4)
        RETURNING id, ong_id, document_type, object_key, public_url, status,
                  reviewer_user_id, rejection_reason, created_at, reviewed_at
        "#,
    )
    .bind(id)
    .bind(payload.document_type.trim())
    .bind(payload.object_key.trim())
    .bind(payload.public_url.trim())
    .fetch_one(&state.db)
    .await?;

    Ok(Json(row_to_kyb_document(row)))
}

pub async fn create_my_kyb_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateKybDocumentRequest>,
) -> Result<Json<KybDocument>, ApiError> {
    let claims = authenticate_any(&state, &headers)?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    if !matches!(claims.account_type, AccountType::Ong | AccountType::Admin) {
        return Err(ApiError::Forbidden);
    }
    rate_limit::check_key(
        &state,
        &format!("kyb:create:{user_id}"),
        state.config.throttle_limit,
        StdDuration::from_secs(state.config.throttle_ttl_seconds * 5),
    )
    .await?;

    let ong_id: Uuid = sqlx::query_scalar("SELECT id FROM ong_profiles WHERE user_id = $1")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::Forbidden)?;
    let document_type = normalize_document_type(payload.document_type.trim())?;
    if payload.object_key.trim().is_empty() || payload.public_url.trim().is_empty() {
        return Err(ApiError::Validation(
            "objectKey and publicUrl are required".into(),
        ));
    }

    let row = sqlx::query(
        r#"
        INSERT INTO ong_kyb_documents (ong_id, document_type, object_key, public_url)
        VALUES ($1, $2, $3, $4)
        RETURNING id, ong_id, document_type, object_key, public_url, status,
                  reviewer_user_id, rejection_reason, created_at, reviewed_at
        "#,
    )
    .bind(ong_id)
    .bind(document_type)
    .bind(payload.object_key.trim())
    .bind(payload.public_url.trim())
    .fetch_one(&state.db)
    .await?;

    Ok(Json(row_to_kyb_document(row)))
}

pub async fn review_kyb_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(document_id): Path<Uuid>,
    Json(payload): Json<ReviewKybDocumentRequest>,
) -> Result<Json<KybDocument>, ApiError> {
    let admin = authenticate_admin(&state, &headers)?;
    let reviewer_id = Uuid::parse_str(&admin.sub).map_err(|_| ApiError::Unauthorized)?;
    let status = normalize_review_status(&payload.status)?;
    let row = sqlx::query(
        r#"
        UPDATE ong_kyb_documents
        SET status = $1,
            reviewer_user_id = $2,
            rejection_reason = CASE WHEN $1 = 'approved' THEN NULL ELSE $3 END,
            reviewed_at = now()
        WHERE id = $4
        RETURNING id, ong_id, document_type, object_key, public_url, status,
                  reviewer_user_id, rejection_reason, created_at, reviewed_at
        "#,
    )
    .bind(status)
    .bind(reviewer_id)
    .bind(payload.rejection_reason.as_deref())
    .bind(document_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;

    let ong_id: Uuid = row.get("ong_id");
    if status == "approved" {
        maybe_auto_approve_ong_from_kyb(&state, ong_id, reviewer_id).await?;
    }

    Ok(Json(row_to_kyb_document(row)))
}

pub async fn list_moderation_jobs(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ModerationJob>>, ApiError> {
    authenticate_admin(&state, &headers)?;
    let rows = sqlx::query(
        r#"
        SELECT id, subject_type, subject_id, image_url, status, score, labels,
               provider, error, created_at, updated_at
        FROM moderation_jobs
        WHERE status IN ('queued', 'needs_review')
        ORDER BY created_at ASC
        LIMIT 200
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows.into_iter().map(row_to_moderation_job).collect()))
}

pub async fn review_moderation_job(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(payload): Json<ReviewModerationJobRequest>,
) -> Result<Json<ModerationJob>, ApiError> {
    authenticate_admin(&state, &headers)?;
    let status = normalize_moderation_status(&payload.status)?;
    let labels = payload.labels.unwrap_or_default();
    let row = sqlx::query(
        r#"
        UPDATE moderation_jobs
        SET status = $1,
            score = $2,
            labels = $3,
            error = $4,
            updated_at = now()
        WHERE id = $5
        RETURNING id, subject_type, subject_id, image_url, status, score, labels,
                  provider, error, created_at, updated_at
        "#,
    )
    .bind(status)
    .bind(payload.score)
    .bind(labels)
    .bind(payload.error)
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;

    Ok(Json(row_to_moderation_job(row)))
}

pub async fn list_post_reports(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<PostReport>>, ApiError> {
    authenticate_admin(&state, &headers)?;
    let rows = sqlx::query(
        r#"
        SELECT id, post_id, reporter_user_id, reason, details, severity, status, created_at
        FROM post_reports
        WHERE status = 'queued_review'
        ORDER BY
          CASE WHEN severity = 'high' THEN 0 ELSE 1 END,
          created_at ASC
        LIMIT 200
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows.into_iter().map(row_to_post_report).collect()))
}

pub async fn list_rescue_final_reports(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<RescueFinalReportAdmin>>, ApiError> {
    authenticate_admin(&state, &headers)?;
    let status = query
        .get("status")
        .map(String::as_str)
        .unwrap_or("pending_approval");
    if !matches!(
        status,
        "draft" | "pending_approval" | "published" | "rejected" | "all"
    ) {
        return Err(ApiError::Validation("invalid report status".into()));
    }

    let rows = sqlx::query(
        r#"
        SELECT
          rfr.*,
          COALESCE(p.name, 'Publicacao') AS post_title,
          p.post_type::text AS post_type
        FROM rescue_final_reports rfr
        JOIN posts p ON p.id = rfr.post_id
        WHERE ($1 = 'all' OR rfr.publication_status = $1)
        ORDER BY
          CASE WHEN rfr.publication_status = 'pending_approval' THEN 0 ELSE 1 END,
          rfr.created_at ASC
        LIMIT 200
        "#,
    )
    .bind(status)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(row_to_rescue_final_report_admin)
            .collect(),
    ))
}

pub async fn queue_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<QueueInfo>>, ApiError> {
    authenticate_admin(&state, &headers)?;

    let waiting = count_push_jobs(&state, "queued").await?;
    let failed =
        count_push_jobs(&state, "failed").await? + count_push_jobs(&state, "dead_letter").await?;
    let completed = count_push_jobs(&state, "sent").await?;
    let delayed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM push_delivery_jobs WHERE status IN ('queued', 'failed') AND next_attempt_at > now()",
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(vec![QueueInfo {
        name: "push-delivery".into(),
        active: 0,
        waiting,
        completed,
        failed,
        delayed,
        paused: !state.config.push_worker_enabled,
    }]))
}

pub async fn queue_jobs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(queue_name): Path<String>,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Vec<QueueJob>>, ApiError> {
    authenticate_admin(&state, &headers)?;
    ensure_push_queue(&queue_name)?;

    let status_filter = query.get("status").and_then(|value| match value.as_str() {
        "waiting" => Some(vec!["queued"]),
        "failed" => Some(vec!["failed", "dead_letter"]),
        "completed" => Some(vec!["sent"]),
        "delayed" => Some(vec!["queued", "failed"]),
        "active" => Some(Vec::new()),
        _ => None,
    });

    let mut rows = if let Some(statuses) = status_filter {
        if statuses.is_empty() {
            Vec::new()
        } else if query.get("status").map(String::as_str) == Some("delayed") {
            sqlx::query(
                r#"
                SELECT id, payload, status, attempts, last_error, created_at, updated_at
                FROM push_delivery_jobs
                WHERE status = ANY($1) AND next_attempt_at > now()
                ORDER BY next_attempt_at ASC
                LIMIT 200
                "#,
            )
            .bind(statuses)
            .fetch_all(&state.db)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT id, payload, status, attempts, last_error, created_at, updated_at
                FROM push_delivery_jobs
                WHERE status = ANY($1)
                ORDER BY created_at DESC
                LIMIT 200
                "#,
            )
            .bind(statuses)
            .fetch_all(&state.db)
            .await?
        }
    } else {
        sqlx::query(
            r#"
            SELECT id, payload, status, attempts, last_error, created_at, updated_at
            FROM push_delivery_jobs
            ORDER BY created_at DESC
            LIMIT 200
            "#,
        )
        .fetch_all(&state.db)
        .await?
    };

    rows.sort_by_key(|row| row.get::<DateTime<Utc>, _>("created_at"));
    rows.reverse();
    Ok(Json(rows.into_iter().map(row_to_queue_job).collect()))
}

pub async fn retry_queue_job(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((queue_name, job_id)): Path<(String, Uuid)>,
) -> Result<Json<QueueJob>, ApiError> {
    let admin = authenticate_admin(&state, &headers)?;
    ensure_push_queue(&queue_name)?;

    let row = sqlx::query(
        r#"
        UPDATE push_delivery_jobs
        SET status = 'queued',
            next_attempt_at = now(),
            last_error = NULL,
            updated_at = now()
        WHERE id = $1
          AND status IN ('failed', 'dead_letter')
        RETURNING id, payload, status, attempts, last_error, created_at, updated_at
        "#,
    )
    .bind(job_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;

    audit_event(
        &state,
        Uuid::parse_str(&admin.sub).ok(),
        "admin.queue_job.retry",
        serde_json::json!({ "queue": queue_name, "jobId": job_id }),
    )
    .await;

    Ok(Json(row_to_queue_job(row)))
}

async fn count_push_jobs(state: &AppState, status: &str) -> Result<i64, ApiError> {
    Ok(
        sqlx::query_scalar("SELECT count(*) FROM push_delivery_jobs WHERE status = $1")
            .bind(status)
            .fetch_one(&state.db)
            .await?,
    )
}

fn ensure_push_queue(queue_name: &str) -> Result<(), ApiError> {
    if queue_name == "push-delivery" || queue_name == "push_delivery_jobs" {
        Ok(())
    } else {
        Err(ApiError::NotFound)
    }
}

fn authenticate_admin(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<auth_service::AccessClaims, ApiError> {
    let header = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::Unauthorized)?;
    let token = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
        .ok_or(ApiError::Unauthorized)?;
    let claims = auth_service::verify_access_token(&state.config, token)
        .map_err(|_| ApiError::Unauthorized)?;

    if !matches!(claims.account_type, AccountType::Admin) {
        return Err(ApiError::Forbidden);
    }

    Ok(claims)
}

fn normalize_status(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_uppercase().as_str() {
        "PENDING" | "PENDING_REVIEW" | "PENDING_MANUAL_REVIEW" => Some("PENDING_MANUAL_REVIEW"),
        "APPROVED" => Some("APPROVED"),
        "REJECTED" => Some("REJECTED"),
        "BLOCKED" => Some("BLOCKED"),
        _ => None,
    }
}

fn normalize_review_status(value: &str) -> Result<&'static str, ApiError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "pending" | "pending_review" => Ok("pending_review"),
        "approved" => Ok("approved"),
        "rejected" => Ok("rejected"),
        _ => Err(ApiError::Validation("invalid review status".into())),
    }
}

fn normalize_document_type(value: &str) -> Result<&'static str, ApiError> {
    match value {
        "document_front" | "rg_front" | "id_front" => Ok("document_front"),
        "document_back" | "rg_back" | "id_back" => Ok("document_back"),
        "selfie_with_document" | "selfie_rg" | "selfie" => Ok("selfie_with_document"),
        "address_proof" | "proof_of_address" => Ok("address_proof"),
        _ => Err(ApiError::Validation("invalid documentType".into())),
    }
}

async fn maybe_auto_approve_ong_from_kyb(
    state: &AppState,
    ong_id: Uuid,
    reviewer_id: Uuid,
) -> Result<(), ApiError> {
    let approved_required_docs: i64 = sqlx::query_scalar(
        r#"
        SELECT count(DISTINCT document_type)
        FROM ong_kyb_documents
        WHERE ong_id = $1
          AND status = 'approved'
          AND document_type IN ('document_front', 'document_back', 'selfie_with_document')
        "#,
    )
    .bind(ong_id)
    .fetch_one(&state.db)
    .await?;

    if approved_required_docs < 3 {
        return Ok(());
    }

    let mut tx = state.db.begin().await?;
    let user_id: Option<Uuid> = sqlx::query_scalar(
        r#"
        UPDATE ong_profiles
        SET verification_status = 'APPROVED',
            verification_reviewed_at = COALESCE(verification_reviewed_at, now()),
            verification_reviewer_user_id = $2,
            verification_rejection_reason = NULL,
            verified_at = COALESCE(verified_at, now()),
            updated_at = now()
        WHERE id = $1
        RETURNING user_id
        "#,
    )
    .bind(ong_id)
    .bind(reviewer_id)
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(user_id) = user_id {
        sqlx::query("UPDATE users SET verified = true WHERE id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(())
}

fn authenticate_any(
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

    auth_service::verify_access_token(&state.config, token).map_err(|_| ApiError::Unauthorized)
}

fn normalize_moderation_status(value: &str) -> Result<&'static str, ApiError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "queued" => Ok("queued"),
        "approved" => Ok("approved"),
        "rejected" => Ok("rejected"),
        "needs_review" => Ok("needs_review"),
        "failed" => Ok("failed"),
        _ => Err(ApiError::Validation("invalid moderation status".into())),
    }
}

fn row_to_admin_ong(row: sqlx::postgres::PgRow) -> AdminOngProfile {
    let verification_status: String = row.get("verification_status");
    let name: String = row.get("name");
    let legal_name: String = row.get("legal_name");

    AdminOngProfile {
        id: row.get::<Uuid, _>("id").to_string(),
        user_id: row.get::<Uuid, _>("user_id").to_string(),
        full_name: legal_name.clone(),
        legal_name,
        name,
        email: row.get("email"),
        avatar: row.get("avatar_url"),
        phone: row.get("contact_phone"),
        cep: row.get("cep"),
        street: row.get("street"),
        number: row.get("number"),
        complement: row.get("complement"),
        neighborhood: row.get("neighborhood"),
        city: row.get("city"),
        state: row.get("state"),
        foundation_year: row.get("foundation_year"),
        cnpj: row.get("cnpj"),
        ong_type: row.get("area_type"),
        verified: verification_status == "APPROVED",
        verification_status,
        rejection_reason: row.get("verification_rejection_reason"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        five_star_review_count: 0,
        monthly_bookings_count: 0,
        total_earnings: "0.00".into(),
    }
}

fn row_to_admin_user(row: sqlx::postgres::PgRow) -> AdminUserProfile {
    let account_type: String = row.get("account_type");
    let role = match account_type.as_str() {
        "admin" => "ADMIN",
        "ong" | "vet" => "PROVIDER",
        _ => "CLIENT",
    };
    let verified: bool = row.get("verified");
    let created_at: DateTime<Utc> = row.get("created_at");

    AdminUserProfile {
        id: row.get::<Uuid, _>("id").to_string(),
        name: row.get("name"),
        full_name: row.get("name"),
        email: row.get("email"),
        avatar_url: row.get("avatar_url"),
        role: role.into(),
        status: "active".into(),
        verified,
        verification_status: row.get("verification_status"),
        phone: row.get("contact_phone"),
        city: row.get("city"),
        state: row.get("state"),
        completed_bookings_count: 0,
        total_spent: "0.00".into(),
        created_at,
        updated_at: created_at,
    }
}

fn row_to_kyb_document(row: sqlx::postgres::PgRow) -> KybDocument {
    KybDocument {
        id: row.get::<Uuid, _>("id").to_string(),
        ong_id: row.get::<Uuid, _>("ong_id").to_string(),
        document_type: row.get("document_type"),
        object_key: row.get("object_key"),
        public_url: row.get("public_url"),
        status: row.get("status"),
        reviewer_user_id: row
            .get::<Option<Uuid>, _>("reviewer_user_id")
            .map(|value| value.to_string()),
        rejection_reason: row.get("rejection_reason"),
        created_at: row.get("created_at"),
        reviewed_at: row.get("reviewed_at"),
    }
}

fn row_to_moderation_job(row: sqlx::postgres::PgRow) -> ModerationJob {
    ModerationJob {
        id: row.get::<Uuid, _>("id").to_string(),
        subject_type: row.get("subject_type"),
        subject_id: row.get("subject_id"),
        image_url: row.get("image_url"),
        status: row.get("status"),
        score: row.get("score"),
        labels: row.get("labels"),
        provider: row.get("provider"),
        error: row.get("error"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn row_to_post_report(row: sqlx::postgres::PgRow) -> PostReport {
    PostReport {
        id: row.get::<Uuid, _>("id").to_string(),
        post_id: row.get::<Uuid, _>("post_id").to_string(),
        reporter_user_id: row.get::<Uuid, _>("reporter_user_id").to_string(),
        reason: row.get("reason"),
        details: row.get("details"),
        severity: row.get("severity"),
        status: row.get("status"),
        created_at: row.get("created_at"),
    }
}

fn row_to_rescue_final_report_admin(row: sqlx::postgres::PgRow) -> RescueFinalReportAdmin {
    RescueFinalReportAdmin {
        id: row.get::<Uuid, _>("id").to_string(),
        rescue_id: row
            .get::<Option<Uuid>, _>("rescue_id")
            .map(|value| value.to_string()),
        post_id: row.get::<Uuid, _>("post_id").to_string(),
        post_title: row.get("post_title"),
        post_type: row.get("post_type"),
        status: row.get("status"),
        summary: row.get("summary"),
        public_update: row.get("public_update"),
        generated_by_ai: row.get("generated_by_ai"),
        publication_status: row.get("publication_status"),
        rejection_reason: row.get("rejection_reason"),
        approved_by: row
            .get::<Option<Uuid>, _>("approved_by")
            .map(|value| value.to_string()),
        approved_at: row.get("approved_at"),
        rejected_by: row
            .get::<Option<Uuid>, _>("rejected_by")
            .map(|value| value.to_string()),
        rejected_at: row.get("rejected_at"),
        created_by: row
            .get::<Option<Uuid>, _>("created_by")
            .map(|value| value.to_string()),
        updated_by: row
            .get::<Option<Uuid>, _>("updated_by")
            .map(|value| value.to_string()),
        admin_notes: row.get("admin_notes"),
        ai_model: row.get("ai_model"),
        ai_latency_ms: row.get("ai_latency_ms"),
        ai_cost_cents: row.get("ai_cost_cents"),
        prompt_version: row.get("prompt_version"),
        schema_version: row.get("schema_version"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

fn row_to_queue_job(row: sqlx::postgres::PgRow) -> QueueJob {
    let status: String = row.get("status");
    let finished_at = if status == "sent" {
        Some(row.get("updated_at"))
    } else {
        None
    };
    QueueJob {
        id: row.get::<Uuid, _>("id").to_string(),
        name: "push-delivery".into(),
        data: row.get("payload"),
        status: match status.as_str() {
            "queued" => "waiting",
            "sent" => "completed",
            "failed" | "dead_letter" => "failed",
            _ => status.as_str(),
        }
        .into(),
        progress: if status == "sent" { 100 } else { 0 },
        attempts_made: row.get("attempts"),
        failed_reason: row.get("last_error"),
        created_at: row.get("created_at"),
        finished_at,
    }
}
