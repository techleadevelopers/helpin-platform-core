use std::{
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use axum::{extract::State, Json};
use chrono::Utc;
use serde::Serialize;

use crate::{error::ApiError, state::AppState};

const IMPACT_METRICS_TTL: Duration = Duration::from_secs(30);
static IMPACT_METRICS_CACHE: OnceLock<Mutex<Option<(Instant, ImpactMetrics)>>> = OnceLock::new();

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImpactMetrics {
    pub resolved_cases: i64,
    pub animals_helped: i64,
    pub confirmed_help_cases: i64,
    pub active_protectors_30d: i64,
    pub active_verified_ongs: i64,
    pub repeat_helpers: i64,
    pub median_first_nearby_signal_seconds: Option<i64>,
    pub median_first_response_seconds: Option<i64>,
    pub median_geocode_activation_seconds: Option<i64>,
    pub generated_at: String,
}

pub async fn metrics(State(state): State<AppState>) -> Result<Json<ImpactMetrics>, ApiError> {
    if let Some(cached) = cached_metrics() {
        return Ok(Json(cached));
    }

    let db = &state.db;

    let resolved_cases = scalar_i64(
        db,
        r#"
        SELECT count(*)
        FROM posts
        WHERE rescue_status = 'resolved'
           OR resolved_at IS NOT NULL
        "#,
    )
    .await?;

    let animals_helped = scalar_i64(
        db,
        r#"
        SELECT count(DISTINCT post_id)
        FROM rescue_final_reports
        WHERE publication_status = 'published'
          AND status IN ('rescued', 'referred')
        "#,
    )
    .await?;

    let confirmed_help_cases = scalar_i64(
        db,
        r#"
        SELECT count(DISTINCT post_id)
        FROM rescue_responses
        WHERE status IN ('confirmed', 'arrived')
        "#,
    )
    .await?;

    let active_protectors_30d = scalar_i64(
        db,
        r#"
        SELECT count(DISTINCT user_id)
        FROM rescue_responses
        WHERE status IN ('confirmed', 'arrived')
          AND created_at >= now() - interval '30 days'
        "#,
    )
    .await?;

    let active_verified_ongs = scalar_i64(
        db,
        r#"
        SELECT count(DISTINCT op.id)
        FROM ong_profiles op
        JOIN users u ON u.id = op.user_id
        WHERE u.account_type = 'ong'
          AND (
            u.verified = true
            OR op.verified_at IS NOT NULL
            OR COALESCE(op.verification_status, '') = 'APPROVED'
          )
        "#,
    )
    .await?;

    let repeat_helpers = scalar_i64(
        db,
        r#"
        SELECT count(*)
        FROM (
          SELECT user_id
          FROM rescue_responses
          WHERE status IN ('confirmed', 'arrived')
          GROUP BY user_id
          HAVING count(DISTINCT post_id) >= 2
        ) helpers
        "#,
    )
    .await?;

    let median_first_nearby_signal_seconds = scalar_seconds(
        db,
        r#"
        SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY seconds)
        FROM (
          SELECT EXTRACT(EPOCH FROM (min(COALESCE(ne.acked_at, ne.read_at, ne.created_at)) - p.created_at))::double precision AS seconds
          FROM posts p
          JOIN notification_events ne ON ne.post_id = p.id::text
          WHERE ne.kind = 'rescue_alert'
            AND p.created_at >= now() - interval '90 days'
          GROUP BY p.id, p.created_at
        ) signals
        WHERE seconds >= 0
        "#,
    )
    .await?;

    let median_first_response_seconds = scalar_seconds(
        db,
        r#"
        SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY seconds)
        FROM (
          SELECT EXTRACT(EPOCH FROM (min(rr.created_at) - p.created_at))::double precision AS seconds
          FROM posts p
          JOIN rescue_responses rr ON rr.post_id = p.id
          WHERE rr.status IN ('confirmed', 'arrived')
            AND p.created_at >= now() - interval '90 days'
          GROUP BY p.id, p.created_at
        ) responses
        WHERE seconds >= 0
        "#,
    )
    .await?;

    let median_geocode_activation_seconds = scalar_seconds(
        db,
        r#"
        SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY seconds)
        FROM (
          SELECT EXTRACT(EPOCH FROM (geo_resolved_at - created_at))::double precision AS seconds
          FROM posts
          WHERE geo_resolved_at IS NOT NULL
            AND geo_source = 'address_geocoded'
            AND created_at >= now() - interval '90 days'
        ) geocoded
        WHERE seconds >= 0
        "#,
    )
    .await?;

    let metrics = ImpactMetrics {
        resolved_cases,
        animals_helped,
        confirmed_help_cases,
        active_protectors_30d,
        active_verified_ongs,
        repeat_helpers,
        median_first_nearby_signal_seconds,
        median_first_response_seconds,
        median_geocode_activation_seconds,
        generated_at: Utc::now().to_rfc3339(),
    };
    store_metrics(metrics.clone());
    Ok(Json(metrics))
}

async fn scalar_i64(db: &sqlx::PgPool, query: &str) -> Result<i64, ApiError> {
    Ok(sqlx::query_scalar::<_, i64>(query).fetch_one(db).await?)
}

async fn scalar_seconds(db: &sqlx::PgPool, query: &str) -> Result<Option<i64>, ApiError> {
    let value = sqlx::query_scalar::<_, Option<f64>>(query)
        .fetch_one(db)
        .await?;
    Ok(value
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .map(|seconds| seconds.round() as i64))
}

fn cached_metrics() -> Option<ImpactMetrics> {
    let cache = IMPACT_METRICS_CACHE.get_or_init(|| Mutex::new(None));
    let guard = cache.lock().ok()?;
    let (created_at, metrics) = guard.as_ref()?;
    (created_at.elapsed() <= IMPACT_METRICS_TTL).then(|| metrics.clone())
}

fn store_metrics(metrics: ImpactMetrics) {
    let cache = IMPACT_METRICS_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((Instant::now(), metrics));
    }
}
