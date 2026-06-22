use axum::{
    extract::{Path, State},
    http::HeaderMap,
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;
use validator::Validate;

use crate::{
    error::ApiError, routes::auth::authenticate_request, services::rate_limit, state::AppState,
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportTicket {
    pub id: String,
    pub subject: String,
    pub status: String,
    pub category: String,
    pub severity: String,
    pub created_at: String,
    pub messages: Vec<SupportMessage>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportMessage {
    pub id: String,
    pub body: String,
    pub author_type: String,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct SupportMeta {
    pub categories: Vec<&'static str>,
    pub severities: Vec<&'static str>,
}

#[derive(Deserialize, Validate)]
pub struct CreateTicketRequest {
    #[validate(length(min = 1, max = 160))]
    pub subject: String,
    #[validate(length(min = 1, max = 4000))]
    pub body: String,
    pub category: Option<String>,
    pub severity: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct AddMessageRequest {
    #[validate(length(min = 1, max = 4000))]
    pub body: String,
}

pub async fn meta() -> Json<SupportMeta> {
    Json(SupportMeta {
        categories: vec!["RESCUE", "APP", "DONATION", "SAFETY", "OTHER"],
        severities: vec!["LOW", "MEDIUM", "HIGH", "URGENT"],
    })
}

pub async fn list_tickets(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Json<Vec<SupportTicket>>, ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    rate_limit::check_user(
        &state,
        &claims.sub,
        "support:list",
        30,
        std::time::Duration::from_secs(60),
    )
    .await?;
    let rows = sqlx::query(
        r#"
        SELECT id, subject, status, category, severity, created_at
        FROM support_tickets
        ORDER BY created_at DESC
        LIMIT 200
        "#,
    )
    .fetch_all(&state.db)
    .await?;
    let mut tickets = Vec::with_capacity(rows.len());
    for row in rows {
        let ticket_id = row.get("id");
        let messages = load_messages(&state, ticket_id).await?;
        tickets.push(row_to_ticket(row, messages));
    }
    Ok(Json(tickets))
}

pub async fn create_ticket(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<CreateTicketRequest>,
) -> Result<(StatusCode, Json<SupportTicket>), ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    rate_limit::check_user(
        &state,
        &claims.sub,
        "support:create",
        5,
        std::time::Duration::from_secs(60 * 60),
    )
    .await?;
    payload
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;

    let mut tx = state.db.begin().await?;
    let ticket_row = sqlx::query(
        r#"
        INSERT INTO support_tickets (id, subject, category, severity)
        VALUES ($1, $2, $3, $4)
        RETURNING id, subject, status, category, severity, created_at
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(payload.subject.trim())
    .bind(normalize_category(payload.category.as_deref()))
    .bind(normalize_severity(payload.severity.as_deref()))
    .fetch_one(&mut *tx)
    .await?;
    let ticket_id: Uuid = ticket_row.get("id");
    let message_row = sqlx::query(
        r#"
        INSERT INTO support_ticket_messages (id, ticket_id, body, author_type)
        VALUES ($1, $2, $3, 'user')
        RETURNING id, body, author_type, created_at
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(ticket_id)
    .bind(payload.body.trim())
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok((
        StatusCode::CREATED,
        Json(row_to_ticket(ticket_row, vec![row_to_message(message_row)])),
    ))
}

pub async fn get_ticket(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<SupportTicket>, ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    rate_limit::check_user(
        &state,
        &claims.sub,
        "support:get",
        60,
        std::time::Duration::from_secs(60),
    )
    .await?;
    let row = sqlx::query(
        r#"
        SELECT id, subject, status, category, severity, created_at
        FROM support_tickets
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    let messages = load_messages(&state, id).await?;
    Ok(Json(row_to_ticket(row, messages)))
}

pub async fn add_message(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<AddMessageRequest>,
) -> Result<(StatusCode, Json<SupportMessage>), ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    rate_limit::check_user(
        &state,
        &claims.sub,
        "support:message",
        20,
        std::time::Duration::from_secs(60 * 60),
    )
    .await?;
    payload
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;

    let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM support_tickets WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    if exists.is_none() {
        return Err(ApiError::NotFound);
    }

    let row = sqlx::query(
        r#"
        INSERT INTO support_ticket_messages (id, ticket_id, body, author_type)
        VALUES ($1, $2, $3, 'user')
        RETURNING id, body, author_type, created_at
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(id)
    .bind(payload.body.trim())
    .fetch_one(&state.db)
    .await?;
    sqlx::query("UPDATE support_tickets SET updated_at = now() WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    Ok((StatusCode::CREATED, Json(row_to_message(row))))
}

async fn load_messages(state: &AppState, ticket_id: Uuid) -> Result<Vec<SupportMessage>, ApiError> {
    let rows = sqlx::query(
        r#"
        SELECT id, body, author_type, created_at
        FROM support_ticket_messages
        WHERE ticket_id = $1
        ORDER BY created_at ASC
        "#,
    )
    .bind(ticket_id)
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(row_to_message).collect())
}

fn row_to_ticket(row: sqlx::postgres::PgRow, messages: Vec<SupportMessage>) -> SupportTicket {
    SupportTicket {
        id: row.get::<Uuid, _>("id").to_string(),
        subject: row.get("subject"),
        status: row.get("status"),
        category: row.get("category"),
        severity: row.get("severity"),
        created_at: row.get::<DateTime<Utc>, _>("created_at").to_rfc3339(),
        messages,
    }
}

fn row_to_message(row: sqlx::postgres::PgRow) -> SupportMessage {
    SupportMessage {
        id: row.get::<Uuid, _>("id").to_string(),
        body: row.get("body"),
        author_type: row.get("author_type"),
        created_at: row.get::<DateTime<Utc>, _>("created_at").to_rfc3339(),
    }
}

fn normalize_category(value: Option<&str>) -> &'static str {
    match value
        .unwrap_or("OTHER")
        .trim()
        .to_ascii_uppercase()
        .as_str()
    {
        "RESCUE" => "RESCUE",
        "APP" => "APP",
        "DONATION" => "DONATION",
        "SAFETY" => "SAFETY",
        _ => "OTHER",
    }
}

fn normalize_severity(value: Option<&str>) -> &'static str {
    match value
        .unwrap_or("MEDIUM")
        .trim()
        .to_ascii_uppercase()
        .as_str()
    {
        "LOW" => "LOW",
        "HIGH" => "HIGH",
        "URGENT" => "URGENT",
        _ => "MEDIUM",
    }
}
