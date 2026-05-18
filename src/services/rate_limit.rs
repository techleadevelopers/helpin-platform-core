use std::time::{Duration, Instant};

use crate::{error::ApiError, state::AppState};

pub fn check_key(state: &AppState, key: &str, max_requests: usize, window: Duration) -> Result<(), ApiError> {
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
