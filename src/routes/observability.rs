use std::{fs, time::Instant};

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Serialize;

use crate::{
    domain::AccountType, error::ApiError, services::auth as auth_service, state::AppState,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservabilityStatus {
    pub status: &'static str,
    pub service: &'static str,
    pub tracing: &'static str,
    pub metrics: &'static str,
    pub sentry: SentryStatus,
    pub redis_rate_limit: &'static str,
    pub event_bus: &'static str,
    pub push_worker: &'static str,
    pub postgis: &'static str,
    pub payments: &'static str,
    pub payment_provider: String,
    pub db: DbHealth,
    pub memory: MemoryHealth,
    pub queues: QueueHealth,
    pub runtime: RuntimeHealth,
    pub stack: ObservabilityStack,
    pub links: ObservabilityLinks,
    pub active_sessions: i64,
    pub active_rescue_sessions: i64,
    pub active_chat_rooms: i64,
    pub latency_series: Vec<LatencyPoint>,
    pub latency_averages: LatencyAverages,
    pub api_latency_ms: f64,
    pub sentry_latency_ms: f64,
    pub timestamp: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbHealth {
    pub status: &'static str,
    pub latency_ms: f64,
    pub pool_size: u32,
    pub idle_connections: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryHealth {
    pub rss_mb: f64,
    pub heap_used_mb: f64,
    pub heap_total_mb: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueHealth {
    pub queued_push_jobs: i64,
    pub dead_letter_push_jobs: i64,
    pub queued_moderation_jobs: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeHealth {
    pub uptime_seconds: i64,
    pub started_at: String,
    pub app_env: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservabilityStack {
    pub prometheus: &'static str,
    pub grafana: &'static str,
    pub opentelemetry: &'static str,
    pub otlp_endpoint: Option<String>,
    pub traces: &'static str,
    pub logs: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservabilityLinks {
    pub metrics_path: &'static str,
    pub readiness_path: &'static str,
    pub grafana_dashboard_uid: &'static str,
    pub prometheus_job: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SentryStatus {
    pub configured: bool,
    pub status: &'static str,
    pub total_unresolved: Option<i64>,
    pub crash_free_sessions: Option<f64>,
    pub by_platform: SentryPlatformStatus,
    pub recent_issues: Vec<SentryIssue>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SentryPlatformStatus {
    pub android: i64,
    pub ios: i64,
    pub other: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SentryIssue {
    pub id: String,
    pub title: String,
    pub platform: String,
    pub last_seen: String,
    pub stack_trace: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyPoint {
    pub timestamp: String,
    pub register_latency: Option<f64>,
    pub radius_latency: Option<f64>,
    pub booking_latency: Option<f64>,
    pub payment_latency: Option<f64>,
    pub critical_average: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyAverages {
    pub register_latency: Option<f64>,
    pub radius_latency: Option<f64>,
    pub booking_latency: Option<f64>,
    pub payment_latency: Option<f64>,
}

pub async fn status(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Json<ObservabilityStatus>, ApiError> {
    authenticate_observability(&state, &headers)?;
    Ok(Json(build_status(&state).await))
}

pub async fn metrics(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    authenticate_observability(&state, &headers)?;
    let snapshot = build_status(&state).await;
    let db_up = if snapshot.db.status == "up" { 1 } else { 0 };
    let sentry_configured = if snapshot.sentry.configured { 1 } else { 0 };
    let otlp_configured = if snapshot.stack.otlp_endpoint.is_some() {
        1
    } else {
        0
    };
    let body = format!(
        "# HELP zoohelp_backend_up Backend process availability\n\
         # TYPE zoohelp_backend_up gauge\n\
         zoohelp_backend_up 1\n\
         # HELP zoohelp_backend_timestamp_seconds Current unix timestamp\n\
         # TYPE zoohelp_backend_timestamp_seconds gauge\n\
         zoohelp_backend_timestamp_seconds {}\n\
         # HELP zoohelp_backend_uptime_seconds Backend process uptime\n\
         # TYPE zoohelp_backend_uptime_seconds gauge\n\
         zoohelp_backend_uptime_seconds {}\n\
         # HELP zoohelp_database_up Database readiness\n\
         # TYPE zoohelp_database_up gauge\n\
         zoohelp_database_up {}\n\
         # HELP zoohelp_database_latency_ms Database SELECT 1 latency\n\
         # TYPE zoohelp_database_latency_ms gauge\n\
         zoohelp_database_latency_ms {}\n\
         # HELP zoohelp_database_pool_size Open SQLx pool size\n\
         # TYPE zoohelp_database_pool_size gauge\n\
         zoohelp_database_pool_size {}\n\
         # HELP zoohelp_database_pool_idle Idle SQLx pool connections\n\
         # TYPE zoohelp_database_pool_idle gauge\n\
         zoohelp_database_pool_idle {}\n\
         # HELP zoohelp_push_jobs_queued Queued push delivery jobs\n\
         # TYPE zoohelp_push_jobs_queued gauge\n\
         zoohelp_push_jobs_queued {}\n\
         # HELP zoohelp_push_jobs_dead_letter Dead-lettered push delivery jobs\n\
         # TYPE zoohelp_push_jobs_dead_letter gauge\n\
         zoohelp_push_jobs_dead_letter {}\n\
         # HELP zoohelp_moderation_jobs_queued Queued moderation jobs\n\
         # TYPE zoohelp_moderation_jobs_queued gauge\n\
         zoohelp_moderation_jobs_queued {}\n\
         # HELP zoohelp_rescue_sessions_active Active rescue sessions\n\
         # TYPE zoohelp_rescue_sessions_active gauge\n\
         zoohelp_rescue_sessions_active {}\n\
         # HELP zoohelp_chat_rooms_active Active chat rooms\n\
         # TYPE zoohelp_chat_rooms_active gauge\n\
         zoohelp_chat_rooms_active {}\n\
         # HELP zoohelp_sentry_configured Sentry DSN configured\n\
         # TYPE zoohelp_sentry_configured gauge\n\
         zoohelp_sentry_configured {}\n\
         # HELP zoohelp_otlp_configured OTLP endpoint configured\n\
         # TYPE zoohelp_otlp_configured gauge\n\
         zoohelp_otlp_configured {}\n",
        chrono::Utc::now().timestamp(),
        snapshot.runtime.uptime_seconds,
        db_up,
        snapshot.db.latency_ms,
        snapshot.db.pool_size,
        snapshot.db.idle_connections,
        snapshot.queues.queued_push_jobs,
        snapshot.queues.dead_letter_push_jobs,
        snapshot.queues.queued_moderation_jobs,
        snapshot.active_rescue_sessions,
        snapshot.active_chat_rooms,
        sentry_configured,
        otlp_configured,
    );
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    ))
}

async fn build_status(state: &AppState) -> ObservabilityStatus {
    let started = Instant::now();
    let db = database_health(state).await;
    let db_latency_ms = db.latency_ms;
    let api_latency_ms = started.elapsed().as_secs_f64() * 1000.0;
    let queue = QueueHealth {
        queued_push_jobs: count_scalar(
            state,
            "SELECT count(*) FROM push_delivery_jobs WHERE status = 'queued'",
        )
        .await,
        dead_letter_push_jobs: count_scalar(
            state,
            "SELECT count(*) FROM push_delivery_jobs WHERE status = 'dead_letter'",
        )
        .await,
        queued_moderation_jobs: count_scalar(
            state,
            "SELECT count(*) FROM moderation_jobs WHERE status IN ('queued', 'needs_review')",
        )
        .await,
    };
    let active_rescue_sessions = count_scalar(
        state,
        "SELECT count(*) FROM rescue_sessions WHERE status = 'active'",
    )
    .await;
    let active_chat_rooms = count_scalar(state, "SELECT count(*) FROM chat_rooms").await;
    let status = if db.status == "up" && queue.dead_letter_push_jobs == 0 {
        "ok"
    } else {
        "degraded"
    };

    ObservabilityStatus {
        status,
        service: "zoohelp-backend",
        tracing: "enabled",
        metrics: "enabled",
        sentry: sentry_status(state),
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
        postgis: if state.config.postgis_enabled {
            "enabled"
        } else {
            "disabled_lat_lng_fallback"
        },
        payments: if state.config.payments_enabled {
            "enabled"
        } else {
            "disabled_until_community_support_phase"
        },
        payment_provider: state.config.payment_provider.clone(),
        db,
        memory: process_memory_mb(),
        queues: queue,
        runtime: RuntimeHealth {
            uptime_seconds: (chrono::Utc::now() - state.started_at).num_seconds(),
            started_at: state.started_at.to_rfc3339(),
            app_env: state.config.app_env.clone(),
        },
        stack: ObservabilityStack {
            prometheus: "scrape_metrics_endpoint",
            grafana: "provisioned_dashboard",
            opentelemetry: if state.config.otel_exporter_otlp_endpoint.is_some() {
                "otlp_exporter_enabled"
            } else {
                "json_tracing_only"
            },
            otlp_endpoint: state.config.otel_exporter_otlp_endpoint.clone(),
            traces: if state.config.otel_exporter_otlp_endpoint.is_some() {
                "otel_trace_layer"
            } else {
                "tower_http_trace_json_logs"
            },
            logs: "structured_json",
        },
        links: ObservabilityLinks {
            metrics_path: "/metrics",
            readiness_path: "/readyz",
            grafana_dashboard_uid: "zoohelp-core-overview",
            prometheus_job: "zoohelp-backend",
        },
        active_sessions: active_rescue_sessions,
        active_rescue_sessions,
        active_chat_rooms,
        latency_series: Vec::new(),
        latency_averages: LatencyAverages {
            register_latency: None,
            radius_latency: Some(db_latency_ms),
            booking_latency: None,
            payment_latency: None,
        },
        api_latency_ms,
        sentry_latency_ms: 0.0,
        timestamp: chrono::Utc::now().to_rfc3339(),
    }
}

fn authenticate_observability(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
        })
        .ok_or(ApiError::Unauthorized)?;
    let claims = auth_service::verify_access_token(&state.config, token)
        .map_err(|_| ApiError::Unauthorized)?;
    if !matches!(claims.account_type, AccountType::Admin) {
        return Err(ApiError::Forbidden);
    }
    Ok(())
}

async fn database_health(state: &AppState) -> DbHealth {
    let started = Instant::now();
    let healthy = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();

    DbHealth {
        status: if healthy { "up" } else { "down" },
        latency_ms: started.elapsed().as_secs_f64() * 1000.0,
        pool_size: state.db.size(),
        idle_connections: state.db.num_idle(),
    }
}

fn sentry_status(state: &AppState) -> SentryStatus {
    SentryStatus {
        configured: state.config.sentry_dsn.is_some(),
        status: if state.config.sentry_dsn.is_some() {
            "configured"
        } else {
            "missing"
        },
        total_unresolved: None,
        crash_free_sessions: None,
        by_platform: SentryPlatformStatus {
            android: 0,
            ios: 0,
            other: 0,
        },
        recent_issues: Vec::new(),
    }
}

async fn count_scalar(state: &AppState, sql: &str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(&state.db)
        .await
        .unwrap_or(0)
}

#[cfg(target_os = "linux")]
fn process_memory_mb() -> MemoryHealth {
    let page_size = 4096.0;
    let statm = fs::read_to_string("/proc/self/statm").unwrap_or_default();
    let parts: Vec<f64> = statm
        .split_whitespace()
        .filter_map(|value| value.parse::<f64>().ok())
        .collect();
    let rss_mb = parts.get(1).copied().unwrap_or_default() * page_size / 1024.0 / 1024.0;

    MemoryHealth {
        rss_mb,
        heap_used_mb: 0.0,
        heap_total_mb: rss_mb,
    }
}

#[cfg(not(target_os = "linux"))]
fn process_memory_mb() -> MemoryHealth {
    let _ = fs::metadata(".");
    MemoryHealth {
        rss_mb: 0.0,
        heap_used_mb: 0.0,
        heap_total_mb: 0.0,
    }
}
