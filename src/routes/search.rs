use axum::{extract::Query, Json};
use serde::{Deserialize, Serialize};

use crate::domain::{seed_ongs, seed_posts, Ong, Post};

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
}

#[derive(Serialize)]
pub struct SearchResponse {
    pub posts: Vec<Post>,
    pub ongs: Vec<Ong>,
}

pub async fn search(Query(query): Query<SearchQuery>) -> Json<SearchResponse> {
    let q = query.q.unwrap_or_default().to_lowercase();

    let posts = seed_posts()
        .into_iter()
        .filter(|post| {
            q.is_empty()
                || post.name.to_lowercase().contains(&q)
                || post.description.to_lowercase().contains(&q)
                || post.neighborhood.to_lowercase().contains(&q)
        })
        .collect();

    let ongs = seed_ongs()
        .into_iter()
        .filter(|ong| {
            q.is_empty()
                || ong.name.to_lowercase().contains(&q)
                || ong.cause.to_lowercase().contains(&q)
                || ong.city.to_lowercase().contains(&q)
        })
        .collect();

    Json(SearchResponse { posts, ongs })
}
