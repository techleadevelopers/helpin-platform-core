use std::time::Duration as StdDuration;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
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
    services::{auth as auth_service, rate_limit},
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

#[derive(Debug, Deserialize)]
pub struct WsAuthQuery {
    pub access_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListRoomsQuery {
    pub post_id: Option<String>,
}

pub async fn list_rooms(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<ListRoomsQuery>,
) -> Result<Json<Vec<ChatConversation>>, ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    let current_user_id = parse_claim_user_id(&claims)?;
    let post_id = query.post_id.as_deref().map(parse_uuid).transpose()?;

    let rows = sqlx::query(
        r#"
        SELECT
          r.id,
          COALESCE(r.post_id::text, '') AS post_id,
          COALESCE(p.name, p.title, 'Caso ZooHelp') AS post_title,
          COALESCE(last_message.body, '') AS last_message,
          COALESCE(last_message.created_at, r.created_at) AS last_message_time,
          COALESCE(participant_user.id, r.post_id)::text AS participant_id,
          COALESCE(participant_user.name, 'ZooHelp') AS participant_name,
          participant_user.avatar_url AS participant_avatar,
          COALESCE(participant_user.verified, false) AS participant_verified,
          COALESCE(participant_user.account_type::text, 'person') AS participant_type
        FROM chat_rooms r
        LEFT JOIN posts p ON p.id = r.post_id
        LEFT JOIN LATERAL (
          SELECT body, created_at
          FROM chat_messages
          WHERE room_id = r.id
          ORDER BY created_at DESC
          LIMIT 1
        ) last_message ON true
        LEFT JOIN LATERAL (
          SELECT sender_id
          FROM chat_messages
          WHERE room_id = r.id AND sender_id <> $2
          ORDER BY created_at DESC
          LIMIT 1
        ) other_sender ON true
        LEFT JOIN users participant_user ON participant_user.id = CASE
          WHEN p.author_id = $2 THEN other_sender.sender_id
          ELSE p.author_id
        END
        WHERE (
          ($1::uuid IS NOT NULL AND r.post_id = $1)
          OR ($1::uuid IS NULL AND (
            p.author_id = $2
            OR EXISTS (
              SELECT 1 FROM chat_messages mine
              WHERE mine.room_id = r.id AND mine.sender_id = $2
            )
          ))
        )
        ORDER BY COALESCE(last_message.created_at, r.created_at) DESC
        LIMIT 100
        "#,
    )
    .bind(post_id)
    .bind(current_user_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(rows.into_iter().map(row_to_conversation).collect()))
}

pub async fn get_room(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ChatConversation>, ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    let current_user_id = parse_claim_user_id(&claims)?;
    let room_id = parse_uuid(&id)?;
    load_room(&state, room_id, current_user_id).await.map(Json)
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
    rate_limit::check_key(
        &state,
        &format!("chat:message:{sender_id}"),
        state.config.throttle_limit * 2,
        StdDuration::from_secs(state.config.throttle_ttl_seconds),
    )
    .await?;
    ensure_room_exists(&state, room_id).await?;

    let message = persist_chat_message(&state, room_id, sender_id, payload.body).await?;
    broadcast_chat_message(&state, &room_id.to_string(), &message);

    Ok((StatusCode::CREATED, Json(SendMessageResponse { message })))
}

pub async fn room_ws(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsAuthQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let claims = authenticate_socket(&state, &headers, query.access_token.as_deref())?;
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
                        if rate_limit::check_key(
                            &state,
                            &format!("chat:ws:{sender_user_id}"),
                            state.config.throttle_limit * 2,
                            StdDuration::from_secs(state.config.throttle_ttl_seconds),
                        )
                        .await
                        .is_err()
                        {
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

async fn load_room(
    state: &AppState,
    room_id: Uuid,
    current_user_id: Uuid,
) -> Result<ChatConversation, ApiError> {
    let row = sqlx::query(
        r#"
        SELECT
          r.id,
          COALESCE(r.post_id::text, '') AS post_id,
          COALESCE(p.name, p.title, 'Caso ZooHelp') AS post_title,
          COALESCE(last_message.body, '') AS last_message,
          COALESCE(last_message.created_at, r.created_at) AS last_message_time,
          COALESCE(participant_user.id, r.post_id)::text AS participant_id,
          COALESCE(participant_user.name, 'ZooHelp') AS participant_name,
          participant_user.avatar_url AS participant_avatar,
          COALESCE(participant_user.verified, false) AS participant_verified,
          COALESCE(participant_user.account_type::text, 'person') AS participant_type
        FROM chat_rooms r
        LEFT JOIN posts p ON p.id = r.post_id
        LEFT JOIN LATERAL (
          SELECT body, created_at
          FROM chat_messages
          WHERE room_id = r.id
          ORDER BY created_at DESC
          LIMIT 1
        ) last_message ON true
        LEFT JOIN LATERAL (
          SELECT sender_id
          FROM chat_messages
          WHERE room_id = r.id AND sender_id <> $2
          ORDER BY created_at DESC
          LIMIT 1
        ) other_sender ON true
        LEFT JOIN users participant_user ON participant_user.id = CASE
          WHEN p.author_id = $2 THEN other_sender.sender_id
          ELSE p.author_id
        END
        WHERE r.id = $1
        "#,
    )
    .bind(room_id)
    .bind(current_user_id)
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
    let _ = state.chat_tx.send(event.clone());
    let bus = state.event_bus.clone();
    tokio::spawn(async move {
        bus.publish_chat(&event).await;
    });
}

fn parse_uuid(value: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(value).map_err(|_| ApiError::NotFound)
}

fn parse_claim_user_id(claims: &auth_service::AccessClaims) -> Result<Uuid, ApiError> {
    Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized)
}

fn authenticate_socket(
    state: &AppState,
    headers: &HeaderMap,
    query_token: Option<&str>,
) -> Result<auth_service::AccessClaims, ApiError> {
    if headers.get(axum::http::header::AUTHORIZATION).is_some() {
        return authenticate_request(state, headers);
    }

    let token = query_token
        .filter(|value| !value.trim().is_empty())
        .ok_or(ApiError::Unauthorized)?;

    auth_service::verify_access_token(&state.config, token).map_err(|_| ApiError::Unauthorized)
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
