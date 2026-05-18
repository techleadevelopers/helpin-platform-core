use axum::Json;
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::error::ApiError;

#[derive(Debug, Deserialize, Validate)]
pub struct DonationIntentRequest {
    pub ong_id: String,
    #[validate(range(min = 100))]
    pub amount_cents: i64,
    pub currency: Option<String>,
}

#[derive(Serialize)]
pub struct DonationIntentResponse {
    pub id: String,
    pub status: &'static str,
}

pub async fn create_intent(
    Json(payload): Json<DonationIntentRequest>,
) -> Result<Json<DonationIntentResponse>, ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    Ok(Json(DonationIntentResponse {
        id: uuid::Uuid::now_v7().to_string(),
        status: "pending_provider",
    }))
}
