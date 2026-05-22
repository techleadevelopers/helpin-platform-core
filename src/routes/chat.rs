use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::{HeaderMap, StatusCode},
    response::Response,
    Json,
};
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;
use validator::Validate;

use crate::{
    domain::{AccountType, Author, ChatConversation, ChatMessage},
    error::ApiError,
    routes::auth::authenticate_request,
    services::auth as auth_service,
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

pub async fn list_rooms(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Json<Vec<ChatConversation>>, ApiError> {
    authenticate_request(&state, &headers)?;

    let rows = sqlx::query(
        r#"
        SELECT
          r.id,
          COALESCE(r.post_id::text, '') AS post_id,
          COALESCE(p.name, p.title, 'Caso ZooHelp') AS post_title,
          COALESCE(last_message.body, '') AS last_message,
          COALESCE(last_message.created_at, r.created_at) AS last_message_time,
          COALESCE(u.id, r.post_id)::text AS participant_id,
          COALESCE(u.name, 'ZooHelp') AS participant_name,
          u.avatar_url AS participant_avatar,
          COALESCE(u.verified, false) AS participant_verified,
          COALESCE(u.account_type::text, 'person') AS participant_type
        FROM chat_rooms r
        LEFT JOIN posts p ON p.id = r.post_id
        LEFT JOIN users u ON u.id = p.author_id
        LEFT JOIN LATERAL (
          SELECT body, created_at
          FROM chat_messages
          WHERE room_id = r.id
          ORDER BY created_at DESC
          LIMIT 1
        ) last_message ON true
        ORDER BY COALESCE(last_message.created_at, r.created_at) DESC
        LIMIT 100
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows.into_iter().map(row_to_conversation).collect()))
}

pub async fn get_room(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ChatConversation>, ApiError> {
    authenticate_request(&state, &headers)?;
    let room_id = parse_uuid(&id)?;
    load_room(&state, room_id).await.map(Json)
}

pub async fn list_messages(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<ChatMessage>>, ApiError> {
    authenticate_request(&state, &headers)?;
    let room_id = parse_uuid(&id)?;
    ensure_room_exists(&state, room_id).await?;

    let rows = sqlx::query(
        r#"
        SELECT id, sender_id, body, created_at
        FROM chat_messages
        WHERE room_id = $1
        ORDER BY created_at DESC
        LIMIT 100
        "#,
    )
    .bind(room_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows.into_iter().map(row_to_message).collect()))
}

pub async fn send_message(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<SendMessageRequest>,
) -> Result<(StatusCode, Json<SendMessageResponse>), ApiError> {
    payload
        .validate()
        .map_err(|e| ApiError::Validation(e.to_string()))?;

    let claims = authenticate_request(&state, &headers)?;
    let room_id = parse_uuid(&id)?;
    let sender_id = parse_claim_user_id(&claims)?;
    ensure_room_exists(&state, room_id).await?;

    let message = persist_chat_message(&state, room_id, sender_id, payload.body).await?;
    broadcast_chat_message(&state, &room_id.to_string(), &message);

    Ok((StatusCode::CREATED, Json(SendMessageResponse { message })))
}

pub async fn room_ws(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    let room_id = parse_uuid(&id)?;
    let sender_id = parse_claim_user_id(&claims)?;
    ensure_room_exists(&state, room_id).await?;

    Ok(ws.on_upgrade(move |socket| handle_socket(state, room_id, sender_id, socket)))
}

async fn handle_socket(state: AppState, room_id: Uuid, sender_user_id: Uuid, socket: WebSocket) {
    let room_id_str = room_id.to_string();
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
                        match persist_chat_message(&state, room_id, sender_user_id, body.to_string()).await {
                            Ok(message) => broadcast_chat_message(&state, &room_id_str, &message),
                            Err(error) => {
                                tracing::warn!(?error, %room_id, "websocket message was not persisted");
                                break;
                            }
                        }
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
                    Ok(event) if event.room_id == room_id_str => {
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

async fn load_room(state: &AppState, room_id: Uuid) -> Result<ChatConversation, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT
          r.id,
          COALESCE(r.post_id::text, '') AS post_id,
          COALESCE(p.name, p.title, 'Caso ZooHelp') AS post_title,
          COALESCE(last_message.body, '') AS last_message,
          COALESCE(last_message.created_at, r.created_at) AS last_message_time,
          COALESCE(u.id, r.post_id)::text AS participant_id,
          COALESCE(u.name, 'ZooHelp') AS participant_name,
          u.avatar_url AS participant_avatar,
          COALESCE(u.verified, false) AS participant_verified,
          COALESCE(u.account_type::text, 'person') AS participant_type
        FROM chat_rooms r
        LEFT JOIN posts p ON p.id = r.post_id
        LEFT JOIN users u ON u.id = p.author_id
        LEFT JOIN LATERAL (
          SELECT body, created_at
          FROM chat_messages
          WHERE room_id = r.id
          ORDER BY created_at DESC
          LIMIT 1
        ) last_message ON true
        WHERE r.id = $1
        "#,
    )
    .bind(room_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;

    Ok(row_to_conversation(row))
}

async fn ensure_room_exists(state: &AppState, room_id: Uuid) -> Result<(), ApiError> {
    let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM chat_rooms WHERE id = $1")
        .bind(room_id)
        .fetch_optional(&state.db)
        .await?;
    exists.map(|_| ()).ok_or(ApiError::NotFound)
}

async fn persist_chat_message(
    state: &AppState,
    room_id: Uuid,
    sender_id: Uuid,
    body: String,
) -> Result<ChatMessage, ApiError> {
    let row = sqlx::query(
        r#"
        INSERT INTO chat_messages (room_id, sender_id, body)
        VALUES ($1, $2, $3)
        RETURNING id, sender_id, body, created_at
        "#,
    )
    .bind(room_id)
    .bind(sender_id)
    .bind(body)
    .fetch_one(&state.db)
    .await?;

    Ok(row_to_message(row))
}

fn row_to_conversation(row: sqlx::postgres::PgRow) -> ChatConversation {
    ChatConversation {
        id: row.get::<Uuid, _>("id").to_string(),
        post_id: row.get("post_id"),
        participant: Author {
            id: row.get("participant_id"),
            name: row.get("participant_name"),
            avatar: row.get("participant_avatar"),
            verified: row.get("participant_verified"),
            account_type: account_type_from_str(row.get::<&str, _>("participant_type")),
        },
        last_message: row.get("last_message"),
        last_message_time: format_timestamp(row.get("last_message_time")),
        unread: 0,
        post_title: row.get("post_title"),
    }
}

fn row_to_message(row: sqlx::postgres::PgRow) -> ChatMessage {
    ChatMessage {
        id: row.get::<Uuid, _>("id").to_string(),
        sender_id: row.get::<Uuid, _>("sender_id").to_string(),
        body: row.get("body"),
        created_at: format_timestamp(row.get("created_at")),
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

fn parse_uuid(value: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(value).map_err(|_| ApiError::NotFound)
}

fn parse_claim_user_id(claims: &auth_service::AccessClaims) -> Result<Uuid, ApiError> {
    Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)
}

fn format_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339()
}

fn account_type_from_str(value: &str) -> AccountType {
    match value {
        "ong" => AccountType::Ong,
        "vet" => AccountType::Vet,
        "admin" => AccountType::Admin,
        _ => AccountType::Person,
    }
}
