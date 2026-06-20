use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;
use validator::Validate;

use crate::{error::ApiError, routes::auth::authenticate_request, state::AppState};

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct DonationIntentRequest {
    pub ong_id: String,
    #[validate(range(min = 100))]
    pub amount_cents: i64,
    pub currency: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceContributionRequest {
    #[validate(range(min = 10, max = 100))]
    pub amount_cents: i64,
    #[validate(length(max = 280))]
    pub public_message: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DonationIntentResponse {
    pub id: String,
    pub ong_id: Option<String>,
    pub amount_cents: i64,
    pub currency: String,
    pub purpose: String,
    pub recurrence: String,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentWebhookRequest {
    pub event_id: String,
    pub event_type: String,
    pub provider_reference: String,
    pub status: String,
    pub amount_cents: Option<i64>,
    pub currency: Option<String>,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[derive(Serialize)]
pub struct PaymentWebhookResponse {
    pub status: &'static str,
    pub donation_id: Option<String>,
}

pub async fn create_intent(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<DonationIntentRequest>,
) -> Result<Json<DonationIntentResponse>, ApiError> {
    if !state.config.payments_enabled {
        return Err(ApiError::ServiceUnavailable);
    }

    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    let claims = authenticate_request(&state, &headers)?;
    let donor_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    let ong_id = Uuid::parse_str(&payload.ong_id).map_err(|_| ApiError::NotFound)?;
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let currency = normalize_currency(payload.currency.as_deref())?;

    let ong_exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM ong_profiles WHERE id = $1")
        .bind(ong_id)
        .fetch_optional(&state.db)
        .await?;
    if ong_exists.is_none() {
        return Err(ApiError::NotFound);
    }

    if let Some(key) = &idempotency_key {
        if let Some(existing) = find_existing_intent(&state, donor_id, key).await? {
            return Ok(Json(existing));
        }
    }

    let provider = &state.config.payment_provider;
    if !state.config.is_development() && provider == "manual_psp_required" {
        return Err(ApiError::ServiceUnavailable);
    }

    let mut tx = state.db.begin().await?;
    let provider_reference = format!("{provider}-{}", Uuid::now_v7());
    let row = sqlx::query(
        r#"
        INSERT INTO donations (
          donor_id, ong_id, amount_cents, currency, provider, provider_reference,
          status, idempotency_key, purpose, recurrence
        )
        VALUES ($1, $2, $3, $4, $5, $6, 'pending_provider', $7, 'ong_donation', 'one_time')
        RETURNING id, ong_id, amount_cents, currency, purpose, recurrence, status
        "#,
    )
    .bind(donor_id)
    .bind(ong_id)
    .bind(payload.amount_cents)
    .bind(&currency)
    .bind(provider)
    .bind(&provider_reference)
    .bind(idempotency_key.as_deref())
    .fetch_one(&mut *tx)
    .await?;

    let donation_id: Uuid = row.get("id");
    sqlx::query(
        r#"
        INSERT INTO donation_ledger_entries (
          donation_id, entry_type, amount_cents, currency, metadata
        )
        VALUES ($1, 'intent_created', $2, $3, $4)
        "#,
    )
    .bind(donation_id)
    .bind(payload.amount_cents)
    .bind(&currency)
    .bind(serde_json::json!({ "provider": provider, "providerReference": provider_reference }))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(row_to_response(row)))
}

pub async fn create_maintenance_intent(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<MaintenanceContributionRequest>,
) -> Result<Json<DonationIntentResponse>, ApiError> {
    if !state.config.payments_enabled {
        return Err(ApiError::ServiceUnavailable);
    }

    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    let claims = authenticate_request(&state, &headers)?;
    let donor_id = Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)?;
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if let Some(key) = &idempotency_key {
        if let Some(existing) = find_existing_intent(&state, donor_id, key).await? {
            return Ok(Json(existing));
        }
    }

    let provider = &state.config.payment_provider;
    if !state.config.is_development() && provider == "manual_psp_required" {
        return Err(ApiError::ServiceUnavailable);
    }

    let message = payload
        .public_message
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let provider_reference = format!("{provider}-maintenance-{}", Uuid::now_v7());

    let mut tx = state.db.begin().await?;
    let row = sqlx::query(
        r#"
        INSERT INTO donations (
          donor_id, ong_id, amount_cents, currency, provider, provider_reference,
          status, idempotency_key, purpose, recurrence, public_message
        )
        VALUES ($1, NULL, $2, 'BRL', $3, $4, 'pending_provider', $5,
                'platform_maintenance', 'monthly', $6)
        RETURNING id, ong_id, amount_cents, currency, purpose, recurrence, status
        "#,
    )
    .bind(donor_id)
    .bind(payload.amount_cents)
    .bind(provider)
    .bind(&provider_reference)
    .bind(idempotency_key.as_deref())
    .bind(message)
    .fetch_one(&mut *tx)
    .await?;

    let donation_id: Uuid = row.get("id");
    sqlx::query(
        r#"
        INSERT INTO donation_ledger_entries (
          donation_id, entry_type, amount_cents, currency, metadata
        )
        VALUES ($1, 'maintenance_intent_created', $2, 'BRL', $3)
        "#,
    )
    .bind(donation_id)
    .bind(payload.amount_cents)
    .bind(serde_json::json!({
        "provider": provider,
        "providerReference": provider_reference,
        "copy": maintenance_copy()
    }))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(row_to_response(row)))
}

pub async fn payment_webhook(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<PaymentWebhookRequest>,
) -> Result<Json<PaymentWebhookResponse>, ApiError> {
    if !state.config.payments_enabled {
        return Err(ApiError::ServiceUnavailable);
    }

    verify_webhook_secret(&state, &headers)?;
    if provider != state.config.payment_provider {
        return Err(ApiError::NotFound);
    }

    let donation = sqlx::query(
        r#"
        SELECT id, amount_cents, currency
        FROM donations
        WHERE provider = $1 AND provider_reference = $2
        "#,
    )
    .bind(&provider)
    .bind(&payload.provider_reference)
    .fetch_optional(&state.db)
    .await?;

    let Some(donation) = donation else {
        let _ = insert_webhook_event(&state, &provider, &payload, None).await?;
        return Ok(Json(PaymentWebhookResponse {
            status: "ignored",
            donation_id: None,
        }));
    };

    let donation_id: Uuid = donation.get("id");
    let inserted = insert_webhook_event(&state, &provider, &payload, Some(donation_id)).await?;
    if !inserted {
        return Ok(Json(PaymentWebhookResponse {
            status: "duplicate",
            donation_id: Some(donation_id.to_string()),
        }));
    }

    let next_status = normalize_payment_status(&payload.status)?;
    let amount_cents = payload
        .amount_cents
        .unwrap_or_else(|| donation.get::<i64, _>("amount_cents"));
    let currency = normalize_currency(
        payload
            .currency
            .as_deref()
            .or_else(|| Some(donation.get::<&str, _>("currency"))),
    )?;

    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE donations SET status = $1 WHERE id = $2")
        .bind(next_status)
        .bind(donation_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        r#"
        INSERT INTO donation_ledger_entries (donation_id, entry_type, amount_cents, currency, metadata)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(donation_id)
    .bind(format!("provider_{next_status}"))
    .bind(amount_cents)
    .bind(currency)
    .bind(serde_json::json!({
        "provider": provider,
        "providerEventId": payload.event_id,
        "eventType": payload.event_type
    }))
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE payment_webhook_events SET processed_at = now() WHERE provider = $1 AND provider_event_id = $2",
    )
    .bind(&provider)
    .bind(&payload.event_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(PaymentWebhookResponse {
        status: "processed",
        donation_id: Some(donation_id.to_string()),
    }))
}

async fn find_existing_intent(
    state: &AppState,
    donor_id: Uuid,
    idempotency_key: &str,
) -> Result<Option<DonationIntentResponse>, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT id, ong_id, amount_cents, currency, purpose, recurrence, status
        FROM donations
        WHERE donor_id = $1 AND idempotency_key = $2
        "#,
    )
    .bind(donor_id)
    .bind(idempotency_key)
    .fetch_optional(&state.db)
    .await?;

    Ok(row.map(row_to_response))
}

fn row_to_response(row: sqlx::postgres::PgRow) -> DonationIntentResponse {
    DonationIntentResponse {
        id: row.get::<Uuid, _>("id").to_string(),
        ong_id: row
            .get::<Option<Uuid>, _>("ong_id")
            .map(|value| value.to_string()),
        amount_cents: row.get("amount_cents"),
        currency: row.get::<String, _>("currency"),
        purpose: row.get("purpose"),
        recurrence: row.get("recurrence"),
        status: row.get("status"),
        message: maintenance_copy().to_string(),
    }
}

fn maintenance_copy() -> &'static str {
    "O ZooHelp continuará gratuito para resgates, adoções e apoio animal. Esta contribuição voluntária ajuda a manter servidores, notificações, armazenamento de fotos, monitoramento, moderação e operação da plataforma."
}

fn normalize_currency(value: Option<&str>) -> Result<String, ApiError> {
    let currency = value.unwrap_or("BRL").trim().to_ascii_uppercase();
    if currency.len() != 3 || !currency.chars().all(|ch| ch.is_ascii_uppercase()) {
        return Err(ApiError::Validation(
            "currency must be ISO-4217 alpha-3".into(),
        ));
    }
    Ok(currency)
}

fn verify_webhook_secret(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let expected = state
        .config
        .payment_webhook_secret
        .as_deref()
        .ok_or(ApiError::ServiceUnavailable)?;
    let received = headers
        .get("x-zoohelp-webhook-secret")
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::Unauthorized)?;
    if received != expected {
        return Err(ApiError::Unauthorized);
    }
    Ok(())
}

async fn insert_webhook_event(
    state: &AppState,
    provider: &str,
    payload: &PaymentWebhookRequest,
    donation_id: Option<Uuid>,
) -> Result<bool, ApiError> {
    let result = sqlx::query(
        r#"
        INSERT INTO payment_webhook_events (
          provider, provider_event_id, event_type, donation_id, payload
        )
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (provider, provider_event_id) DO NOTHING
        "#,
    )
    .bind(provider)
    .bind(&payload.event_id)
    .bind(&payload.event_type)
    .bind(donation_id)
    .bind(&payload.payload)
    .execute(&state.db)
    .await?;

    Ok(result.rows_affected() > 0)
}

fn normalize_payment_status(value: &str) -> Result<&'static str, ApiError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "paid" | "succeeded" | "approved" => Ok("paid"),
        "failed" | "rejected" | "declined" => Ok("failed"),
        "refunded" => Ok("refunded"),
        "chargeback" | "disputed" => Ok("chargeback"),
        "pending" | "processing" => Ok("pending_provider"),
        _ => Err(ApiError::Validation("unsupported payment status".into())),
    }
}
