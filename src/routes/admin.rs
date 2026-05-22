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
    domain::AccountType, error::ApiError, services::auth as auth_service, state::AppState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationStatusRequest {
    pub status: String,
    pub rejection_reason: Option<String>,
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
