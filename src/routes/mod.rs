use axum::{
    routing::{get, patch, post},
    Router,
};

use crate::state::AppState;

mod admin;
mod ai;
mod auth;
mod chat;
mod donations;
mod feed;
mod geo;
mod health;
mod maps;
mod marketplace;
mod media;
mod notifications;
mod observability;
mod ongs;
mod posts;
mod rescue;
mod search;
mod support;
mod trust;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz))
        .route("/metrics", get(observability::metrics))
        .route("/v1/observability", get(observability::status))
        .route("/v1/auth/login", post(auth::login))
        .route("/v1/auth/register", post(auth::register))
        .route("/v1/auth/email/verify", get(auth::verify_email))
        .route(
            "/v1/auth/password-reset",
            post(auth::request_password_reset),
        )
        .route(
            "/v1/admin/ongs/pending-verification",
            get(admin::pending_ongs),
        )
        .route("/v1/admin/users", get(admin::list_users))
        .route(
            "/v1/admin/users/:id",
            get(admin::get_user)
                .patch(admin::update_user)
                .delete(admin::delete_user),
        )
        .route(
            "/v1/admin/ongs/:id/verification-status",
            patch(admin::update_ong_verification_status),
        )
        .route(
            "/v1/admin/ongs/:id/kyb-documents",
            get(admin::list_kyb_documents).post(admin::create_kyb_document),
        )
        .route(
            "/v1/admin/kyb-documents/:id/review",
            patch(admin::review_kyb_document),
        )
        .route(
            "/v1/admin/moderation/jobs",
            get(admin::list_moderation_jobs),
        )
        .route(
            "/v1/admin/moderation/jobs/:id",
            patch(admin::review_moderation_job),
        )
        .route("/v1/admin/reports/posts", get(admin::list_post_reports))
        .route("/v1/me", get(auth::me).delete(auth::delete_account))
        .route("/v1/me/avatar", patch(auth::update_avatar))
        .route("/v1/feed", get(feed::list_feed))
        .route("/v1/posts", post(posts::create_post))
        .route("/v1/posts/:id", get(posts::get_post))
        .route("/v1/posts/:id/like", post(posts::toggle_like))
        .route("/v1/posts/:id/comments", post(posts::create_comment))
        .route("/v1/posts/:id/report", post(posts::report_post))
        .route(
            "/v1/media/upload-intents",
            post(media::create_upload_intent),
        )
        .route("/v1/chat/rooms", get(chat::list_rooms))
        .route("/v1/chat/rooms/:id", get(chat::get_room))
        .route(
            "/v1/chat/rooms/:id/messages",
            get(chat::list_messages).post(chat::send_message),
        )
        .route("/v1/chat/rooms/:id/ws", get(chat::room_ws))
        .route("/v1/geo/nearby", get(geo::nearby_cases))
        .route("/v1/maps/static-url", get(maps::static_map_url))
        .route("/v1/ongs", get(ongs::list_ongs))
        .route("/v1/ongs/:id", get(ongs::get_ong))
        .route("/v1/ongs/:id/follow", post(ongs::follow_ong))
        .route("/v1/donations/intents", post(donations::create_intent))
        .route(
            "/v1/donations/webhooks/:provider",
            post(donations::payment_webhook),
        )
        .route("/v1/trust/score/:subject_id", get(trust::score))
        .route("/v1/notifications", get(notifications::list_notifications))
        .route(
            "/v1/notifications/:id/mark-as-read",
            axum::routing::patch(notifications::mark_as_read),
        )
        .route("/v1/notifications/:id/ack", post(notifications::ack))
        .route(
            "/v1/notifications/push-token",
            post(notifications::register_push_token),
        )
        .route(
            "/v1/notifications/rescue-alerts/:post_id/preview",
            post(notifications::preview_rescue_alert),
        )
        .route(
            "/v1/rescue/active",
            get(rescue::list_active).post(rescue::trigger),
        )
        .route(
            "/v1/rescue/active/:id/location",
            axum::routing::patch(rescue::update_location),
        )
        .route(
            "/v1/rescue/active/:id/end",
            axum::routing::patch(rescue::end),
        )
        .route("/v1/rescue/active/:id/incident", post(rescue::incident))
        .route("/v1/rescue/active/:id/ws", get(rescue::rescue_ws))
        .route("/v1/support/meta", get(support::meta))
        .route(
            "/v1/support/tickets",
            get(support::list_tickets).post(support::create_ticket),
        )
        .route("/v1/support/tickets/:id", get(support::get_ticket))
        .route(
            "/v1/support/tickets/:id/messages",
            post(support::add_message),
        )
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
    use crate::{
        config::Config, domain::AccountType, services::auth as auth_service, state::AppState,
    };

    fn test_config() -> Config {
        Config {
            app_env: "test".into(),
            bind_addr: "127.0.0.1:0".into(),
            database_url: "postgres://zoohelp:zoohelp@localhost:5432/zoohelp".into(),
            database_max_connections: 5,
            database_min_connections: 0,
            redis_url: "redis://localhost:6379".into(),
            nats_url: "nats://localhost:4222".into(),
            ai_worker_url: "http://127.0.0.1:8090".into(),
            jwt_secret: "test-secret".into(),
            cloudinary_cloud_name: "limpeja".into(),
            cloudinary_api_key: Some("test-api-key".into()),
            cloudinary_api_secret: Some("test-api-secret".into()),
            geocoding_api_provider: Some("google".into()),
            google_maps_api_key: Some("test-google-key".into()),
            api_public_url: "https://api.zoohelp.test".into(),
            app_public_url: "https://zoohelp.test".into(),
            smtp_host: None,
            smtp_port: 587,
            smtp_user: None,
            smtp_pass: None,
            smtp_secure: true,
            smtp_from_email: "no-reply@zoohelp.test".into(),
            smtp_from_name: "ZooHelp".into(),
            access_token_ttl_minutes: 15,
            refresh_token_ttl_days: 30,
            cors_allowed_origins: Vec::new(),
            postgis_enabled: false,
            payments_enabled: false,
            payment_provider: "test".into(),
            payment_webhook_secret: Some("test-webhook-secret-123456".into()),
            sentry_dsn: None,
            otel_exporter_otlp_endpoint: None,
            push_worker_enabled: false,
            push_provider: "expo".into(),
            expo_access_token: None,
        }
    }

    fn test_auth_header(account_type: AccountType) -> String {
        let token = auth_service::issue_access_token(
            &test_config(),
            "018f0000-0000-7000-8000-000000000001",
            "admin@zoohelp.test",
            account_type,
        )
        .expect("test token");
        format!("Bearer {token}")
    }

    async fn test_app() -> Router {
        let config = test_config();
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
    async fn static_map_url_uses_google_maps_config() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/maps/static-url?lat=-23.5505&lng=-46.6333&zoom=14")
            .body(Body::empty())
            .unwrap();

        let (status, value) = request_json(app, request).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["provider"], "google");
        assert!(value["imageUrl"]
            .as_str()
            .expect("image url")
            .contains("maps.googleapis.com/maps/api/staticmap"));
        assert!(value["imageUrl"]
            .as_str()
            .expect("image url")
            .contains("key=test-google-key"));
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
                    "accountType": "ong",
                    "ongType": "rescue",
                    "cnpj": "12.345.678/0001-90",
                    "phone": "(11) 99999-0001",
                    "city": "Sao Paulo",
                    "state": "SP"
                })
                .to_string(),
            ))
            .unwrap();

        let (status, body) = request_json(app, request).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["user"]["type"], "ong");
        assert_eq!(body["ongProfile"]["ongType"], "rescue");
        assert_eq!(body["ongProfile"]["phone"], "(11) 99999-0001");
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
    async fn create_post_accepts_frontend_media_contract() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/posts")
            .header("content-type", "application/json")
            .header("authorization", test_auth_header(AccountType::Ong))
            .body(Body::from(
                json!({
                    "name": "Mel",
                    "postType": "adoption",
                    "animalType": "dog",
                    "description": "Animal vacinado para adocao responsavel.",
                    "location": "Sao Paulo, SP",
                    "neighborhood": "Vila Mariana",
                    "images": [{
                        "objectKey": "posts/test/mel.webp",
                        "publicUrl": "https://cdn.zoohelp.local/posts/test/mel.webp",
                        "contentType": "image/webp",
                        "width": 1080,
                        "height": 1080,
                        "sizeBytes": 320000
                    }]
                })
                .to_string(),
            ))
            .unwrap();

        let (status, body) = request_json(app, request).await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(
            body["post"]["image"],
            "https://cdn.zoohelp.local/posts/test/mel.webp"
        );
        assert_eq!(body["post"]["images"][0]["contentType"], "image/webp");
        assert_eq!(body["media"][0]["moderationStatus"], "approved");
    }

    #[tokio::test]
    async fn emergency_post_requires_real_coordinates() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/posts")
            .header("content-type", "application/json")
            .header("authorization", test_auth_header(AccountType::Ong))
            .body(Body::from(
                json!({
                    "name": "Pedido de ajuda",
                    "postType": "emergency",
                    "animalType": "other",
                    "description": "Animal ferido precisa de ajuda agora.",
                    "location": "Localizacao atual",
                    "urgent": true
                })
                .to_string(),
            ))
            .unwrap();

        let (status, _body) = request_json(app, request).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn emergency_post_dispatches_geo_rescue_alert() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/posts")
            .header("content-type", "application/json")
            .header("authorization", test_auth_header(AccountType::Ong))
            .body(Body::from(
                json!({
                    "name": "Pedido de ajuda",
                    "postType": "emergency",
                    "animalType": "other",
                    "description": "Animal ferido precisa de ajuda agora.",
                    "location": "Localizacao atual",
                    "neighborhood": "Localizacao atual",
                    "urgent": true,
                    "latitude": -23.5505,
                    "longitude": -46.6333
                })
                .to_string(),
            ))
            .unwrap();

        let (status, body) = request_json(app, request).await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["post"]["latitude"], -23.5505);
        assert_eq!(body["post"]["longitude"], -46.6333);
        assert_eq!(body["rescueAlert"]["critical"], true);
        assert_eq!(body["rescueAlert"]["radiusKm"], 0.03);
    }

    #[tokio::test]
    async fn media_upload_intent_validates_image_contract() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/media/upload-intents")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "fileName": "resgate.webp",
                    "contentType": "image/webp",
                    "sizeBytes": 420000
                })
                .to_string(),
            ))
            .unwrap();

        let (status, body) = request_json(app, request).await;

        assert_eq!(status, StatusCode::CREATED);
        assert!(body["objectKey"]
            .as_str()
            .unwrap()
            .starts_with("zoohelp/posts/image/"));
        assert!(body["uploadUrl"]
            .as_str()
            .unwrap()
            .contains("api.cloudinary.com"));
        assert_eq!(body["provider"], "cloudinary");
        assert_eq!(body["cloudinary"]["cloudName"], "limpeja");
        assert_eq!(body["cloudinary"]["apiKey"], "test-api-key");
        assert!(body["cloudinary"]["signature"].as_str().unwrap().len() >= 40);
        assert_eq!(body["maxSizeBytes"], 10 * 1024 * 1024);
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
