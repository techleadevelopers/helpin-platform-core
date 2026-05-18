use axum::{extract::Query, Json};
use serde::Deserialize;

use crate::domain::{seed_posts, AccountType, Post, PostType};

#[derive(Debug, Deserialize)]
pub struct FeedQuery {
    #[serde(rename = "type")]
    pub post_type: Option<PostType>,
    pub author_type: Option<AccountType>,
    pub urgent: Option<bool>,
    pub limit: Option<usize>,
}

pub async fn list_feed(Query(query): Query<FeedQuery>) -> Json<Vec<Post>> {
    let limit = query.limit.unwrap_or(30).clamp(1, 100);
    let posts = seed_posts()
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
        .take(limit)
        .collect();

    Json(posts)
}
