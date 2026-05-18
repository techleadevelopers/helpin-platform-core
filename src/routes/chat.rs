use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::StatusCode,
    response::Response,
    Json,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{
    domain::{seed_conversations, seed_messages, ChatConversation, ChatMessage},
    error::ApiError,
    state::{AppState, ChatEvent},
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
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<SendMessageRequest>,
) -> Result<(StatusCode, Json<SendMessageResponse>), ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    if !seed_conversations().iter().any(|room| room.id == id) {
        return Err(ApiError::NotFound);
    }

    let message = ChatMessage {
        id: Uuid::now_v7().to_string(),
        sender_id: "me".into(),
        body: payload.body,
        created_at: "agora".into(),
    };
    broadcast_chat_message(&state, &id, &message);

    Ok((StatusCode::CREATED, Json(SendMessageResponse { message })))
}

pub async fn room_ws(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    if !seed_conversations().iter().any(|room| room.id == id) {
        return Err(ApiError::NotFound);
    }

    Ok(ws.on_upgrade(move |socket| handle_socket(state, id, socket)))
}

async fn handle_socket(state: AppState, room_id: String, socket: WebSocket) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.chat_tx.subscribe();

    loop {
        tokio::select! {
            incoming = receiver.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let body = text.trim();
                        if body.is_empty() || body.len() > 2000 {
                            continue;
                        }
                        let message = ChatMessage {
                            id: Uuid::now_v7().to_string(),
                            sender_id: "me".into(),
                            body: body.to_string(),
                            created_at: "agora".into(),
                        };
                        broadcast_chat_message(&state, &room_id, &message);
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(payload))) => {
                        if sender.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        tracing::debug!(?error, %room_id, "websocket receive error");
                        break;
                    }
                }
            }
            event = rx.recv() => {
                match event {
                    Ok(event) if event.room_id == room_id => {
                        let Ok(payload) = serde_json::to_string(&event) else {
                            continue;
                        };
                        if sender.send(Message::Text(payload)).await.is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::debug!(?error, %room_id, "chat broadcast receive error");
                        break;
                    }
                }
            }
        }
    }
}

fn broadcast_chat_message(state: &AppState, room_id: &str, message: &ChatMessage) {
    let event = ChatEvent {
        room_id: room_id.into(),
        message_id: message.id.clone(),
        sender_id: message.sender_id.clone(),
        body: message.body.clone(),
        created_at: message.created_at.clone(),
    };
    let _ = state.chat_tx.send(event);
}
