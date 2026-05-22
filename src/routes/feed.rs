use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use sqlx::Row;

use crate::{
    domain::{seed_posts, AccountType, Author, Post, PostType},
    routes::posts::{animal_type_from_str, post_type_from_str},
    services::geo::haversine_km,
    state::AppState,
};

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
}

pub async fn list_feed(
    State(state): State<AppState>,
    Query(query): Query<FeedQuery>,
) -> Json<Vec<Post>> {
    let db_posts = load_db_posts(&state).await.unwrap_or_else(|error| {
        tracing::warn!(?error, "database feed unavailable; using seed feed only");
        Vec::new()
    });
    Json(rank_feed(query, db_posts))
}

fn rank_feed(query: FeedQuery, db_posts: Vec<Post>) -> Vec<Post> {
    let limit = query.limit.unwrap_or(30).clamp(1, 100);
    let origin = query.lat.zip(query.lng);
    let radius = query.radius_km.unwrap_or(80.0).clamp(1.0, 500.0);

    let mut posts = db_posts;
    posts.extend(seed_posts());

    let mut scored: Vec<(f64, Post)> = posts
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

async fn load_db_posts(state: &AppState) -> Result<Vec<Post>, sqlx::Error> {
    let rows = sqlx::query(
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
            p.contact,
            p.tags,
            COALESCE(p.latitude, -23.5505) AS latitude,
            COALESCE(p.longitude, -46.6333) AS longitude,
            u.id::text AS author_id,
            u.name AS author_name,
            u.avatar_url AS author_avatar,
            u.verified AS author_verified,
            u.account_type::text AS author_type
        FROM posts p
        INNER JOIN users u ON u.id = p.author_id
        WHERE p.moderation_status = 'approved'
        ORDER BY p.created_at DESC
        LIMIT 100
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    Ok(rows
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
                created_at: "agora".into(),
                contact: row.get("contact"),
                tags: row.get("tags"),
                latitude: row.get("latitude"),
                longitude: row.get("longitude"),
            }
        })
        .collect())
}

fn feed_score(post: &Post, distance_km: Option<f64>) -> f64 {
    let urgency = if post.urgent { 1000.0 } else { 0.0 };
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
            },
            Vec::new(),
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
            },
            Vec::new(),
        );

        assert!(!ranked.iter().any(|post| post.location == "Campinas, SP"));
    }
}
