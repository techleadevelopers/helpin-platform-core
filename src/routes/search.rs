use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    domain::{Ong, Post},
    error::ApiError,
    routes::{
        feed::{load_db_posts, FeedQuery},
        ongs::load_db_ongs,
    },
    state::AppState,
};

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
}

#[derive(Serialize)]
pub struct SearchResponse {
    pub posts: Vec<Post>,
    pub ongs: Vec<Ong>,
}

pub async fn search(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, ApiError> {
    let q = query.q.unwrap_or_default().trim().to_lowercase();

    let posts = load_db_posts(
        &state,
        &FeedQuery {
            post_type: None,
            author_type: None,
            urgent: None,
            lat: None,
            lng: None,
            radius_km: None,
            limit: Some(100),
            before: None,
        },
    )
    .await?
    .into_iter()
    .filter(|post| {
        q.is_empty()
            || post.name.to_lowercase().contains(&q)
            || post.description.to_lowercase().contains(&q)
            || post.neighborhood.to_lowercase().contains(&q)
            || post.tags.iter().any(|tag| tag.to_lowercase().contains(&q))
    })
    .collect();

    let ongs = load_db_ongs(&state)
        .await?
        .into_iter()
        .filter(|ong| {
            q.is_empty()
                || ong.name.to_lowercase().contains(&q)
                || ong.cause.to_lowercase().contains(&q)
                || ong.city.to_lowercase().contains(&q)
        })
        .collect();

    Ok(Json(SearchResponse { posts, ongs }))
}
