use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct FeedItem {
    pub id: &'static str,
    pub kind: &'static str,
    pub title: &'static str,
    pub urgency: u8,
}

pub async fn list_feed() -> Json<Vec<FeedItem>> {
    Json(vec![FeedItem {
        id: "seed-1",
        kind: "emergency",
        title: "Caso urgente proximo",
        urgency: 90,
    }])
}
