use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct ChatRoom {
    pub id: &'static str,
    pub post_id: &'static str,
    pub unread: u32,
}

pub async fn list_rooms() -> Json<Vec<ChatRoom>> {
    Json(vec![ChatRoom {
        id: "room-dev",
        post_id: "seed-1",
        unread: 0,
    }])
}
