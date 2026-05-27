use std::time::Duration;

use reqwest::Client;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::config::Config;

const MAX_ATTEMPTS: i32 = 5;
const BATCH_SIZE: i64 = 50;
const EXPO_PUSH_URL: &str = "https://exp.host/--/api/v2/push/send";

#[derive(Debug)]
struct PushJob {
    id: Uuid,
    attempts: i32,
    push_token: String,
    payload: Value,
}

pub fn spawn(config: Config, db: PgPool) {
    if !config.push_worker_enabled {
        return;
    }

    tokio::spawn(async move {
        let client = Client::new();
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            if let Err(error) = process_batch(&config, &db, &client).await {
                tracing::warn!(?error, "push worker batch failed");
            }
        }
    });
}

async fn process_batch(config: &Config, db: &PgPool, client: &Client) -> anyhow::Result<()> {
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
          LIMIT $1
          FOR UPDATE SKIP LOCKED
        )
        RETURNING id, attempts, push_token, payload
        "#,
    )
    .bind(BATCH_SIZE)
    .fetch_all(db)
    .await?;

    for row in rows {
        let job = PushJob {
            id: row.get("id"),
            attempts: row.get("attempts"),
            push_token: row.get("push_token"),
            payload: row.get("payload"),
        };
        deliver_or_defer(config, db, client, job).await?;
    }

    Ok(())
}

async fn deliver_or_defer(
    config: &Config,
    db: &PgPool,
    client: &Client,
    job: PushJob,
) -> anyhow::Result<()> {
    if matches!(config.push_provider.as_str(), "expo") {
        match send_expo_push(config, client, &job).await {
            Ok(()) => {
                sqlx::query(
                    r#"
                    UPDATE push_delivery_jobs
                    SET status = 'sent',
                        updated_at = now(),
                        last_error = NULL
                    WHERE id = $1
                    "#,
                )
                .bind(job.id)
                .execute(db)
                .await?;
                return Ok(());
            }
            Err(error) => {
                defer_delivery(db, job.id, job.attempts, error.to_string()).await?;
                return Ok(());
            }
        }
    }

    defer_delivery(
        db,
        job.id,
        job.attempts,
        format!(
            "push provider '{}' is not configured for real delivery",
            config.push_provider
        ),
    )
    .await
}

async fn send_expo_push(config: &Config, client: &Client, job: &PushJob) -> anyhow::Result<()> {
    let title = job
        .payload
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Helpin");
    let body = job
        .payload
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or("Novo alerta perto de voce");
    let deeplink = job
        .payload
        .get("deeplink")
        .and_then(Value::as_str)
        .unwrap_or("zoohelp://");
    let critical = job
        .payload
        .get("critical")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let message = json!({
        "to": job.push_token,
        "title": title,
        "body": body,
        "sound": "default",
        "priority": if critical { "high" } else { "default" },
        "channelId": if critical { "rescue-alerts" } else { "default" },
        "data": {
            "deeplink": deeplink,
            "postId": job.payload.get("postId").cloned().unwrap_or(Value::Null),
            "critical": critical,
            "category": "rescue"
        }
    });

    let mut request = client.post(EXPO_PUSH_URL).json(&message);
    if let Some(token) = config.expo_access_token.as_deref() {
        request = request.bearer_auth(token);
    }

    let response = request.send().await?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or_else(|_| Value::Null);
    if !status.is_success() {
        anyhow::bail!("expo push returned HTTP {status}: {body}");
    }
    if let Some(error) = expo_response_error(&body) {
        anyhow::bail!("{error}");
    }

    Ok(())
}

fn expo_response_error(body: &Value) -> Option<String> {
    let data = body.get("data")?;
    let status = data.get("status").and_then(Value::as_str);
    if status == Some("ok") {
        return None;
    }

    let message = data
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("expo push rejected delivery");
    let details = data.get("details").cloned().unwrap_or(Value::Null);
    Some(format!("{message}: {details}"))
}

async fn defer_delivery(
    db: &PgPool,
    job_id: Uuid,
    attempts: i32,
    error: String,
) -> anyhow::Result<()> {
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
    .bind(error)
    .execute(db)
    .await?;

    Ok(())
}
