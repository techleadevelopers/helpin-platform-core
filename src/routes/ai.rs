use axum::{extract::State, http::HeaderMap, Json};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{error::ApiError, routes::auth::authenticate_request, state::AppState};

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
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<ModerationJobRequest>,
) -> Result<Json<ModerationJobResponse>, ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;
    authenticate_request(&state, &headers)?;
    let job_id = uuid::Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO moderation_jobs (id, subject_type, subject_id, image_url, status, provider)
        VALUES ($1, 'post', $2, $3, 'queued', $4)
        "#,
    )
    .bind(job_id)
    .bind(&payload.post_id)
    .bind(&payload.image_url)
    .bind(&state.config.ai_worker_url)
    .execute(&state.db)
    .await?;

    Ok(Json(ModerationJobResponse {
        job_id: job_id.to_string(),
        worker_url: state.config.ai_worker_url,
        status: "queued",
    }))
}
