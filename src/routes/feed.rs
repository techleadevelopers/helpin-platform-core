use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::Response,
    Json,
};
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    domain::{AccountType, Author, Post, PostType, RescueOperationalSummary},
    error::ApiError,
    routes::posts::{animal_type_from_str, load_post_media, post_type_as_str, post_type_from_str},
    state::AppState,
};

#[cfg(test)]
use crate::services::geo::haversine_km;

#[derive(Debug, Deserialize)]
pub struct FeedQuery {
    #[serde(rename = "type")]
    pub post_type: Option<PostType>,
    pub author_type: Option<AccountType>,
    pub urgent: Option<bool>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub radius_km: Option<f64>,
    pub limit: Option<usize>,
    pub before: Option<DateTime<Utc>>,
}

pub async fn list_feed(
    State(state): State<AppState>,
    Query(query): Query<FeedQuery>,
) -> Result<Json<Vec<Post>>, ApiError> {
    Ok(Json(load_db_posts(&state, &query).await?))
}

pub async fn feed_ws(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| handle_feed_socket(state, socket))
}

#[cfg(test)]
fn rank_feed(query: FeedQuery, db_posts: Vec<Post>) -> Vec<Post> {
    let limit = query.limit.unwrap_or(30).clamp(1, 100);
    let origin = query.lat.zip(query.lng);
    let radius = query.radius_km.unwrap_or(80.0).clamp(1.0, 500.0);

    let mut scored: Vec<(f64, Post)> = db_posts
        .into_iter()
        .filter(|post| {
            query
                .post_type
                .as_ref()
                .map_or(true, |kind| post.post_type == *kind)
        })
        .filter(|post| {
            query
                .author_type
                .as_ref()
                .map_or(true, |kind| post.author.account_type == *kind)
        })
        .filter(|post| query.urgent.map_or(true, |urgent| post.urgent == urgent))
        .filter_map(|post| {
            let distance =
                origin.map(|(lat, lng)| haversine_km(lat, lng, post.latitude, post.longitude));
            if distance.map_or(false, |value| value > radius) {
                return None;
            }
            let score = feed_score(&post, distance);
            Some((score, post))
        })
        .collect();

    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored
        .into_iter()
        .map(|(_, post)| post)
        .take(limit)
        .collect()
}

pub(crate) async fn load_db_posts(
    state: &AppState,
    query: &FeedQuery,
) -> Result<Vec<Post>, sqlx::Error> {
    let limit = query.limit.unwrap_or(30).clamp(1, 100) as i64;
    let radius = query.radius_km.unwrap_or(80.0).clamp(1.0, 500.0) * 1000.0;
    let post_type = query.post_type.as_ref().map(post_type_as_str);
    let author_type = query.author_type.as_ref().map(account_type_as_str);

    let use_postgis = state.config.postgis_enabled && query.lat.is_some() && query.lng.is_some();
    let sql = if use_postgis {
        r#"
        SELECT
            p.id::text AS id,
            p.post_type::text AS post_type,
            p.animal_type,
            COALESCE(p.name, 'Publicacao') AS name,
            COALESCE(p.breed, '') AS breed,
            COALESCE(p.age, '') AS age,
            p.description,
            COALESCE(p.location_label, '') AS location,
            COALESCE(p.neighborhood, p.location_label, '') AS neighborhood,
            (
                SELECT pm.public_url
                FROM post_media pm
                WHERE pm.post_id = p.id
                ORDER BY pm.sort_order ASC, pm.created_at ASC
                LIMIT 1
            ) AS image,
            p.text_only,
            p.likes_count,
            p.comments_count,
            p.shares_count,
            p.urgent,
            p.rescue_status,
            p.resolved_at,
            p.created_at,
            p.contact,
            p.tags,
            COALESCE(p.latitude, -23.5505) AS latitude,
            COALESCE(p.longitude, -46.6333) AS longitude,
            fs.current_phase AS fanout_phase,
            COALESCE(fs.confirmed_count, 0) AS help_going_count,
            COALESCE(fs.arrived_count, 0) AS help_arrived_count,
            u.id::text AS author_id,
            u.name AS author_name,
            u.avatar_url AS author_avatar,
            u.verified AS author_verified,
            u.account_type::text AS author_type
        FROM posts p
        INNER JOIN users u ON u.id = p.author_id
        LEFT JOIN rescue_fanout_states fs ON fs.post_id = p.id
        WHERE p.moderation_status = 'approved'
          AND u.deleted_at IS NULL
          AND ($1::post_type IS NULL OR p.post_type = $1::post_type)
          AND ($2::account_type IS NULL OR u.account_type = $2::account_type)
          AND ($3::boolean IS NULL OR p.urgent = $3)
          AND ($4::timestamptz IS NULL OR p.created_at < $4)
          AND (
            $5::double precision IS NULL
            OR $6::double precision IS NULL
            OR p.geo IS NULL
            OR ST_DWithin(
              p.geo,
              ST_SetSRID(ST_MakePoint($6, $5), 4326)::geography,
              $7
            )
          )
        ORDER BY
          (
            CASE WHEN p.urgent AND p.rescue_status <> 'resolved' THEN 100 ELSE 0 END +
            CASE WHEN p.post_type = 'emergency' AND p.rescue_status <> 'resolved' THEN 40 ELSE 0 END +
            CASE WHEN p.rescue_status = 'active' THEN 30 ELSE 0 END +
            CASE WHEN u.verified THEN 12 ELSE 0 END +
            LEAST(COALESCE(u.trust_score, 0), 100) / 5.0 +
            LEAST(GREATEST(p.comments_count, 0), 30) * 0.6 +
            LEAST(GREATEST(p.likes_count, 0), 80) * 0.15 +
            CASE
              WHEN p.created_at > now() - interval '1 hour' THEN 16
              WHEN p.created_at > now() - interval '6 hours' THEN 8
              WHEN p.created_at > now() - interval '24 hours' THEN 3
              ELSE 0
            END
          ) DESC,
          CASE
            WHEN $5::double precision IS NULL OR $6::double precision IS NULL OR p.geo IS NULL THEN 25
            ELSE LEAST(
              ST_Distance(p.geo, ST_SetSRID(ST_MakePoint($6, $5), 4326)::geography) / 1000.0,
              50
            )
          END ASC,
          p.created_at DESC
        LIMIT $8
        "#
    } else {
        r#"
        SELECT
            p.id::text AS id,
            p.post_type::text AS post_type,
            p.animal_type,
            COALESCE(p.name, 'Publicacao') AS name,
            COALESCE(p.breed, '') AS breed,
            COALESCE(p.age, '') AS age,
            p.description,
            COALESCE(p.location_label, '') AS location,
            COALESCE(p.neighborhood, p.location_label, '') AS neighborhood,
            (
                SELECT pm.public_url
                FROM post_media pm
                WHERE pm.post_id = p.id
                ORDER BY pm.sort_order ASC, pm.created_at ASC
                LIMIT 1
            ) AS image,
            p.text_only,
            p.likes_count,
            p.comments_count,
            p.shares_count,
            p.urgent,
            p.rescue_status,
            p.resolved_at,
            p.created_at,
            p.contact,
            p.tags,
            COALESCE(p.latitude, -23.5505) AS latitude,
            COALESCE(p.longitude, -46.6333) AS longitude,
            fs.current_phase AS fanout_phase,
            COALESCE(fs.confirmed_count, 0) AS help_going_count,
            COALESCE(fs.arrived_count, 0) AS help_arrived_count,
            u.id::text AS author_id,
            u.name AS author_name,
            u.avatar_url AS author_avatar,
            u.verified AS author_verified,
            u.account_type::text AS author_type
        FROM posts p
        INNER JOIN users u ON u.id = p.author_id
        LEFT JOIN rescue_fanout_states fs ON fs.post_id = p.id
        WHERE p.moderation_status = 'approved'
          AND u.deleted_at IS NULL
          AND ($1::post_type IS NULL OR p.post_type = $1::post_type)
          AND ($2::account_type IS NULL OR u.account_type = $2::account_type)
          AND ($3::boolean IS NULL OR p.urgent = $3)
          AND ($4::timestamptz IS NULL OR p.created_at < $4)
          AND (
            $5::double precision IS NULL
            OR $6::double precision IS NULL
            OR (
              p.latitude BETWEEN $5 - ($7 / 111000.0) AND $5 + ($7 / 111000.0)
              AND p.longitude BETWEEN
                $6 - ($7 / (111000.0 * GREATEST(abs(cos(radians($5))), 0.2)))
                AND
                $6 + ($7 / (111000.0 * GREATEST(abs(cos(radians($5))), 0.2)))
            )
          )
        ORDER BY
          (
            CASE WHEN p.urgent AND p.rescue_status <> 'resolved' THEN 100 ELSE 0 END +
            CASE WHEN p.post_type = 'emergency' AND p.rescue_status <> 'resolved' THEN 40 ELSE 0 END +
            CASE WHEN p.rescue_status = 'active' THEN 30 ELSE 0 END +
            CASE WHEN u.verified THEN 12 ELSE 0 END +
            LEAST(COALESCE(u.trust_score, 0), 100) / 5.0 +
            LEAST(GREATEST(p.comments_count, 0), 30) * 0.6 +
            LEAST(GREATEST(p.likes_count, 0), 80) * 0.15 +
            CASE
              WHEN p.created_at > now() - interval '1 hour' THEN 16
              WHEN p.created_at > now() - interval '6 hours' THEN 8
              WHEN p.created_at > now() - interval '24 hours' THEN 3
              ELSE 0
            END
          ) DESC,
          CASE
            WHEN $5::double precision IS NULL OR $6::double precision IS NULL THEN 0
            ELSE ((p.latitude - $5) * (p.latitude - $5)) + ((p.longitude - $6) * (p.longitude - $6))
          END ASC,
          p.created_at DESC
        LIMIT $8
        "#
    };

    let rows = sqlx::query(sql)
        .bind(post_type)
        .bind(author_type)
        .bind(query.urgent)
        .bind(query.before)
        .bind(query.lat)
        .bind(query.lng)
        .bind(radius)
        .bind(limit)
        .fetch_all(&state.db)
        .await?;

    let mut posts: Vec<Post> = rows
        .into_iter()
        .map(|row| {
            let author_type = match row.get::<&str, _>("author_type") {
                "ong" => AccountType::Ong,
                "vet" => AccountType::Vet,
                _ => AccountType::Person,
            };
            Post {
                id: row.get("id"),
                post_type: post_type_from_str(row.get::<&str, _>("post_type")),
                animal_type: animal_type_from_str(row.get::<&str, _>("animal_type")),
                name: row.get("name"),
                breed: row.get("breed"),
                age: row.get("age"),
                description: row.get("description"),
                location: row.get("location"),
                neighborhood: row.get("neighborhood"),
                image: row.get("image"),
                images: Vec::new(),
                text_only: row.get("text_only"),
                author: Author {
                    id: row.get("author_id"),
                    name: row.get("author_name"),
                    avatar: row.get("author_avatar"),
                    verified: row.get("author_verified"),
                    account_type: author_type,
                },
                likes: row.get::<i32, _>("likes_count").max(0) as u32,
                comments: row.get::<i32, _>("comments_count").max(0) as u32,
                shares: row.get::<i32, _>("shares_count").max(0) as u32,
                urgent: row.get("urgent"),
                rescue_status: row.get("rescue_status"),
                resolved_at: row
                    .get::<Option<DateTime<Utc>>, _>("resolved_at")
                    .map(|value| value.to_rfc3339()),
                created_at: row.get::<DateTime<Utc>, _>("created_at").to_rfc3339(),
                contact: row.get("contact"),
                tags: row.get("tags"),
                latitude: row.get("latitude"),
                longitude: row.get("longitude"),
                rescue_operational: rescue_operational_from_row(&row),
            }
        })
        .collect();

    let post_ids: Vec<Uuid> = posts
        .iter()
        .filter_map(|post| Uuid::parse_str(&post.id).ok())
        .collect();
    let mut media_by_post = load_post_media(&state, &post_ids).await?;
    for post in &mut posts {
        post.images = media_by_post.remove(&post.id).unwrap_or_default();
        if post.image.is_none() {
            post.image = post.images.first().map(|image| image.url.clone());
        }
    }

    Ok(posts)
}

fn rescue_operational_from_row(row: &sqlx::postgres::PgRow) -> Option<RescueOperationalSummary> {
    let phase = row.try_get::<Option<i32>, _>("fanout_phase").ok().flatten();
    let going = row.try_get::<i32, _>("help_going_count").unwrap_or(0);
    let arrived = row.try_get::<i32, _>("help_arrived_count").unwrap_or(0);
    phase.map(|fanout_phase| RescueOperationalSummary {
        fanout_phase: Some(fanout_phase),
        help_going_count: going,
        help_arrived_count: arrived,
        operational_label: if arrived > 0 {
            "Ajuda no local".to_string()
        } else if going == 1 {
            "1 pessoa a caminho".to_string()
        } else if going > 1 {
            format!("{going} pessoas a caminho")
        } else if fanout_phase >= 9 {
            "Acionando apoio ambiental".to_string()
        } else if fanout_phase >= 6 {
            "Buscando apoio regional".to_string()
        } else {
            "Precisa de ajuda".to_string()
        },
    })
}

fn account_type_as_str(value: &AccountType) -> &'static str {
    match value {
        AccountType::Person => "person",
        AccountType::Ong => "ong",
        AccountType::Vet => "vet",
        AccountType::Admin => "admin",
    }
}

async fn handle_feed_socket(state: AppState, socket: WebSocket) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.feed_tx.subscribe();

    loop {
        tokio::select! {
            incoming = receiver.next() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(payload))) => {
                        if sender.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        tracing::debug!(?error, "feed websocket receive error");
                        break;
                    }
                }
            }
            event = rx.recv() => {
                match event {
                    Ok(event) => {
                        let Ok(payload) = serde_json::to_string(&event) else {
                            continue;
                        };
                        if sender.send(Message::Text(payload)).await.is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        tracing::debug!(?error, "feed broadcast receive error");
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
fn feed_score(post: &Post, distance_km: Option<f64>) -> f64 {
    let urgency = if post.urgent && post.rescue_status != "resolved" {
        1000.0
    } else {
        0.0
    };
    let kind_weight = match post.post_type {
        PostType::Emergency => 500.0,
        PostType::Lost => 350.0,
        PostType::Found => 250.0,
        PostType::Adoption => 180.0,
        PostType::Campaign => 120.0,
        PostType::Post => 40.0,
    };
    let trust = if post.author.verified { 75.0 } else { 0.0 };
    let engagement =
        (post.likes as f64 * 0.15) + (post.comments as f64 * 0.35) + (post.shares as f64 * 0.5);
    let proximity = distance_km.map_or(0.0, |distance| (3000.0 - distance * 150.0).max(-1500.0));

    urgency + kind_weight + trust + engagement + proximity
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::seed_posts;

    #[test]
    fn prioritizes_nearby_urgent_cases_for_logged_user_location() {
        let ranked = rank_feed(
            FeedQuery {
                post_type: None,
                author_type: None,
                urgent: None,
                lat: Some(-23.5614),
                lng: Some(-46.6559),
                radius_km: Some(20.0),
                limit: Some(10),
                before: None,
            },
            seed_posts(),
        );

        assert_eq!(ranked.first().map(|post| post.id.as_str()), Some("2"));
    }

    #[test]
    fn filters_by_radius() {
        let ranked = rank_feed(
            FeedQuery {
                post_type: None,
                author_type: None,
                urgent: None,
                lat: Some(-23.5505),
                lng: Some(-46.6333),
                radius_km: Some(5.0),
                limit: Some(100),
                before: None,
            },
            seed_posts(),
        );

        assert!(!ranked.iter().any(|post| post.location == "Campinas, SP"));
    }
}
