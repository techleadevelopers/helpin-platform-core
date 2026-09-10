use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
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

pub async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    // A configured URL is not readiness. At minimum prove that the API can
    // reach its authoritative store; failure returns 503 so orchestrators do
    // not route emergency traffic to a disconnected instance.
    let database_ready = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();
    let status = if database_ready { "ready" } else { "not_ready" };
    let response = Json(ReadinessResponse {
        status,
        database_configured: database_ready,
        redis_configured: !state.config.redis_url.is_empty(),
        nats_configured: !state.config.nats_url.is_empty(),
        ai_worker_configured: !state.config.ai_worker_url.is_empty(),
    });
    (
        if database_ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        response,
    )
}
