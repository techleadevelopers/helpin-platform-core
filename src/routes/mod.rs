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
        .route("/readyz", get(health::readyz))
        .route("/v1/auth/login", post(auth::login))
        .route("/v1/auth/register", post(auth::register))
        .route("/v1/feed", get(feed::list_feed))
        .route("/v1/posts", post(posts::create_post))
        .route("/v1/posts/:id", get(posts::get_post))
        .route("/v1/posts/:id/like", post(posts::toggle_like))
        .route("/v1/chat/rooms", get(chat::list_rooms))
        .route("/v1/chat/rooms/:id", get(chat::get_room))
        .route(
            "/v1/chat/rooms/:id/messages",
            get(chat::list_messages).post(chat::send_message),
        )
        .route("/v1/geo/nearby", get(geo::nearby_cases))
        .route("/v1/ongs", get(ongs::list_ongs))
        .route("/v1/ongs/:id", get(ongs::get_ong))
        .route("/v1/ongs/:id/follow", post(ongs::follow_ong))
        .route("/v1/donations/intents", post(donations::create_intent))
        .route("/v1/trust/score/:subject_id", get(trust::score))
        .route("/v1/notifications", get(notifications::list_notifications))
        .route("/v1/search", get(search::search))
        .route("/v1/marketplace/items", get(marketplace::list_items))
        .route("/v1/ai/moderation-jobs", post(ai::enqueue_moderation_job))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{Method, Request, StatusCode},
    };
    use serde_json::{json, Value};
    use tower::ServiceExt;

    use super::*;
    use crate::{config::Config, state::AppState};

    async fn test_app() -> Router {
        let config = Config {
            bind_addr: "127.0.0.1:0".into(),
            database_url: "postgres://zoohelp:zoohelp@localhost:5432/zoohelp".into(),
            redis_url: "redis://localhost:6379".into(),
            nats_url: "nats://localhost:4222".into(),
            ai_worker_url: "http://127.0.0.1:8090".into(),
        };
        let state = AppState::new(config).await.expect("test state");
        router(state)
    }

    async fn request_json(app: Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = app.oneshot(request).await.expect("response");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        (status, value)
    }

    #[tokio::test]
    async fn feed_supports_frontend_filters() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/feed?type=emergency")
            .body(Body::empty())
            .unwrap();

        let (status, body) = request_json(app, request).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body[0]["type"], "emergency");
        assert_eq!(body[0]["animalType"], "cat");
    }

    #[tokio::test]
    async fn auth_register_returns_frontend_user_shape() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/auth/register")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "name": "ONG Teste",
                    "email": "ong@zoohelp.com",
                    "password": "senha-segura",
                    "accountType": "ong"
                })
                .to_string(),
            ))
            .unwrap();

        let (status, body) = request_json(app, request).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["user"]["type"], "ong");
        assert_eq!(body["tokenType"], "Bearer");
    }

    #[tokio::test]
    async fn create_post_rejects_empty_content() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/posts")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "postType": "adoption",
                    "animalType": "dog",
                    "description": "",
                    "location": "São Paulo, SP"
                })
                .to_string(),
            ))
            .unwrap();

        let (status, _body) = request_json(app, request).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn ong_detail_matches_frontend_profile_need() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/ongs/o1")
            .body(Body::empty())
            .unwrap();

        let (status, body) = request_json(app, request).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["id"], "o1");
        assert_eq!(body["verified"], true);
        assert!(body["animalsRescued"].as_u64().unwrap() > 0);
    }
}
