use axum::{extract::Query, Json};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
}

#[derive(Serialize)]
pub struct SearchResult {
    pub id: &'static str,
    pub kind: &'static str,
    pub title: String,
}

pub async fn search(Query(query): Query<SearchQuery>) -> Json<Vec<SearchResult>> {
    let title = query.q.unwrap_or_else(|| "ZooHelp".into());
    Json(vec![SearchResult {
        id: "search-dev",
        kind: "post",
        title,
    }])
}
