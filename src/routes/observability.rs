use axum::{
    extract::State,
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Serialize;

use crate::state::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservabilityStatus {
    pub service: &'static str,
    pub tracing: &'static str,
    pub metrics: &'static str,
    pub sentry: &'static str,
    pub redis_rate_limit: &'static str,
    pub event_bus: &'static str,
    pub push_worker: &'static str,
    pub payment_provider: String,
    pub queued_push_jobs: i64,
    pub dead_letter_push_jobs: i64,
    pub queued_moderation_jobs: i64,
    pub timestamp: String,
}

pub async fn status(State(state): State<AppState>) -> Json<ObservabilityStatus> {
    let queued_push_jobs = count_scalar(
        &state,
        "SELECT count(*) FROM push_delivery_jobs WHERE status = 'queued'",
    )
    .await;
    let dead_letter_push_jobs = count_scalar(
        &state,
        "SELECT count(*) FROM push_delivery_jobs WHERE status = 'dead_letter'",
    )
    .await;
    let queued_moderation_jobs = count_scalar(
        &state,
        "SELECT count(*) FROM moderation_jobs WHERE status IN ('queued', 'needs_review')",
    )
    .await;

    Json(ObservabilityStatus {
        service: "zoohelp-backend",
        tracing: "enabled",
        metrics: "enabled",
        sentry: if state.config.sentry_dsn.is_some() {
            "configured"
        } else {
            "missing"
        },
        redis_rate_limit: if state.redis.is_some() {
            "configured"
        } else {
            "local_dev_fallback"
        },
        event_bus: if state.event_bus.enabled() {
            "nats"
        } else {
            "local_dev_fallback"
        },
        push_worker: if state.config.push_worker_enabled {
            "enabled"
        } else {
            "disabled"
        },
        payment_provider: state.config.payment_provider.clone(),
        queued_push_jobs,
        dead_letter_push_jobs,
        queued_moderation_jobs,
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

pub async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    let queued_push_jobs = count_scalar(
        &state,
        "SELECT count(*) FROM push_delivery_jobs WHERE status = 'queued'",
    )
    .await;
    let dead_letter_push_jobs = count_scalar(
        &state,
        "SELECT count(*) FROM push_delivery_jobs WHERE status = 'dead_letter'",
    )
    .await;
    let queued_moderation_jobs = count_scalar(
        &state,
        "SELECT count(*) FROM moderation_jobs WHERE status IN ('queued', 'needs_review')",
    )
    .await;
    let body = format!(
        "# HELP zoohelp_backend_up Backend process availability\n# TYPE zoohelp_backend_up gauge\nzoohelp_backend_up 1\n# HELP zoohelp_backend_timestamp_seconds Current unix timestamp\n# TYPE zoohelp_backend_timestamp_seconds gauge\nzoohelp_backend_timestamp_seconds {}\n# HELP zoohelp_push_jobs_queued Queued push delivery jobs\n# TYPE zoohelp_push_jobs_queued gauge\nzoohelp_push_jobs_queued {}\n# HELP zoohelp_push_jobs_dead_letter Dead-lettered push delivery jobs\n# TYPE zoohelp_push_jobs_dead_letter gauge\nzoohelp_push_jobs_dead_letter {}\n# HELP zoohelp_moderation_jobs_queued Queued moderation jobs\n# TYPE zoohelp_moderation_jobs_queued gauge\nzoohelp_moderation_jobs_queued {}\n",
        chrono::Utc::now().timestamp(),
        queued_push_jobs,
        dead_letter_push_jobs,
        queued_moderation_jobs
    );
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
}

async fn count_scalar(state: &AppState, sql: &str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(&state.db)
        .await
        .unwrap_or(0)
}
