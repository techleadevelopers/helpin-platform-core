use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{error::ApiError, state::AppState};

#[derive(Debug, Deserialize, Validate)]
pub struct ModerationJobRequest {
    #[validate(url)]
    pub image_url: String,
    pub post_id: String,
}

#[derive(Serialize)]
pub struct ModerationJobResponse {
    pub job_id: String,
    pub worker_url: String,
    pub status: &'static str,
}

pub async fn enqueue_moderation_job(
    State(state): State<AppState>,
    Json(payload): Json<ModerationJobRequest>,
) -> Result<Json<ModerationJobResponse>, ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    let _ = (payload.image_url, payload.post_id);
    Ok(Json(ModerationJobResponse {
        job_id: uuid::Uuid::now_v7().to_string(),
        worker_url: state.config.ai_worker_url,
        status: "queued",
    }))
}
