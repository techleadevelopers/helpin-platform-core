use axum::{extract::Query, Json};
use serde::{Deserialize, Serialize};

use crate::{
    domain::{seed_posts, Post},
    services::geo::haversine_km,
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

pub async fn nearby_cases(Query(query): Query<NearbyQuery>) -> Json<Vec<NearbyCase>> {
    let origin = (query.lat.unwrap_or(-23.5505), query.lng.unwrap_or(-46.6333));
    let radius = query.radius_km.unwrap_or(30.0).clamp(1.0, 500.0);

    let cases = seed_posts()
        .into_iter()
        .filter_map(|post| {
            let distance = haversine_km(origin.0, origin.1, post.latitude, post.longitude);
            (distance <= radius).then_some(NearbyCase {
                post,
                distance_km: (distance * 10.0).round() / 10.0,
            })
        })
        .collect();

    Json(cases)
}
