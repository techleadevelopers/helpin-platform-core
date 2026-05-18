use axum::{extract::Query, Json};
use serde::Deserialize;

use crate::{
    domain::{seed_posts, AccountType, Post, PostType},
    services::geo::haversine_km,
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

pub async fn list_feed(Query(query): Query<FeedQuery>) -> Json<Vec<Post>> {
    Json(rank_feed(query))
}

fn rank_feed(query: FeedQuery) -> Vec<Post> {
    let limit = query.limit.unwrap_or(30).clamp(1, 100);
    let origin = query.lat.zip(query.lng);
    let radius = query.radius_km.unwrap_or(80.0).clamp(1.0, 500.0);

    let mut scored: Vec<(f64, Post)> = seed_posts()
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
        let ranked = rank_feed(FeedQuery {
            post_type: None,
            author_type: None,
            urgent: None,
            lat: Some(-23.5614),
            lng: Some(-46.6559),
            radius_km: Some(20.0),
            limit: Some(10),
        });

        assert_eq!(ranked.first().map(|post| post.id.as_str()), Some("2"));
    }

    #[test]
    fn filters_by_radius() {
        let ranked = rank_feed(FeedQuery {
            post_type: None,
            author_type: None,
            urgent: None,
            lat: Some(-23.5505),
            lng: Some(-46.6333),
            radius_km: Some(5.0),
            limit: Some(100),
        });

        assert!(!ranked.iter().any(|post| post.location == "Campinas, SP"));
    }
}
