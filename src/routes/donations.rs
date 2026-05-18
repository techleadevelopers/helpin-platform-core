use axum::Json;
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{domain::seed_ongs, error::ApiError};

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct DonationIntentRequest {
    pub ong_id: String,
    #[validate(range(min = 100))]
    pub amount_cents: i64,
    pub currency: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DonationIntentResponse {
    pub id: String,
    pub ong_id: String,
    pub amount_cents: i64,
    pub currency: String,
    pub status: &'static str,
}

pub async fn create_intent(
    Json(payload): Json<DonationIntentRequest>,
) -> Result<Json<DonationIntentResponse>, ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    if !seed_ongs().iter().any(|ong| ong.id == payload.ong_id) {
        return Err(ApiError::NotFound);
    }

    Ok(Json(DonationIntentResponse {
        id: uuid::Uuid::now_v7().to_string(),
        ong_id: payload.ong_id,
        amount_cents: payload.amount_cents,
        currency: payload.currency.unwrap_or_else(|| "BRL".into()),
        status: "pending_provider",
    }))
}
