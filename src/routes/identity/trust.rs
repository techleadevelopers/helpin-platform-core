use axum::{
    extract::{Path, State},
    Json,
};
use serde::Serialize;
use uuid::Uuid;

use crate::{error::ApiError, state::AppState};

#[derive(Serialize)]
pub struct TrustScoreResponse {
    pub subject_id: String,
    pub score: u8,
    pub tier: &'static str,
}

pub async fn score(
    State(state): State<AppState>,
    Path(subject_id): Path<String>,
) -> Result<Json<TrustScoreResponse>, ApiError> {
    let user_id = Uuid::parse_str(&subject_id).map_err(|_| ApiError::NotFound)?;
    let score: Option<i16> = sqlx::query_scalar("SELECT trust_score FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?;
    let score = score.ok_or(ApiError::NotFound)?.clamp(0, 100) as u8;

    Ok(Json(TrustScoreResponse {
        subject_id,
        score,
        tier: trust_tier(score),
    }))
}

fn trust_tier(score: u8) -> &'static str {
    match score {
        80..=100 => "verified",
        50..=79 => "established",
        25..=49 => "limited",
        _ => "new_or_risk",
    }
}
