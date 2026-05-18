use axum::{extract::Query, Json};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct NearbyQuery {
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub radius_km: Option<f64>,
}

#[derive(Serialize)]
pub struct NearbyCase {
    pub id: &'static str,
    pub distance_km: f64,
}

pub async fn nearby_cases(Query(query): Query<NearbyQuery>) -> Json<Vec<NearbyCase>> {
    let _radius = query.radius_km.unwrap_or(10.0);
    let _origin = (query.lat, query.lng);
    Json(vec![NearbyCase {
        id: "seed-1",
        distance_km: 1.8,
    }])
}
