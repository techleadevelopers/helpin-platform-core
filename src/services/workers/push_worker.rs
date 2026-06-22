use std::time::Duration;

use futures_util::{stream, StreamExt};
use reqwest::Client;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::config::Config;

const MAX_ATTEMPTS: i32 = 5;
const BATCH_SIZE: i64 = 50;
const DELIVERY_CONCURRENCY: usize = 10;
const EXPO_PUSH_URL: &str = "https://exp.host/--/api/v2/push/send";
const EXPO_RECEIPTS_URL: &str = "https://exp.host/--/api/v2/push/getReceipts";

#[derive(Debug)]
struct PushJob {
    id: Uuid,
    attempts: i32,
    push_token: String,
    payload: Value,
}

struct PushDeliveryResult {
    provider_response: Value,
    provider_ticket_id: Option<String>,
}

#[derive(Debug)]
struct PushReceiptJob {
    id: Uuid,
    provider_ticket_id: String,
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
            if let Err(error) = process_receipts(&config, &db, &client).await {
                tracing::warn!(?error, "push receipt batch failed");
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

    let jobs: Vec<_> = rows
        .into_iter()
        .map(|row| PushJob {
            id: row.get("id"),
            attempts: row.get("attempts"),
            push_token: row.get("push_token"),
            payload: row.get("payload"),
        })
        .collect();

    stream::iter(jobs)
        .for_each_concurrent(DELIVERY_CONCURRENCY, |job| async move {
            if let Err(error) = deliver_or_defer(config, db, client, job).await {
                tracing::warn!(?error, "push delivery job failed before deferral");
            }
        })
        .await;

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
            Ok(result) => {
                sqlx::query(
                    r#"
                    UPDATE push_delivery_jobs
                    SET status = 'provider_accepted',
                        updated_at = now(),
                        last_error = NULL,
                        provider_response = $2,
                        provider_ticket_id = $3,
                        provider_accepted_at = now()
                    WHERE id = $1
                    "#,
                )
                .bind(job.id)
                .bind(result.provider_response)
                .bind(result.provider_ticket_id)
                .execute(db)
                .await?;
                return Ok(());
            }
            Err(error) => {
                let message = error.to_string();
                if is_invalid_token_error(&message) {
                    invalidate_push_token(db, &job.push_token, &message).await?;
                }
                defer_delivery(db, job.id, job.attempts, message).await?;
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

async fn process_receipts(config: &Config, db: &PgPool, client: &Client) -> anyhow::Result<()> {
    if !matches!(config.push_provider.as_str(), "expo") {
        return Ok(());
    }

    let rows = sqlx::query(
        r#"
        SELECT id, provider_ticket_id
        FROM push_delivery_jobs
        WHERE status = 'provider_accepted'
          AND provider_ticket_id IS NOT NULL
          AND provider_accepted_at <= now() - interval '15 seconds'
          AND (
            receipt_checked_at IS NULL
            OR receipt_checked_at <= now() - interval '60 seconds'
          )
        ORDER BY provider_accepted_at ASC
        LIMIT $1
        "#,
    )
    .bind(BATCH_SIZE)
    .fetch_all(db)
    .await?;

    let jobs: Vec<_> = rows
        .into_iter()
        .filter_map(|row| {
            row.get::<Option<String>, _>("provider_ticket_id")
                .map(|provider_ticket_id| PushReceiptJob {
                    id: row.get("id"),
                    provider_ticket_id,
                })
        })
        .collect();

    for chunk in jobs.chunks(100) {
        check_receipt_chunk(config, db, client, chunk).await?;
    }

    Ok(())
}

async fn check_receipt_chunk(
    config: &Config,
    db: &PgPool,
    client: &Client,
    jobs: &[PushReceiptJob],
) -> anyhow::Result<()> {
    if jobs.is_empty() {
        return Ok(());
    }

    let ids: Vec<&str> = jobs
        .iter()
        .map(|job| job.provider_ticket_id.as_str())
        .collect();
    let mut request = client.post(EXPO_RECEIPTS_URL).json(&json!({ "ids": ids }));
    if let Some(token) = config.expo_access_token.as_deref() {
        request = request.bearer_auth(token);
    }

    let response = request.send().await?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or_else(|_| Value::Null);
    if !status.is_success() {
        anyhow::bail!("expo receipts returned HTTP {status}: {body}");
    }

    let data = body.get("data").and_then(Value::as_object);
    for job in jobs {
        let receipt = data
            .and_then(|data| data.get(&job.provider_ticket_id))
            .cloned()
            .unwrap_or(Value::Null);
        if receipt.is_null() {
            sqlx::query(
                r#"
                UPDATE push_delivery_jobs
                SET receipt_checked_at = now(),
                    receipt_response = $2,
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(job.id)
            .bind(receipt)
            .execute(db)
            .await?;
            continue;
        }

        let receipt_status = receipt
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if receipt_status == "ok" {
            sqlx::query(
                r#"
                UPDATE push_delivery_jobs
                SET status = 'delivered',
                    receipt_status = $2,
                    receipt_checked_at = now(),
                    receipt_response = $3,
                    delivered_at = now(),
                    updated_at = now(),
                    last_error = NULL
                WHERE id = $1
                "#,
            )
            .bind(job.id)
            .bind(receipt_status)
            .bind(receipt)
            .execute(db)
            .await?;
        } else {
            let message = expo_receipt_error(&receipt);
            sqlx::query(
                r#"
                UPDATE push_delivery_jobs
                SET status = 'failed',
                    receipt_status = $2,
                    receipt_checked_at = now(),
                    receipt_response = $3,
                    last_error = $4,
                    next_attempt_at = now() + interval '2 minutes',
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(job.id)
            .bind(receipt_status)
            .bind(receipt)
            .bind(&message)
            .execute(db)
            .await?;

            if is_invalid_token_error(&message) {
                if let Some(push_token) = sqlx::query_scalar::<_, String>(
                    "SELECT push_token FROM push_delivery_jobs WHERE id = $1",
                )
                .bind(job.id)
                .fetch_optional(db)
                .await?
                {
                    invalidate_push_token(db, &push_token, &message).await?;
                }
            }
        }
    }

    Ok(())
}

async fn send_expo_push(
    config: &Config,
    client: &Client,
    job: &PushJob,
) -> anyhow::Result<PushDeliveryResult> {
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

    Ok(PushDeliveryResult {
        provider_ticket_id: body
            .get("data")
            .and_then(|data| data.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string),
        provider_response: body,
    })
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

fn expo_receipt_error(receipt: &Value) -> String {
    let message = receipt
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("expo receipt rejected delivery");
    let details = receipt.get("details").cloned().unwrap_or(Value::Null);
    format!("{message}: {details}")
}

fn is_invalid_token_error(message: &str) -> bool {
    message.contains("DeviceNotRegistered") || message.contains("InvalidCredentials")
}

async fn invalidate_push_token(db: &PgPool, push_token: &str, error: &str) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE push_subscriptions
        SET invalidated_at = COALESCE(invalidated_at, now()),
            last_delivery_error = $2,
            updated_at = now()
        WHERE push_token = $1
        "#,
    )
    .bind(push_token)
    .bind(error)
    .execute(db)
    .await?;
    Ok(())
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
