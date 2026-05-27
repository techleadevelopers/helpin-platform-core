use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct MarketplaceItem {
    pub id: &'static str,
    pub title: &'static str,
    pub item_type: &'static str,
}

pub async fn list_items() -> Json<Vec<MarketplaceItem>> {
    Json(vec![MarketplaceItem {
        id: "market-dev",
        title: "Ração para doação",
        item_type: "donation",
    }])
}
