use axum::{
    extract::{Query, State},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    domain::Post,
    error::ApiError,
    routes::{
        feed::{load_db_posts, FeedQuery},
        posts::optional_authenticated_user_id,
    },
    services::{geo::haversine_km, rate_limit},
    state::AppState,
};

#[derive(Deserialize)]
pub struct NearbyQuery {
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub radius_km: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NearbyCase {
    pub post: Post,
    pub distance_km: f64,
}

pub async fn nearby_cases(
    headers: HeaderMap,
    State(state): State<AppState>,
    Query(query): Query<NearbyQuery>,
) -> Result<Json<Vec<NearbyCase>>, ApiError> {
    let window = std::time::Duration::from_secs(60);
    if let Some(user_id) = optional_authenticated_user_id(&state, &headers) {
        rate_limit::check_user(&state, &user_id.to_string(), "nearby", 30, window).await?;
    } else {
        rate_limit::check_ip(&state, &headers, "nearby", 20, window).await?;
    }
    let origin = query
        .lat
        .zip(query.lng)
        .ok_or_else(|| ApiError::Validation("lat and lng are required".into()))?;
    let radius = query.radius_km.unwrap_or(30.0).clamp(1.0, 500.0);

    let posts = load_db_posts(
        &state,
        &FeedQuery {
            post_type: None,
            author_type: None,
            urgent: None,
            lat: Some(origin.0),
            lng: Some(origin.1),
            radius_km: Some(radius),
            limit: Some(100),
            before: None,
            liked: None,
            author_id: None,
        },
        None,
    )
    .await?;

    let cases = posts
        .into_iter()
        .filter_map(|post| {
            let (lat, lng) = post.latitude.zip(post.longitude)?;
            let distance = haversine_km(origin.0, origin.1, lat, lng);
            (distance <= radius).then_some(NearbyCase {
                post,
                distance_km: (distance * 10.0).round() / 10.0,
            })
        })
        .collect();

    Ok(Json(cases))
}
