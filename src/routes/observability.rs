use axum::{
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservabilityStatus {
    pub service: &'static str,
    pub tracing: &'static str,
    pub metrics: &'static str,
    pub sentry: &'static str,
    pub timestamp: String,
}

pub async fn status() -> Json<ObservabilityStatus> {
    Json(ObservabilityStatus {
        service: "zoohelp-backend",
        tracing: "enabled",
        metrics: "enabled",
        sentry: "configure_sentry_dsn",
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

pub async fn metrics() -> impl IntoResponse {
    let body = format!(
        "# HELP zoohelp_backend_up Backend process availability\n# TYPE zoohelp_backend_up gauge\nzoohelp_backend_up 1\n# HELP zoohelp_backend_timestamp_seconds Current unix timestamp\n# TYPE zoohelp_backend_timestamp_seconds gauge\nzoohelp_backend_timestamp_seconds {}\n",
        chrono::Utc::now().timestamp()
    );
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
}
