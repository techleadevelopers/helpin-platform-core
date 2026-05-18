use axum::Json;
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::error::ApiError;

#[derive(Debug, Deserialize, Validate)]
pub struct LoginRequest {
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 8))]
    pub password: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct RegisterRequest {
    #[validate(length(min = 2, max = 120))]
    pub name: String,
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 8))]
    pub password: String,
    pub account_type: Option<String>,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub user_id: String,
    pub access_token: String,
    pub token_type: &'static str,
}

pub async fn login(Json(payload): Json<LoginRequest>) -> Result<Json<AuthResponse>, ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    Ok(Json(AuthResponse {
        user_id: "dev-user".into(),
        access_token: "dev-token-replace-with-jwt".into(),
        token_type: "Bearer",
    }))
}

pub async fn register(
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<AuthResponse>, ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let _account_type = payload.account_type.unwrap_or_else(|| "person".into());
    Ok(Json(AuthResponse {
        user_id: "dev-user".into(),
        access_token: "dev-token-replace-with-jwt".into(),
        token_type: "Bearer",
    }))
}
