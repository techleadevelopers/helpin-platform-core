use axum::{
    extract::{Path, Query},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    domain::{seed_ongs, Ong},
    error::ApiError,
};

#[derive(Debug, Deserialize)]
pub struct OngQuery {
    pub q: Option<String>,
    pub city: Option<String>,
    pub verified: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowOngResponse {
    pub ong_id: String,
    pub following: bool,
}

pub async fn list_ongs(Query(query): Query<OngQuery>) -> Json<Vec<Ong>> {
    let q = query.q.map(|value| value.to_lowercase());
    let city = query.city.map(|value| value.to_lowercase());

    let ongs = seed_ongs()
        .into_iter()
        .filter(|ong| {
            q.as_ref().map_or(true, |q| {
                ong.name.to_lowercase().contains(q)
                    || ong.cause.to_lowercase().contains(q)
                    || ong.city.to_lowercase().contains(q)
            })
        })
        .filter(|ong| {
            city.as_ref()
                .map_or(true, |city| ong.city.to_lowercase() == *city)
        })
        .filter(|ong| {
            query
                .verified
                .map_or(true, |verified| ong.verified == verified)
        })
        .collect();

    Json(ongs)
}

pub async fn get_ong(Path(id): Path<String>) -> Result<Json<Ong>, ApiError> {
    seed_ongs()
        .into_iter()
        .find(|ong| ong.id == id)
        .map(Json)
        .ok_or(ApiError::NotFound)
}

pub async fn follow_ong(Path(id): Path<String>) -> Result<Json<FollowOngResponse>, ApiError> {
    if seed_ongs().iter().any(|ong| ong.id == id) {
        return Ok(Json(FollowOngResponse {
            ong_id: id,
            following: true,
        }));
    }

    Err(ApiError::NotFound)
}
