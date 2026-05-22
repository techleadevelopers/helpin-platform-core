use axum::{extract::State, http::HeaderMap, Json};
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DonationIntentResponse {
    pub id: String,
    pub ong_id: String,
    pub amount_cents: i64,
    pub currency: String,
    pub status: String,
}

pub async fn create_intent(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<DonationIntentRequest>,
) -> Result<Json<DonationIntentResponse>, ApiError> {
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

    let mut tx = state.db.begin().await?;
    let row = sqlx::query(
        r#"
        INSERT INTO donations (
          donor_id, ong_id, amount_cents, currency, provider, provider_reference, status, idempotency_key
        )
        VALUES ($1, $2, $3, $4, 'manual_psp_required', $5, 'pending_provider', $6)
        RETURNING id, ong_id, amount_cents, currency, status
        "#,
    )
    .bind(donor_id)
    .bind(ong_id)
    .bind(payload.amount_cents)
    .bind(&currency)
    .bind(format!("zoohelp-{}", Uuid::now_v7()))
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
    .bind(serde_json::json!({ "provider": "manual_psp_required" }))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(row_to_response(row)))
}

async fn find_existing_intent(
    state: &AppState,
    donor_id: Uuid,
    idempotency_key: &str,
) -> Result<Option<DonationIntentResponse>, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT id, ong_id, amount_cents, currency, status
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
        ong_id: row.get::<Uuid, _>("ong_id").to_string(),
        amount_cents: row.get("amount_cents"),
        currency: row.get::<String, _>("currency"),
        status: row.get("status"),
    }
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
