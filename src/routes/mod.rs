use axum::{
    routing::{get, post},
    Router,
};

use crate::state::AppState;

mod ai;
mod auth;
mod chat;
mod donations;
mod feed;
mod geo;
mod health;
mod marketplace;
mod notifications;
mod ongs;
mod posts;
mod search;
mod trust;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health::healthz))
        .route("/v1/auth/login", post(auth::login))
        .route("/v1/auth/register", post(auth::register))
        .route("/v1/feed", get(feed::list_feed))
        .route("/v1/posts", post(posts::create_post))
        .route("/v1/posts/:id", get(posts::get_post))
        .route("/v1/chat/rooms", get(chat::list_rooms))
        .route("/v1/geo/nearby", get(geo::nearby_cases))
        .route("/v1/ongs", get(ongs::list_ongs))
        .route("/v1/donations/intents", post(donations::create_intent))
        .route("/v1/trust/score/:subject_id", get(trust::score))
        .route("/v1/notifications", get(notifications::list_notifications))
        .route("/v1/search", get(search::search))
        .route("/v1/marketplace/items", get(marketplace::list_items))
        .route("/v1/ai/moderation-jobs", post(ai::enqueue_moderation_job))
        .with_state(state)
}
