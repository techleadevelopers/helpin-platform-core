use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    domain::Post,
    error::ApiError,
    routes::feed::{load_db_posts, FeedQuery},
    services::geo::haversine_km,
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
    State(state): State<AppState>,
    Query(query): Query<NearbyQuery>,
) -> Result<Json<Vec<NearbyCase>>, ApiError> {
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
        },
        None,
    )
    .await?;

    let cases = posts
        .into_iter()
        .filter_map(|post| {
            let distance = haversine_km(origin.0, origin.1, post.latitude, post.longitude);
            (distance <= radius).then_some(NearbyCase {
                post,
                distance_km: (distance * 10.0).round() / 10.0,
            })
        })
        .collect();

    Ok(Json(cases))
}
