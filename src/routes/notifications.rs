use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct NotificationItem {
    pub id: &'static str,
    pub title: &'static str,
    pub read: bool,
}

pub async fn list_notifications() -> Json<Vec<NotificationItem>> {
    Json(vec![NotificationItem {
        id: "notif-dev",
        title: "Novo caso perto de voce",
        read: false,
    }])
}
