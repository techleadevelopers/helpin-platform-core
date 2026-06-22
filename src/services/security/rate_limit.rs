use std::time::{Duration, Instant};

use axum::http::HeaderMap;

use crate::{error::ApiError, state::AppState};

pub async fn check_key(
    state: &AppState,
    key: &str,
    max_requests: usize,
    window: Duration,
) -> Result<(), ApiError> {
    if let Some(redis) = &state.redis {
        match check_redis(redis, key, max_requests, window).await {
            Ok(()) => return Ok(()),
            Err(error) if state.config.is_development() => {
                tracing::warn!(
                    ?error,
                    key,
                    "Redis rate limit unavailable; using local dev fallback"
                );
            }
            Err(error) => {
                tracing::error!(?error, key, "Redis rate limit unavailable");
                return Err(ApiError::ServiceUnavailable);
            }
        }
    }

    let now = Instant::now();
    let mut guard = state.rate_limiter.lock().map_err(|_| ApiError::Internal)?;
    let entries = guard.entry(key.to_string()).or_default();
    entries.retain(|instant| now.duration_since(*instant) <= window);

    if entries.len() >= max_requests {
        return Err(ApiError::TooManyRequests);
    }

    entries.push(now);
    Ok(())
}

pub fn client_ip(headers: &HeaderMap) -> String {
    for name in ["cf-connecting-ip", "x-real-ip", "x-forwarded-for"] {
        if let Some(value) = headers.get(name).and_then(|value| value.to_str().ok()) {
            if let Some(first) = value
                .split(',')
                .map(str::trim)
                .find(|part| !part.is_empty())
            {
                return first.chars().take(80).collect();
            }
        }
    }
    "unknown".into()
}

pub async fn check_ip(
    state: &AppState,
    headers: &HeaderMap,
    action: &str,
    max_requests: usize,
    window: Duration,
) -> Result<(), ApiError> {
    let ip = client_ip(headers);
    check_key(state, &format!("ip:{ip}:{action}"), max_requests, window).await
}

pub async fn check_user(
    state: &AppState,
    user_id: &str,
    action: &str,
    max_requests: usize,
    window: Duration,
) -> Result<(), ApiError> {
    check_key(
        state,
        &format!("user:{user_id}:{action}"),
        max_requests,
        window,
    )
    .await
}

async fn check_redis(
    redis: &redis::Client,
    key: &str,
    max_requests: usize,
    window: Duration,
) -> Result<(), redis::RedisError> {
    let mut conn = redis.get_multiplexed_async_connection().await?;
    let redis_key = format!("rate:{key}");
    let (count, _): (i64, bool) = redis::pipe()
        .atomic()
        .incr(&redis_key, 1)
        .expire(&redis_key, window.as_secs() as i64)
        .query_async(&mut conn)
        .await?;

    if count > max_requests as i64 {
        return Err(redis::RedisError::from((
            redis::ErrorKind::BusyLoadingError,
            "rate limit exceeded",
        )));
    }

    Ok(())
}
