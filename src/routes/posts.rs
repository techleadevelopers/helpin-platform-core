use axum::{extract::Path, Json};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{error::ApiError, services::fraud};

#[derive(Debug, Deserialize, Validate)]
pub struct CreatePostRequest {
    #[validate(length(min = 1, max = 1200))]
    pub description: String,
    pub post_type: String,
    pub animal_type: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub image_urls: Vec<String>,
}

#[derive(Serialize)]
pub struct PostResponse {
    pub id: String,
    pub moderation_status: &'static str,
    pub fraud_risk: u8,
}

pub async fn create_post(
    Json(payload): Json<CreatePostRequest>,
) -> Result<Json<PostResponse>, ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let risk = fraud::score_post_text(&payload.description);
    Ok(Json(PostResponse {
        id: uuid::Uuid::now_v7().to_string(),
        moderation_status: "queued",
        fraud_risk: risk,
    }))
}

pub async fn get_post(Path(id): Path<String>) -> Json<PostResponse> {
    Json(PostResponse {
        id,
        moderation_status: "approved",
        fraud_risk: 0,
    })
}
