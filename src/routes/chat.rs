use axum::{extract::Path, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{
    domain::{seed_conversations, seed_messages, ChatConversation, ChatMessage},
    error::ApiError,
};

#[derive(Debug, Deserialize, Validate)]
pub struct SendMessageRequest {
    #[validate(length(min = 1, max = 2000))]
    pub body: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageResponse {
    pub message: ChatMessage,
}

pub async fn list_rooms() -> Json<Vec<ChatConversation>> {
    Json(seed_conversations())
}

pub async fn get_room(Path(id): Path<String>) -> Result<Json<ChatConversation>, ApiError> {
    seed_conversations()
        .into_iter()
        .find(|room| room.id == id)
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn list_messages(Path(id): Path<String>) -> Result<Json<Vec<ChatMessage>>, ApiError> {
    if seed_conversations().iter().any(|room| room.id == id) {
        return Ok(Json(seed_messages(&id)));
    }

    Err(ApiError::NotFound)
}

pub async fn send_message(
    Path(id): Path<String>,
    Json(payload): Json<SendMessageRequest>,
) -> Result<(StatusCode, Json<SendMessageResponse>), ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    if !seed_conversations().iter().any(|room| room.id == id) {
        return Err(ApiError::NotFound);
    }

    Ok((
        StatusCode::CREATED,
        Json(SendMessageResponse {
            message: ChatMessage {
                id: uuid::Uuid::now_v7().to_string(),
                sender_id: "me".into(),
                body: payload.body,
                created_at: "agora".into(),
            },
        }),
    ))
}
