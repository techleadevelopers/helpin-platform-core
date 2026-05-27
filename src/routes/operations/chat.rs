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
use sha1::{Digest, Sha1};
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
    pub ticket: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListRoomsQuery {
    pub post_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListMessagesQuery {
    pub before: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRoomRequest {
    pub post_id: Option<String>,
    pub participant_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadRoomRequest {
    pub through_message_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WsTicketResponse {
    pub ticket: String,
    pub expires_at: String,
}

#[derive(Serialize)]
pub struct ChatActionResponse {
    pub status: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatRealtimePayload {
    room_id: String,
    message_id: String,
    sender_id: String,
    body: String,
    created_at: String,
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
          COALESCE(p.name, p.title, 'Conversa direta') AS post_title,
          COALESCE(last_message.body, '') AS last_message,
          COALESCE(last_message.created_at, r.created_at) AS last_message_time,
          participant_user.id::text AS participant_id,
          COALESCE(participant_user.name, 'Helpin') AS participant_name,
          participant_user.avatar_url AS participant_avatar,
          COALESCE(participant_user.verified, false) AS participant_verified,
          COALESCE(participant_user.account_type::text, 'person') AS participant_type,
          COALESCE(unread_messages.count, 0)::bigint AS unread
        FROM chat_rooms r
        LEFT JOIN posts p ON p.id = r.post_id
        INNER JOIN chat_room_members me ON me.room_id = r.id AND me.user_id = $2
        INNER JOIN chat_room_members peer ON peer.room_id = r.id AND peer.user_id <> $2
        INNER JOIN users participant_user ON participant_user.id = peer.user_id
        LEFT JOIN LATERAL (
          SELECT body, created_at
          FROM chat_messages
          WHERE room_id = r.id
          ORDER BY created_at DESC
          LIMIT 1
        ) last_message ON true
        LEFT JOIN LATERAL (
          SELECT count(*) AS count
          FROM chat_messages
          WHERE room_id = r.id
            AND sender_id <> $2
            AND created_at > COALESCE(me.last_read_at, '-infinity'::timestamptz)
        ) unread_messages ON true
        WHERE ($1::uuid IS NULL OR r.post_id = $1)
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

pub async fn open_room(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<OpenRoomRequest>,
) -> Result<(StatusCode, Json<ChatConversation>), ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    let requester_id = parse_claim_user_id(&claims)?;
    rate_limit::check_key(
        &state,
        &format!("chat:room:create:{requester_id}"),
        state.config.throttle_limit,
        StdDuration::from_secs(state.config.throttle_ttl_seconds),
    )
    .await?;
    let mut tx = state.db.begin().await?;
    let (room_id, participant_id) = match (
        payload.post_id.as_deref(),
        payload.participant_id.as_deref(),
    ) {
        (Some(post_id), None) => {
            let post_id = parse_uuid(post_id)?;
            let author_id: Uuid = sqlx::query_scalar("SELECT author_id FROM posts WHERE id = $1")
                .bind(post_id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(ApiError::NotFound)?;

            if requester_id == author_id {
                return Err(ApiError::Validation(
                    "cannot open a chat with your own post".into(),
                ));
            }
            ensure_contact_allowed(&state, requester_id, author_id).await?;

            let room_id: Uuid = sqlx::query_scalar(
                r#"
                INSERT INTO chat_rooms (post_id, requester_id)
                VALUES ($1, $2)
                ON CONFLICT (post_id, requester_id)
                  WHERE post_id IS NOT NULL AND requester_id IS NOT NULL
                DO UPDATE SET requester_id = EXCLUDED.requester_id
                RETURNING id
                "#,
            )
            .bind(post_id)
            .bind(requester_id)
            .fetch_one(&mut *tx)
            .await?;
            (room_id, author_id)
        }
        (None, Some(participant_id)) => {
            let participant_id = parse_uuid(participant_id)?;
            if requester_id == participant_id {
                return Err(ApiError::Validation(
                    "cannot open a chat with yourself".into(),
                ));
            }

            let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE id = $1")
                .bind(participant_id)
                .fetch_optional(&mut *tx)
                .await?;
            if exists.is_none() {
                return Err(ApiError::NotFound);
            }
            ensure_contact_allowed(&state, requester_id, participant_id).await?;

            let mut members = [requester_id.to_string(), participant_id.to_string()];
            members.sort();
            let direct_pair_key = members.join(":");
            let room_id: Uuid = sqlx::query_scalar(
                r#"
                INSERT INTO chat_rooms (direct_pair_key)
                VALUES ($1)
                ON CONFLICT (direct_pair_key)
                  WHERE direct_pair_key IS NOT NULL
                DO UPDATE SET direct_pair_key = EXCLUDED.direct_pair_key
                RETURNING id
                "#,
            )
            .bind(direct_pair_key)
            .fetch_one(&mut *tx)
            .await?;
            (room_id, participant_id)
        }
        _ => {
            return Err(ApiError::Validation(
                "provide exactly one of postId or participantId".into(),
            ));
        }
    };

    sqlx::query(
        r#"
        INSERT INTO chat_room_members (room_id, user_id)
        VALUES ($1, $2), ($1, $3)
        ON CONFLICT (room_id, user_id) DO NOTHING
        "#,
    )
    .bind(room_id)
    .bind(requester_id)
    .bind(participant_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let room = load_room(&state, room_id, requester_id).await?;
    Ok((StatusCode::CREATED, Json(room)))
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
    Query(query): Query<ListMessagesQuery>,
) -> Result<Json<Vec<ChatMessage>>, ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    let current_user_id = parse_claim_user_id(&claims)?;
    let room_id = parse_uuid(&id)?;
    ensure_room_member(&state, room_id, current_user_id).await?;
    let before_id = query.before.as_deref().map(parse_uuid).transpose()?;
    let limit = query.limit.unwrap_or(50).clamp(1, 100);

    let rows = sqlx::query(
        r#"
        SELECT id, sender_id, body, created_at
        FROM chat_messages
        WHERE room_id = $1
          AND (
            $2::uuid IS NULL OR (created_at, id) < (
              SELECT created_at, id
              FROM chat_messages
              WHERE room_id = $1 AND id = $2
            )
          )
        ORDER BY created_at DESC
        LIMIT $3
        "#,
    )
    .bind(room_id)
    .bind(before_id)
    .bind(limit)
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
    ensure_room_member(&state, room_id, sender_id).await?;
    ensure_room_send_allowed(&state, room_id, sender_id).await?;
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|header| header.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            ApiError::Validation("idempotency-key is required for chat messages".into())
        })?;
    if idempotency_key.len() > 160 {
        return Err(ApiError::Validation("idempotency-key is too long".into()));
    }

    let (message, inserted) = persist_chat_message(
        &state,
        room_id,
        sender_id,
        payload.body,
        Some(&idempotency_key),
    )
    .await?;
    if inserted {
        broadcast_chat_message(&state, &room_id.to_string(), &message).await;
    }

    Ok((
        if inserted {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(SendMessageResponse { message }),
    ))
}

pub async fn mark_read(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(payload): Json<ReadRoomRequest>,
) -> Result<Json<ChatActionResponse>, ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    let user_id = parse_claim_user_id(&claims)?;
    let room_id = parse_uuid(&id)?;
    let through_message_id = parse_uuid(&payload.through_message_id)?;
    ensure_room_member(&state, room_id, user_id).await?;

    let read_at: DateTime<Utc> =
        sqlx::query_scalar("SELECT created_at FROM chat_messages WHERE id = $1 AND room_id = $2")
            .bind(through_message_id)
            .bind(room_id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(ApiError::NotFound)?;

    mark_room_read(&state, room_id, user_id, read_at).await?;
    Ok(Json(ChatActionResponse { status: "read" }))
}

pub async fn create_ws_ticket(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<WsTicketResponse>), ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    let user_id = parse_claim_user_id(&claims)?;
    let room_id = parse_uuid(&id)?;
    ensure_room_member(&state, room_id, user_id).await?;

    let ticket = format!("{}.{}", Uuid::now_v7(), Uuid::now_v7());
    let token_hash = hash_ticket(&ticket);
    let expires_at = Utc::now() + chrono::Duration::seconds(45);
    sqlx::query("DELETE FROM chat_ws_tickets WHERE expires_at < now() OR consumed_at IS NOT NULL")
        .execute(&state.db)
        .await?;
    sqlx::query(
        "INSERT INTO chat_ws_tickets (token_hash, room_id, user_id, expires_at) VALUES ($1, $2, $3, $4)",
    )
    .bind(token_hash)
    .bind(room_id)
    .bind(user_id)
    .bind(expires_at)
    .execute(&state.db)
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(WsTicketResponse {
            ticket,
            expires_at: expires_at.to_rfc3339(),
        }),
    ))
}

pub async fn room_ws(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsAuthQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let room_id = parse_uuid(&id)?;
    let sender_id = consume_ws_ticket(&state, room_id, query.ticket.as_deref()).await?;
    ensure_room_member(&state, room_id, sender_id).await?;

    Ok(ws.on_upgrade(move |socket| handle_socket(state, room_id, sender_id, socket)))
}

async fn handle_socket(state: AppState, room_id: Uuid, sender_user_id: Uuid, socket: WebSocket) {
    let room_id_str = room_id.to_string();
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.subscribe_chat_room(&room_id_str).await;

    loop {
        tokio::select! {
            incoming = receiver.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        tracing::debug!(%room_id, %sender_user_id, frame_length = text.len(), "websocket write frame ignored; REST message endpoint is required");
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
                    Ok(event) => {
                        if ensure_room_send_allowed(&state, room_id, sender_user_id).await.is_err() {
                            continue;
                        }
                        let Ok(Some(message)) = load_chat_message(&state, room_id, &event.message_id).await else {
                            continue;
                        };
                        let payload = ChatRealtimePayload {
                            room_id: room_id_str.clone(),
                            message_id: message.id,
                            sender_id: message.sender_id,
                            body: message.body,
                            created_at: message.created_at,
                        };
                        let Ok(payload) = serde_json::to_string(&payload) else {
                            continue;
                        };
                        if sender.send(Message::Text(payload)).await.is_err() {
                            break;
                        }
                    }
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
          COALESCE(p.name, p.title, 'Conversa direta') AS post_title,
          COALESCE(last_message.body, '') AS last_message,
          COALESCE(last_message.created_at, r.created_at) AS last_message_time,
          participant_user.id::text AS participant_id,
          COALESCE(participant_user.name, 'Helpin') AS participant_name,
          participant_user.avatar_url AS participant_avatar,
          COALESCE(participant_user.verified, false) AS participant_verified,
          COALESCE(participant_user.account_type::text, 'person') AS participant_type,
          COALESCE(unread_messages.count, 0)::bigint AS unread
        FROM chat_rooms r
        LEFT JOIN posts p ON p.id = r.post_id
        INNER JOIN chat_room_members me ON me.room_id = r.id AND me.user_id = $2
        INNER JOIN chat_room_members peer ON peer.room_id = r.id AND peer.user_id <> $2
        INNER JOIN users participant_user ON participant_user.id = peer.user_id
        LEFT JOIN LATERAL (
          SELECT body, created_at
          FROM chat_messages
          WHERE room_id = r.id
          ORDER BY created_at DESC
          LIMIT 1
        ) last_message ON true
        LEFT JOIN LATERAL (
          SELECT count(*) AS count
          FROM chat_messages
          WHERE room_id = r.id
            AND sender_id <> $2
            AND created_at > COALESCE(me.last_read_at, '-infinity'::timestamptz)
        ) unread_messages ON true
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

async fn ensure_room_member(
    state: &AppState,
    room_id: Uuid,
    user_id: Uuid,
) -> Result<(), ApiError> {
    let exists: Option<Uuid> = sqlx::query_scalar(
        "SELECT room_id FROM chat_room_members WHERE room_id = $1 AND user_id = $2",
    )
    .bind(room_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;
    exists.map(|_| ()).ok_or(ApiError::Forbidden)
}

async fn mark_room_read(
    state: &AppState,
    room_id: Uuid,
    user_id: Uuid,
    read_at: DateTime<Utc>,
) -> Result<(), ApiError> {
    sqlx::query(
        r#"
        UPDATE chat_room_members
        SET last_read_at = GREATEST(COALESCE(last_read_at, '-infinity'::timestamptz), $3)
        WHERE room_id = $1 AND user_id = $2
        "#,
    )
    .bind(room_id)
    .bind(user_id)
    .bind(read_at)
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn persist_chat_message(
    state: &AppState,
    room_id: Uuid,
    sender_id: Uuid,
    body: String,
    idempotency_key: Option<&str>,
) -> Result<(ChatMessage, bool), ApiError> {
    if let Some(idempotency_key) = idempotency_key {
        let row = sqlx::query(
            r#"
            WITH inserted AS (
              INSERT INTO chat_messages (room_id, sender_id, body, idempotency_key)
              VALUES ($1, $2, $3, $4)
              ON CONFLICT (room_id, sender_id, idempotency_key)
                WHERE idempotency_key IS NOT NULL
              DO NOTHING
              RETURNING id, sender_id, body, created_at, true AS inserted
            )
            SELECT id, sender_id, body, created_at, inserted FROM inserted
            UNION ALL
            SELECT id, sender_id, body, created_at, false AS inserted
            FROM chat_messages
            WHERE room_id = $1 AND sender_id = $2 AND idempotency_key = $4
            LIMIT 1
            "#,
        )
        .bind(room_id)
        .bind(sender_id)
        .bind(body)
        .bind(idempotency_key)
        .fetch_one(&state.db)
        .await?;
        let inserted: bool = row.get("inserted");
        return Ok((row_to_message(row), inserted));
    }

    let row = sqlx::query(
        "INSERT INTO chat_messages (room_id, sender_id, body) VALUES ($1, $2, $3) RETURNING id, sender_id, body, created_at",
    )
    .bind(room_id)
    .bind(sender_id)
    .bind(body)
    .fetch_one(&state.db)
    .await?;

    Ok((row_to_message(row), true))
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
        unread: row.get::<i64, _>("unread").try_into().unwrap_or(u32::MAX),
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

async fn broadcast_chat_message(state: &AppState, room_id: &str, message: &ChatMessage) {
    let event = ChatEvent {
        room_id: room_id.into(),
        message_id: message.id.clone(),
    };
    state.deliver_chat_event(event.clone()).await;
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

async fn consume_ws_ticket(
    state: &AppState,
    room_id: Uuid,
    ticket: Option<&str>,
) -> Result<Uuid, ApiError> {
    let ticket = ticket
        .filter(|value| !value.trim().is_empty())
        .ok_or(ApiError::Unauthorized)?;
    let token_hash = hash_ticket(ticket);
    sqlx::query_scalar(
        r#"
        UPDATE chat_ws_tickets
        SET consumed_at = now()
        WHERE token_hash = $1
          AND room_id = $2
          AND consumed_at IS NULL
          AND expires_at > now()
        RETURNING user_id
        "#,
    )
    .bind(token_hash)
    .bind(room_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::Unauthorized)
}

async fn load_chat_message(
    state: &AppState,
    room_id: Uuid,
    message_id: &str,
) -> Result<Option<ChatMessage>, ApiError> {
    let message_id = parse_uuid(message_id)?;
    let row = sqlx::query(
        "SELECT id, sender_id, body, created_at FROM chat_messages WHERE id = $1 AND room_id = $2",
    )
    .bind(message_id)
    .bind(room_id)
    .fetch_optional(&state.db)
    .await?;
    Ok(row.map(row_to_message))
}

async fn ensure_contact_allowed(
    state: &AppState,
    user_id: Uuid,
    participant_id: Uuid,
) -> Result<(), ApiError> {
    let blocked: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
          SELECT 1 FROM chat_user_blocks
          WHERE (blocker_id = $1 AND blocked_id = $2)
             OR (blocker_id = $2 AND blocked_id = $1)
        )
        "#,
    )
    .bind(user_id)
    .bind(participant_id)
    .fetch_one(&state.db)
    .await?;
    if blocked {
        Err(ApiError::Forbidden)
    } else {
        Ok(())
    }
}

async fn ensure_room_send_allowed(
    state: &AppState,
    room_id: Uuid,
    user_id: Uuid,
) -> Result<(), ApiError> {
    let participant_id: Uuid = sqlx::query_scalar(
        "SELECT user_id FROM chat_room_members WHERE room_id = $1 AND user_id <> $2 LIMIT 1",
    )
    .bind(room_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::Forbidden)?;
    ensure_contact_allowed(state, user_id, participant_id).await
}

pub async fn block_participant(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ChatActionResponse>, ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    let blocker_id = parse_claim_user_id(&claims)?;
    let blocked_id = parse_uuid(&id)?;
    if blocker_id == blocked_id {
        return Err(ApiError::Validation("cannot block yourself".into()));
    }
    sqlx::query(
        "INSERT INTO chat_user_blocks (blocker_id, blocked_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(blocker_id)
    .bind(blocked_id)
    .execute(&state.db)
    .await?;
    Ok(Json(ChatActionResponse { status: "blocked" }))
}

pub async fn unblock_participant(
    headers: HeaderMap,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ChatActionResponse>, ApiError> {
    let claims = authenticate_request(&state, &headers)?;
    let blocker_id = parse_claim_user_id(&claims)?;
    let blocked_id = parse_uuid(&id)?;
    sqlx::query("DELETE FROM chat_user_blocks WHERE blocker_id = $1 AND blocked_id = $2")
        .bind(blocker_id)
        .bind(blocked_id)
        .execute(&state.db)
        .await?;
    Ok(Json(ChatActionResponse {
        status: "unblocked",
    }))
}

fn hash_ticket(ticket: &str) -> String {
    format!("{:x}", Sha1::digest(ticket.as_bytes()))
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
