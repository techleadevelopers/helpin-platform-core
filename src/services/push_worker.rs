use std::time::Duration;

use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::config::Config;

const MAX_ATTEMPTS: i32 = 5;

pub fn spawn(config: Config, db: PgPool) {
    if !config.push_worker_enabled {
        return;
    }

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            if let Err(error) = process_batch(&config, &db).await {
                tracing::warn!(?error, "push worker batch failed");
            }
        }
    });
}

async fn process_batch(config: &Config, db: &PgPool) -> anyhow::Result<()> {
    let rows = sqlx::query(
        r#"
        UPDATE push_delivery_jobs
        SET status = 'failed',
            attempts = attempts + 1,
            updated_at = now(),
            last_error = 'claimed for delivery'
        WHERE id IN (
          SELECT id
          FROM push_delivery_jobs
          WHERE status IN ('queued', 'failed')
            AND next_attempt_at <= now()
          ORDER BY created_at ASC
          LIMIT 50
          FOR UPDATE SKIP LOCKED
        )
        RETURNING id, attempts
        "#,
    )
    .fetch_all(db)
    .await?;

    for row in rows {
        let job_id: Uuid = row.get("id");
        let attempts: i32 = row.get("attempts");
        deliver_or_defer(config, db, job_id, attempts).await?;
    }

    Ok(())
}

async fn deliver_or_defer(
    config: &Config,
    db: &PgPool,
    job_id: Uuid,
    attempts: i32,
) -> anyhow::Result<()> {
    // This worker intentionally fails closed until a concrete push provider
    // client is configured. It preserves retries/DLQ instead of pretending a
    // critical rescue alert was delivered.
    let provider_ready = matches!(config.push_provider.as_str(), "expo") && false;
    if provider_ready {
        sqlx::query(
            r#"
            UPDATE push_delivery_jobs
            SET status = 'sent',
                updated_at = now(),
                last_error = NULL
            WHERE id = $1
            "#,
        )
        .bind(job_id)
        .execute(db)
        .await?;
        return Ok(());
    }

    let next_status = if attempts >= MAX_ATTEMPTS {
        "dead_letter"
    } else {
        "failed"
    };
    let delay_minutes = 2_i64.pow(attempts.clamp(1, 5) as u32);
    sqlx::query(
        r#"
        UPDATE push_delivery_jobs
        SET status = $2,
            next_attempt_at = CASE
              WHEN $2 = 'dead_letter' THEN next_attempt_at
              ELSE now() + ($3::text || ' minutes')::interval
            END,
            last_error = $4,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(job_id)
    .bind(next_status)
    .bind(delay_minutes)
    .bind(format!(
        "push provider '{}' is not configured for real delivery",
        config.push_provider
    ))
    .execute(db)
    .await?;

    Ok(())
}
