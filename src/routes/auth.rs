use axum::Json;
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{domain::AccountType, error::ApiError};

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
    pub access_token: String,
    pub token_type: &'static str,
}

pub async fn login(Json(payload): Json<LoginRequest>) -> Result<Json<AuthResponse>, ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let _password_len = payload.password.len();
    let name = payload
        .email
        .split('@')
        .next()
        .unwrap_or("Você")
        .to_string();

    Ok(Json(auth_response(
        "me",
        &name,
        &payload.email,
        AccountType::Person,
    )))
}

pub async fn register(
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let _password_len = payload.password.len();

    Ok(Json(auth_response(
        "me",
        &payload.name,
        &payload.email,
        payload.account_type.unwrap_or(AccountType::Person),
    )))
}

fn auth_response(id: &str, name: &str, email: &str, account_type: AccountType) -> AuthResponse {
    AuthResponse {
        user: UserProfile {
            id: id.into(),
            name: name.into(),
            email: email.into(),
            avatar: None,
            bio: "Apaixonada por animais".into(),
            account_type,
            verified: false,
            posts_count: 3,
            helped_count: 12,
            adoptions_count: 2,
        },
        access_token: "replace-with-jwt-issued-by-rust-core".into(),
        token_type: "Bearer",
    }
}
