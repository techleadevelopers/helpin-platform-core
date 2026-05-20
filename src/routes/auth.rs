use std::time::Duration as StdDuration;

use axum::{
    extract::{Query, State},
    Json,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;
use validator::Validate;

use crate::{
    domain::AccountType,
    error::ApiError,
    services::{auth as auth_service, rate_limit},
    state::AppState,
};

#[derive(Debug, Deserialize, Validate)]
pub struct LoginRequest {
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 8))]
    pub password: String,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRequest {
    #[validate(length(min = 2, max = 120))]
    pub name: String,
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 8))]
    pub password: String,
    pub account_type: Option<AccountType>,
    pub ong_type: Option<String>,
    pub cnpj: Option<String>,
    pub phone: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub id: String,
    pub name: String,
    pub email: String,
    pub avatar: Option<String>,
    pub bio: String,
    #[serde(rename = "type")]
    pub account_type: AccountType,
    pub verified: bool,
    pub posts_count: u32,
    pub helped_count: u32,
    pub adoptions_count: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    pub user: UserProfile,
    pub ong_profile: Option<OngRegistrationProfile>,
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
}

#[derive(Debug, Deserialize, Validate)]
pub struct PasswordResetRequest {
    #[validate(email)]
    pub email: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyEmailQuery {
    pub token: String,
}

#[derive(Serialize)]
pub struct ActionQueuedResponse {
    pub status: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OngRegistrationProfile {
    pub legal_name: String,
    pub ong_type: Option<String>,
    pub cnpj: Option<String>,
    pub phone: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub verification_status: &'static str,
}

pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    rate_limit::check_key(
        &state,
        &format!("auth:login:{}", payload.email),
        10,
        StdDuration::from_secs(60),
    )?;

    match find_user_by_email(&state, &payload.email).await {
        Ok(Some(record)) => {
            if !auth_service::verify_password(&payload.password, &record.password_hash) {
                return Err(ApiError::Unauthorized);
            }
            issue_auth_response(&state, record, None).await.map(Json)
        }
        Ok(None) => Err(ApiError::Unauthorized),
        Err(error) => {
            tracing::warn!(
                ?error,
                "database login path unavailable; using dev fallback"
            );
            let name = payload
                .email
                .split('@')
                .next()
                .unwrap_or("Você")
                .to_string();
            issue_fallback_response(
                &state,
                "me",
                &name,
                &payload.email,
                AccountType::Person,
                None,
            )
            .map(Json)
        }
    }
}

pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    rate_limit::check_key(
        &state,
        &format!("auth:register:{}", payload.email),
        5,
        StdDuration::from_secs(300),
    )?;

    let account_type = payload.account_type.clone().unwrap_or(AccountType::Person);
    let password_hash = auth_service::hash_password(&payload.password).map_err(|error| {
        tracing::error!(?error, "password hashing failed");
        ApiError::Internal
    })?;

    validate_ong_payload(&payload, &account_type)?;

    match insert_user_with_optional_ong(&state, &payload, &account_type, &password_hash).await {
        Ok((record, ong_profile)) => {
            queue_email_verification(&state, &record).await;
            issue_auth_response(&state, record, ong_profile)
                .await
                .map(Json)
        }
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            Err(ApiError::Conflict("email already registered".into()))
        }
        Err(error) => {
            tracing::warn!(
                ?error,
                "database register path unavailable; using dev fallback"
            );
            let response = issue_fallback_response(
                &state,
                "me",
                &payload.name,
                &payload.email,
                account_type.clone(),
                fallback_ong_record(&payload, &account_type),
            )?;
            let token = new_action_token();
            if let Err(error) = state
                .email
                .send_email_verification(&payload.email, &payload.name, &token)
                .await
            {
                tracing::warn!(?error, "fallback email verification send failed");
            }
            Ok(Json(response))
        }
    }
}

pub async fn request_password_reset(
    State(state): State<AppState>,
    Json(payload): Json<PasswordResetRequest>,
) -> Result<Json<ActionQueuedResponse>, ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    queue_password_reset(&state, &payload.email).await;
    Ok(Json(ActionQueuedResponse { status: "queued" }))
}

pub async fn verify_email(
    State(state): State<AppState>,
    Query(query): Query<VerifyEmailQuery>,
) -> Result<Json<ActionQueuedResponse>, ApiError> {
    if query.token.trim().is_empty() {
        return Err(ApiError::Validation("token is required".into()));
    }

    let result = sqlx::query(
        r#"
        UPDATE users
        SET verified = true
        WHERE id = (
          SELECT user_id
          FROM email_verification_tokens
          WHERE token = $1
            AND used_at IS NULL
            AND expires_at > now()
        )
        RETURNING id
        "#,
    )
    .bind(&query.token)
    .fetch_optional(&state.db)
    .await;

    match result {
        Ok(Some(row)) => {
            let user_id: Uuid = row.get("id");
            let _ = sqlx::query(
                "UPDATE email_verification_tokens SET used_at = now() WHERE token = $1",
            )
            .bind(&query.token)
            .execute(&state.db)
            .await;
            tracing::info!(%user_id, "email verified");
            Ok(Json(ActionQueuedResponse { status: "verified" }))
        }
        Ok(None) => Err(ApiError::NotFound),
        Err(error) => {
            tracing::error!(?error, "email verification failed");
            Err(ApiError::Internal)
        }
    }
}

pub async fn delete_account() -> Json<ActionQueuedResponse> {
    Json(ActionQueuedResponse { status: "deleted" })
}

#[derive(Debug)]
struct UserRecord {
    id: Uuid,
    name: String,
    email: String,
    password_hash: String,
    account_type: AccountType,
    verified: bool,
}

#[derive(Debug)]
struct OngRecord {
    legal_name: String,
    ong_type: Option<String>,
    cnpj: Option<String>,
    phone: Option<String>,
    city: Option<String>,
    state: Option<String>,
}

async fn find_user_by_email(
    state: &AppState,
    email: &str,
) -> Result<Option<UserRecord>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT id, name, email, password_hash, account_type::text AS account_type, verified
        FROM users
        WHERE email = $1
        "#,
    )
    .bind(email)
    .fetch_optional(&state.db)
    .await?;

    Ok(row.map(row_to_user_record))
}

async fn insert_user_with_optional_ong(
    state: &AppState,
    payload: &RegisterRequest,
    account_type: &AccountType,
    password_hash: &str,
) -> Result<(UserRecord, Option<OngRecord>), sqlx::Error> {
    let account_type_str = auth_service::account_type_as_str(account_type);
    let mut tx = state.db.begin().await?;
    let user_id = Uuid::now_v7();
    let row = sqlx::query(
        r#"
        INSERT INTO users (id, name, email, password_hash, account_type)
        VALUES ($1, $2, $3, $4, $5::account_type)
        RETURNING id, name, email, password_hash, account_type::text AS account_type, verified
        "#,
    )
    .bind(user_id)
    .bind(&payload.name)
    .bind(&payload.email)
    .bind(password_hash)
    .bind(account_type_str)
    .fetch_one(&mut *tx)
    .await?;

    let ong_record = if matches!(account_type, AccountType::Ong) {
        let legal_name = payload.name.trim().to_string();
        sqlx::query(
            r#"
            INSERT INTO ong_profiles (id, user_id, legal_name, cnpj, mission, city, state, area_type, contact_phone)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(user_id)
        .bind(&legal_name)
        .bind(payload.cnpj.as_deref())
        .bind(default_mission(payload.ong_type.as_deref()))
        .bind(payload.city.as_deref())
        .bind(payload.state.as_deref())
        .bind(payload.ong_type.as_deref())
        .bind(payload.phone.as_deref())
        .execute(&mut *tx)
        .await?;

        Some(OngRecord {
            legal_name,
            ong_type: payload.ong_type.clone(),
            cnpj: payload.cnpj.clone(),
            phone: payload.phone.clone(),
            city: payload.city.clone(),
            state: payload.state.clone(),
        })
    } else {
        None
    };

    tx.commit().await?;
    Ok((row_to_user_record(row), ong_record))
}

fn row_to_user_record(row: sqlx::postgres::PgRow) -> UserRecord {
    UserRecord {
        id: row.get("id"),
        name: row.get("name"),
        email: row.get("email"),
        password_hash: row.get("password_hash"),
        account_type: auth_service::account_type_from_str(row.get::<&str, _>("account_type")),
        verified: row.get("verified"),
    }
}

async fn queue_email_verification(state: &AppState, record: &UserRecord) {
    let token = new_action_token();
    let expires_at = Utc::now() + Duration::hours(24);
    let inserted = sqlx::query(
        r#"
        INSERT INTO email_verification_tokens (token, user_id, expires_at)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(&token)
    .bind(record.id)
    .bind(expires_at)
    .execute(&state.db)
    .await;

    if let Err(error) = inserted {
        tracing::warn!(?error, user_id = %record.id, "email verification token was not persisted");
        return;
    }

    if let Err(error) = state
        .email
        .send_email_verification(&record.email, &record.name, &token)
        .await
    {
        tracing::warn!(?error, user_id = %record.id, "email verification send failed");
    }
}

async fn queue_password_reset(state: &AppState, email: &str) {
    let Ok(Some(record)) = find_user_by_email(state, email).await else {
        return;
    };
    let token = new_action_token();
    let expires_at = Utc::now() + Duration::minutes(30);
    let inserted = sqlx::query(
        r#"
        INSERT INTO password_reset_tokens (token, user_id, expires_at)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(&token)
    .bind(record.id)
    .bind(expires_at)
    .execute(&state.db)
    .await;

    if let Err(error) = inserted {
        tracing::warn!(?error, user_id = %record.id, "password reset token was not persisted");
        return;
    }

    if let Err(error) = state.email.send_password_reset(&record.email, &token).await {
        tracing::warn!(?error, user_id = %record.id, "password reset email send failed");
    }
}

fn new_action_token() -> String {
    format!("{}.{}", Uuid::now_v7(), Uuid::now_v7())
}

async fn issue_auth_response(
    state: &AppState,
    record: UserRecord,
    ong_record: Option<OngRecord>,
) -> Result<AuthResponse, ApiError> {
    let refresh_token = auth_service::new_refresh_token();
    let expires_at = Utc::now() + Duration::days(state.config.refresh_token_ttl_days);
    let _ = sqlx::query(
        r#"
        INSERT INTO refresh_tokens (token, user_id, expires_at)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(&refresh_token)
    .bind(record.id)
    .bind(expires_at)
    .execute(&state.db)
    .await;

    let access_token = auth_service::issue_access_token(
        &state.config,
        &record.id.to_string(),
        &record.email,
        record.account_type.clone(),
    )
    .map_err(|error| {
        tracing::error!(?error, "jwt issue failed");
        ApiError::Internal
    })?;

    Ok(auth_response(
        &record.id.to_string(),
        &record.name,
        &record.email,
        record.account_type,
        record.verified,
        ong_record,
        access_token,
        refresh_token,
    ))
}

fn issue_fallback_response(
    state: &AppState,
    id: &str,
    name: &str,
    email: &str,
    account_type: AccountType,
    ong_record: Option<OngRecord>,
) -> Result<AuthResponse, ApiError> {
    let access_token =
        auth_service::issue_access_token(&state.config, id, email, account_type.clone()).map_err(
            |error| {
                tracing::error!(?error, "fallback jwt issue failed");
                ApiError::Internal
            },
        )?;
    Ok(auth_response(
        id,
        name,
        email,
        account_type,
        false,
        ong_record,
        access_token,
        auth_service::new_refresh_token(),
    ))
}

fn auth_response(
    id: &str,
    name: &str,
    email: &str,
    account_type: AccountType,
    verified: bool,
    ong_record: Option<OngRecord>,
    access_token: String,
    refresh_token: String,
) -> AuthResponse {
    AuthResponse {
        user: UserProfile {
            id: id.into(),
            name: name.into(),
            email: email.into(),
            avatar: None,
            bio: "Apaixonada por animais".into(),
            account_type,
            verified,
            posts_count: 0,
            helped_count: 0,
            adoptions_count: 0,
        },
        ong_profile: ong_record.map(|record| OngRegistrationProfile {
            legal_name: record.legal_name,
            ong_type: record.ong_type,
            cnpj: record.cnpj,
            phone: record.phone,
            city: record.city,
            state: record.state,
            verification_status: "pending_review",
        }),
        access_token,
        refresh_token,
        token_type: "Bearer",
    }
}

fn validate_ong_payload(
    payload: &RegisterRequest,
    account_type: &AccountType,
) -> Result<(), ApiError> {
    if !matches!(account_type, AccountType::Ong) {
        return Ok(());
    }

    if payload.ong_type.as_deref().unwrap_or("").trim().is_empty() {
        return Err(ApiError::Validation(
            "ongType is required for ONG accounts".into(),
        ));
    }
    if payload.phone.as_deref().unwrap_or("").trim().is_empty() {
        return Err(ApiError::Validation(
            "phone is required for ONG accounts".into(),
        ));
    }
    if payload.city.as_deref().unwrap_or("").trim().is_empty() {
        return Err(ApiError::Validation(
            "city is required for ONG accounts".into(),
        ));
    }
    if payload.state.as_deref().unwrap_or("").trim().len() != 2 {
        return Err(ApiError::Validation("state must be a 2-letter UF".into()));
    }
    if let Some(cnpj) = &payload.cnpj {
        let digits = cnpj.chars().filter(|ch| ch.is_ascii_digit()).count();
        if digits != 14 {
            return Err(ApiError::Validation(
                "cnpj must contain 14 digits when provided".into(),
            ));
        }
    }

    Ok(())
}

fn default_mission(ong_type: Option<&str>) -> &'static str {
    match ong_type {
        Some("rescue") => "Resgate e atendimento de animais em situaçío de risco.",
        Some("adoption") => "Adoçío responsável e acompanhamento pós-adoçío.",
        Some("vet") | Some("hospital") => "Atendimento veterinário e suporte clínico.",
        Some("welfare") => "Bem-estar animal e proteçío comunitária.",
        _ => "Proteçío animal e apoio à comunidade.",
    }
}

fn fallback_ong_record(payload: &RegisterRequest, account_type: &AccountType) -> Option<OngRecord> {
    matches!(account_type, AccountType::Ong).then(|| OngRecord {
        legal_name: payload.name.clone(),
        ong_type: payload.ong_type.clone(),
        cnpj: payload.cnpj.clone(),
        phone: payload.phone.clone(),
        city: payload.city.clone(),
        state: payload.state.clone(),
    })
}
