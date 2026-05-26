use std::time::Duration;

use sqlx::Row;
use uuid::Uuid;

use crate::{routes::maps, services::rescue_fanout, state::AppState};

const BATCH_SIZE: i64 = 20;
const MAX_ATTEMPTS: i32 = 5;

pub fn spawn(state: AppState) {
    if state.config.app_env == "test" {
        return;
    }

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            if let Err(error) = process_due_jobs(&state).await {
                tracing::warn!(?error, "post geocoding worker batch failed");
            }
        }
    });
}

async fn process_due_jobs(state: &AppState) -> Result<(), sqlx::Error> {
    let jobs = sqlx::query(
        r#"
        SELECT post_id, address_label, attempts
        FROM post_geocode_jobs
        WHERE status IN ('pending', 'processing')
          AND next_run_at <= now()
        ORDER BY next_run_at ASC
        LIMIT $1
        "#,
    )
    .bind(BATCH_SIZE)
    .fetch_all(&state.db)
    .await?;

    for job in jobs {
        let post_id: Uuid = job.get("post_id");
        let address_label: String = job.get("address_label");
        let attempts: i32 = job.get("attempts");
        sqlx::query(
            "UPDATE post_geocode_jobs SET status = 'processing', attempts = attempts + 1, updated_at = now() WHERE post_id = $1",
        )
        .bind(post_id)
        .execute(&state.db)
        .await?;

        match maps::geocode_address(state, &address_label).await {
            Ok(Some(result)) => resolve_post(state, post_id, result.latitude, result.longitude).await?,
            Ok(None) => fail_or_retry(state, post_id, attempts + 1, "address not found").await?,
            Err(error) => fail_or_retry(state, post_id, attempts + 1, &error.to_string()).await?,
        }
    }

    Ok(())
}

async fn resolve_post(
    state: &AppState,
    post_id: Uuid,
    latitude: f64,
    longitude: f64,
) -> Result<(), sqlx::Error> {
    let sql = if state.config.postgis_enabled {
        r#"
        UPDATE posts
        SET latitude = $2, longitude = $3, geo_status = 'confirmed',
            geo_source = 'address_geocoded', geo_provider = 'google',
            geo_confidence = 1.0, geo_resolved_at = now(),
            geo = ST_SetSRID(ST_MakePoint($3, $2), 4326)::geography
        WHERE id = $1
        RETURNING urgent OR post_type::text = 'emergency'
        "#
    } else {
        r#"
        UPDATE posts
        SET latitude = $2, longitude = $3, geo_status = 'confirmed',
            geo_source = 'address_geocoded', geo_provider = 'google',
            geo_confidence = 1.0, geo_resolved_at = now()
        WHERE id = $1
        RETURNING urgent OR post_type::text = 'emergency'
        "#
    };
    let operational: bool = sqlx::query_scalar(sql)
        .bind(post_id)
        .bind(latitude)
        .bind(longitude)
        .fetch_one(&state.db)
        .await?;
    sqlx::query(
        "UPDATE post_geocode_jobs SET status = 'completed', last_error = NULL, updated_at = now() WHERE post_id = $1",
    )
    .bind(post_id)
    .execute(&state.db)
    .await?;

    if operational {
        sqlx::query("UPDATE posts SET rescue_status = 'active' WHERE id = $1 AND rescue_status = 'open'")
            .bind(post_id)
            .execute(&state.db)
            .await?;
        rescue_fanout::create_fanout_state_for_post(&state.db, post_id, None).await?;
    }
    Ok(())
}

async fn fail_or_retry(
    state: &AppState,
    post_id: Uuid,
    attempts: i32,
    message: &str,
) -> Result<(), sqlx::Error> {
    if attempts >= MAX_ATTEMPTS {
        sqlx::query(
            "UPDATE post_geocode_jobs SET status = 'failed', last_error = $2, updated_at = now() WHERE post_id = $1",
        )
        .bind(post_id)
        .bind(message)
        .execute(&state.db)
        .await?;
        sqlx::query("UPDATE posts SET geo_status = 'failed' WHERE id = $1")
            .bind(post_id)
            .execute(&state.db)
            .await?;
    } else {
        sqlx::query(
            r#"
            UPDATE post_geocode_jobs
            SET status = 'pending', last_error = $2,
                next_run_at = now() + ($3::text || ' seconds')::interval,
                updated_at = now()
            WHERE post_id = $1
            "#,
        )
        .bind(post_id)
        .bind(message)
        .bind(5_i32.pow(attempts as u32))
        .execute(&state.db)
        .await?;
    }
    Ok(())
}
