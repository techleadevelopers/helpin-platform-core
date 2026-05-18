use axum::{extract::State, Json};
use serde::Serialize;

use crate::state::AppState;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub service: &'static str,
}

#[derive(Serialize)]
pub struct ReadinessResponse {
    pub status: &'static str,
    pub database_configured: bool,
    pub redis_configured: bool,
    pub nats_configured: bool,
    pub ai_worker_configured: bool,
}

pub async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "zoohelp-rust-core",
    })
}

pub async fn readyz(State(state): State<AppState>) -> Json<ReadinessResponse> {
    Json(ReadinessResponse {
        status: "ready",
        database_configured: !state.db.is_closed(),
        redis_configured: !state.config.redis_url.is_empty(),
        nats_configured: !state.config.nats_url.is_empty(),
        ai_worker_configured: !state.config.ai_worker_url.is_empty(),
    })
}
