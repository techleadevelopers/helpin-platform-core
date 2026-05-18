use axum::{extract::Path, Json};
use serde::Serialize;

use crate::services::trust;

#[derive(Serialize)]
pub struct TrustScoreResponse {
    pub subject_id: String,
    pub score: u8,
    pub tier: &'static str,
}

pub async fn score(Path(subject_id): Path<String>) -> Json<TrustScoreResponse> {
    let score = trust::initial_score(true, 3);
    Json(TrustScoreResponse {
        subject_id,
        score,
        tier: "verified",
    })
}
