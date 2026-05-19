use std::{collections::HashMap, sync::LazyLock};

use axum::{extract::Path, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use validator::Validate;

use crate::error::ApiError;

static TICKETS: LazyLock<Mutex<HashMap<String, SupportTicket>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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

pub async fn list_tickets() -> Json<Vec<SupportTicket>> {
    let tickets = TICKETS
        .lock()
        .expect("support tickets lock")
        .values()
        .cloned()
        .collect();
    Json(tickets)
}

pub async fn create_ticket(
    Json(payload): Json<CreateTicketRequest>,
) -> Result<(StatusCode, Json<SupportTicket>), ApiError> {
    payload
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;
    let now = chrono::Utc::now().to_rfc3339();
    let first_message = SupportMessage {
        id: uuid::Uuid::now_v7().to_string(),
        body: payload.body,
        author_type: "user".to_string(),
        created_at: now.clone(),
    };
    let ticket = SupportTicket {
        id: uuid::Uuid::now_v7().to_string(),
        subject: payload.subject,
        status: "open".to_string(),
        category: payload.category.unwrap_or_else(|| "OTHER".to_string()),
        severity: payload.severity.unwrap_or_else(|| "MEDIUM".to_string()),
        created_at: now,
        messages: vec![first_message],
    };
    TICKETS
        .lock()
        .map_err(|_| ApiError::Internal)?
        .insert(ticket.id.clone(), ticket.clone());
    Ok((StatusCode::CREATED, Json(ticket)))
}

pub async fn get_ticket(Path(id): Path<String>) -> Result<Json<SupportTicket>, ApiError> {
    TICKETS
        .lock()
        .map_err(|_| ApiError::Internal)?
        .get(&id)
        .cloned()
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn add_message(
    Path(id): Path<String>,
    Json(payload): Json<AddMessageRequest>,
) -> Result<(StatusCode, Json<SupportMessage>), ApiError> {
    payload
        .validate()
        .map_err(|error| ApiError::Validation(error.to_string()))?;
    let mut tickets = TICKETS.lock().map_err(|_| ApiError::Internal)?;
    let ticket = tickets.get_mut(&id).ok_or(ApiError::NotFound)?;
    let message = SupportMessage {
        id: uuid::Uuid::now_v7().to_string(),
        body: payload.body,
        author_type: "user".to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    ticket.messages.push(message.clone());
    Ok((StatusCode::CREATED, Json(message)))
}
