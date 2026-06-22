use axum::{
    routing::{get, patch, post},
    Router,
};

use crate::state::AppState;

mod admin;
#[path = "operations/ai.rs"]
mod ai;
#[path = "identity/auth.rs"]
mod auth;
#[path = "operations/chat.rs"]
mod chat;
#[path = "commerce/donations.rs"]
mod donations;
#[path = "content/feed.rs"]
mod feed;
#[path = "geo/nearby.rs"]
mod geo;
#[path = "platform/health.rs"]
mod health;
#[path = "platform/impact.rs"]
mod impact;
#[path = "geo/maps.rs"]
pub(crate) mod maps;
#[path = "content/marketplace.rs"]
mod marketplace;
#[path = "content/media.rs"]
mod media;
#[path = "operations/notifications.rs"]
mod notifications;
#[path = "platform/observability.rs"]
mod observability;
#[path = "operations/ongs.rs"]
mod ongs;
#[path = "content/posts.rs"]
mod posts;
#[path = "operations/rescue.rs"]
mod rescue;
#[path = "content/search.rs"]
mod search;
#[path = "platform/support.rs"]
mod support;
#[path = "identity/trust.rs"]
mod trust;
#[path = "identity/users.rs"]
mod users;
#[path = "operations/volunteers.rs"]
mod volunteers;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(health::healthz))
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz))
        .route("/metrics", get(observability::metrics))
        .route("/v1/observability", get(observability::status))
        .route("/v1/impact/metrics", get(impact::metrics))
        .route("/v1/auth/login", post(auth::login))
        .route("/v1/auth/register", post(auth::register))
        .route("/v1/auth/email/verify", get(auth::verify_email))
        .route(
            "/v1/auth/password-reset",
            post(auth::request_password_reset),
        )
        .route(
            "/v1/auth/password-reset/confirm",
            post(auth::confirm_password_reset),
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
        .route(
            "/v1/admin/reports/rescue-final",
            get(admin::list_rescue_final_reports),
        )
        .route("/v1/admin/queues/status", get(admin::queue_status))
        .route("/v1/admin/queues/:queue_name/jobs", get(admin::queue_jobs))
        .route(
            "/v1/admin/queues/:queue_name/jobs/:job_id/retry",
            post(admin::retry_queue_job),
        )
        .route("/admin/queues/status", get(admin::queue_status))
        .route("/admin/queues/:queue_name/jobs", get(admin::queue_jobs))
        .route(
            "/admin/queues/:queue_name/jobs/:job_id/retry",
            post(admin::retry_queue_job),
        )
        .route(
            "/v1/me",
            get(auth::me)
                .patch(auth::update_profile)
                .delete(auth::delete_account),
        )
        .route("/v1/me/avatar", patch(auth::update_avatar))
        .route(
            "/v1/me/volunteer-profile",
            get(volunteers::get_my_profile).put(volunteers::upsert_my_profile),
        )
        .route(
            "/v1/me/ong/kyb-documents",
            post(admin::create_my_kyb_document),
        )
        .route("/v1/feed", get(feed::list_feed))
        .route("/v1/feed/ws", get(feed::feed_ws))
        .route("/v1/posts", post(posts::create_post))
        .route(
            "/v1/posts/:id",
            get(posts::get_post).delete(posts::delete_post),
        )
        .route(
            "/v1/posts/:id/like",
            post(posts::like_post)
                .put(posts::like_post)
                .delete(posts::unlike_post),
        )
        .route("/v1/posts/:id/share", post(posts::share_post))
        .route(
            "/v1/posts/:id/comments",
            get(posts::list_comments).post(posts::create_comment),
        )
        .route("/v1/posts/:id/report", post(posts::report_post))
        .route("/v1/users", get(users::list_public_users))
        .route(
            "/v1/users/:id/followers",
            get(users::list_public_user_followers),
        )
        .route(
            "/v1/users/:id/following",
            get(users::list_public_user_following),
        )
        .route("/v1/users/:id", get(users::get_public_user_profile))
        .route(
            "/v1/users/:id/follow",
            post(users::follow_user)
                .put(users::follow_user)
                .delete(users::unfollow_user),
        )
        .route(
            "/v1/posts/:id/rescue-response",
            post(posts::rescue_response),
        )
        .route(
            "/v1/media/upload-intents",
            post(media::create_upload_intent),
        )
        .route(
            "/v1/chat/rooms",
            get(chat::list_rooms).post(chat::open_room),
        )
        .route("/v1/chat/rooms/:id", get(chat::get_room))
        .route(
            "/v1/chat/rooms/:id/messages",
            get(chat::list_messages).post(chat::send_message),
        )
        .route("/v1/chat/rooms/:id/read", patch(chat::mark_read))
        .route("/v1/chat/rooms/:id/ws-ticket", post(chat::create_ws_ticket))
        .route("/v1/chat/rooms/:id/ws", get(chat::room_ws))
        .route(
            "/v1/chat/participants/:id/block",
            axum::routing::put(chat::block_participant).delete(chat::unblock_participant),
        )
        .route("/v1/geo/nearby", get(geo::nearby_cases))
        .route("/v1/maps/static-url", get(maps::static_map_url))
        .route("/v1/maps/geocode", get(maps::geocode))
        .route("/v1/maps/place-autocomplete", get(maps::place_autocomplete))
        .route("/v1/maps/place-details", get(maps::place_details))
        .route("/v1/ongs", get(ongs::list_ongs))
        .route("/v1/ongs/:id", get(ongs::get_ong))
        .route("/v1/ongs/:id/follow", post(ongs::follow_ong))
        .route("/v1/donations/intents", post(donations::create_intent))
        .route(
            "/v1/contributions/maintenance/intents",
            post(donations::create_maintenance_intent),
        )
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
        .route("/v1/rescue/active/:id/responses", post(rescue::respond))
        .route("/v1/rescue/active/:id/ws", get(rescue::rescue_ws))
        .route("/v1/rescue/:id/brief", post(ai::rescue_brief))
        .route(
            "/v1/rescue/:id/final-report/generate",
            post(rescue::generate_final_report),
        )
        .route("/v1/rescue/:id/final-report", get(rescue::get_final_report))
        .route(
            "/v1/rescue/:id/final-report/approve",
            post(rescue::approve_final_report),
        )
        .route(
            "/v1/rescue/:id/final-report/reject",
            post(rescue::reject_final_report),
        )
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
        .route("/v1/ai/post-assessment", post(ai::assess_post))
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
    use std::{env, sync::OnceLock};
    use tokio::sync::Mutex;
    use tower::ServiceExt;

    use super::*;
    use crate::{
        config::{Config, ProcessRole},
        domain::AccountType,
        services::auth as auth_service,
        state::AppState,
    };

    const TEST_USER_ID: &str = "018f0000-0000-7000-8000-000000000001";
    const TEST_SECOND_USER_ID: &str = "018f0000-0000-7000-8000-000000000002";
    const TEST_ONG_ID: &str = "018f0000-0000-7000-8000-000000000101";
    const TEST_FEED_POST_ID: &str = "018f0000-0000-7000-8000-000000000201";
    const TEST_MEDIA_INTENT_ID: &str = "018f0000-0000-7000-8000-000000000301";
    const TEST_RESCUE_ID: &str = "018f0000-0000-7000-8000-000000000401";
    const TEST_MEDIA_OBJECT_KEY: &str = "posts/test/mel.webp";
    const TEST_MEDIA_PUBLIC_URL: &str = "https://cdn.zoohelp.local/posts/test/mel.webp";

    fn test_config() -> Config {
        dotenvy::dotenv().ok();

        Config {
            app_env: "test".into(),
            process_role: ProcessRole::All,
            bind_addr: "127.0.0.1:0".into(),
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgres://zoohelp:zoohelp@localhost:5432/zoohelp".into()),
            database_max_connections: env::var("DATABASE_MAX_CONNECTIONS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(5),
            database_min_connections: env::var("DATABASE_MIN_CONNECTIONS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
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
            rescue_fanout_worker_enabled: false,
            push_provider: "expo".into(),
            expo_access_token: None,
            log_push_tokens: false,
            throttle_ttl_seconds: 60,
            throttle_limit: 10,
        }
    }

    fn test_auth_header(account_type: AccountType) -> String {
        test_auth_header_for(TEST_USER_ID, "admin@zoohelp.test", account_type)
    }

    fn test_auth_header_for(user_id: &str, email: &str, account_type: AccountType) -> String {
        let token = auth_service::issue_access_token(&test_config(), user_id, email, account_type)
            .expect("test token");
        format!("Bearer {token}")
    }

    async fn test_app() -> Router {
        static STATE_INIT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = STATE_INIT_LOCK.get_or_init(|| Mutex::new(())).lock().await;
        let config = test_config();
        let state = AppState::new(config).await.expect("test state");
        seed_test_fixtures(&state).await.expect("test fixtures");
        router(state)
    }

    async fn seed_test_fixtures(state: &AppState) -> anyhow::Result<()> {
        let test_user_id = uuid::Uuid::parse_str(TEST_USER_ID)?;
        let test_second_user_id = uuid::Uuid::parse_str(TEST_SECOND_USER_ID)?;
        let test_post_id = uuid::Uuid::parse_str(TEST_FEED_POST_ID)?;

        sqlx::query(
            "DELETE FROM user_follows WHERE follower_id IN ($1, $2) OR followed_id IN ($1, $2)",
        )
        .bind(test_user_id)
        .bind(test_second_user_id)
        .execute(&state.db)
        .await?;
        sqlx::query("DELETE FROM post_likes WHERE post_id = $1 AND user_id IN ($2, $3)")
            .bind(test_post_id)
            .bind(test_user_id)
            .bind(test_second_user_id)
            .execute(&state.db)
            .await?;
        sqlx::query("DELETE FROM post_comments WHERE post_id = $1 AND user_id IN ($2, $3)")
            .bind(test_post_id)
            .bind(test_user_id)
            .bind(test_second_user_id)
            .execute(&state.db)
            .await?;
        sqlx::query("DELETE FROM support_tickets WHERE user_id IN ($1, $2)")
            .bind(test_user_id)
            .bind(test_second_user_id)
            .execute(&state.db)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO users (
              id, name, email, avatar_url, password_hash, account_type, verified,
              trust_score, gender, cep, street, number, complement, neighborhood, city, state
            )
            VALUES (
              $1, 'Instituto Teste ZooHelp', 'admin@zoohelp.test', NULL, 'test-hash',
              'ong', true, 80, NULL, '01001000', 'Rua Teste', '100', NULL,
              'Vila Mariana', 'Sao Paulo', 'SP'
            )
            ON CONFLICT (id) DO UPDATE SET
              name = EXCLUDED.name,
              email = EXCLUDED.email,
              account_type = EXCLUDED.account_type,
              verified = EXCLUDED.verified,
              deleted_at = NULL
            "#,
        )
        .bind(uuid::Uuid::parse_str(TEST_USER_ID)?)
        .execute(&state.db)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO users (
              id, name, email, avatar_url, password_hash, account_type, verified,
              trust_score, gender, cep, street, number, complement, neighborhood, city, state
            )
            VALUES (
              $1, 'Joao Teste ZooHelp', 'joao@zoohelp.test', 'https://cdn.zoohelp.local/users/joao.webp', 'test-hash',
              'person', false, 20, NULL, NULL, NULL, NULL, NULL,
              'Centro', 'Sao Paulo', 'SP'
            )
            ON CONFLICT (id) DO UPDATE SET
              name = EXCLUDED.name,
              email = EXCLUDED.email,
              account_type = EXCLUDED.account_type,
              verified = EXCLUDED.verified,
              avatar_url = EXCLUDED.avatar_url,
              deleted_at = NULL
            "#,
        )
        .bind(test_second_user_id)
        .execute(&state.db)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO ong_profiles (
              id, user_id, legal_name, cnpj, mission, city, state, area_type,
              contact_phone, cep, street, number, complement, neighborhood,
              foundation_year, verification_status
            )
            VALUES (
              $1, $2, 'Instituto Teste ZooHelp', '12345678000191',
              'Resgate e protecao animal.', 'Sao Paulo', 'SP', 'rescue',
              '(11) 99999-0001', '01001000', 'Rua Teste', '100', NULL,
              'Vila Mariana', 2016, 'APPROVED'
            )
            ON CONFLICT (id) DO UPDATE SET
              user_id = EXCLUDED.user_id,
              legal_name = EXCLUDED.legal_name,
              verification_status = EXCLUDED.verification_status,
              contact_phone = EXCLUDED.contact_phone
            "#,
        )
        .bind(uuid::Uuid::parse_str(TEST_ONG_ID)?)
        .bind(uuid::Uuid::parse_str(TEST_USER_ID)?)
        .execute(&state.db)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO posts (
              id, author_id, post_type, animal_type, name, breed, age, title,
              description, latitude, longitude, location_label, neighborhood, contact,
              tags, urgent, rescue_status, text_only, likes_count, comments_count,
              shares_count, moderation_status, fraud_risk, geo_status, geo_source,
              geo_provider, geo_confidence, geo_resolved_at, route_public
            )
            VALUES (
              $1, $2, 'emergency', 'cat', 'Sem nome', 'Gatinho tigrado',
              'Estimado 3 meses', NULL,
              'Gatinho encontrado ferido na Av. Paulista.',
              -23.5614, -46.6559, 'Sao Paulo, SP', 'Bela Vista',
              '(11) 99999-0002', ARRAY['emergencia', 'ferido'], true, 'active',
              false, 340, 67, 210, 'approved', 0, 'confirmed',
              'gps_confirmed', 'test', 1.0, now(), true
            )
            ON CONFLICT (id) DO UPDATE SET
              author_id = EXCLUDED.author_id,
              post_type = EXCLUDED.post_type,
              animal_type = EXCLUDED.animal_type,
              moderation_status = EXCLUDED.moderation_status,
              urgent = EXCLUDED.urgent,
              rescue_status = EXCLUDED.rescue_status,
              route_public = EXCLUDED.route_public,
              geo_status = EXCLUDED.geo_status,
              likes_count = EXCLUDED.likes_count,
              comments_count = EXCLUDED.comments_count,
              shares_count = EXCLUDED.shares_count
            "#,
        )
        .bind(uuid::Uuid::parse_str(TEST_FEED_POST_ID)?)
        .bind(uuid::Uuid::parse_str(TEST_USER_ID)?)
        .execute(&state.db)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO rescue_sessions (
              id, post_id, reporter_user_id, status, lat, lng, accuracy
            )
            VALUES ($1, $2, $3, 'active', -23.5614, -46.6559, 15.0)
            ON CONFLICT (id) DO UPDATE SET
              post_id = EXCLUDED.post_id,
              reporter_user_id = EXCLUDED.reporter_user_id,
              status = EXCLUDED.status,
              lat = EXCLUDED.lat,
              lng = EXCLUDED.lng,
              accuracy = EXCLUDED.accuracy,
              updated_at = now()
            "#,
        )
        .bind(uuid::Uuid::parse_str(TEST_RESCUE_ID)?)
        .bind(uuid::Uuid::parse_str(TEST_FEED_POST_ID)?)
        .bind(uuid::Uuid::parse_str(TEST_USER_ID)?)
        .execute(&state.db)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO media_upload_intents (
              id, user_id, provider, resource_type, object_key, file_name, content_type,
              size_bytes, checksum_sha256, upload_url, public_url, expires_at, consumed_at
            )
            VALUES (
              $1, $2, 'cloudinary', 'image', $3, 'mel.webp', 'image/webp',
              320000, NULL, 'https://api.cloudinary.com/v1_1/limpeja/image/upload',
              $4, now() + interval '15 minutes', NULL
            )
            ON CONFLICT (object_key) DO UPDATE SET
              user_id = EXCLUDED.user_id,
              public_url = EXCLUDED.public_url,
              expires_at = EXCLUDED.expires_at,
              consumed_at = NULL
            "#,
        )
        .bind(uuid::Uuid::parse_str(TEST_MEDIA_INTENT_ID)?)
        .bind(uuid::Uuid::parse_str(TEST_USER_ID)?)
        .bind(TEST_MEDIA_OBJECT_KEY)
        .bind(TEST_MEDIA_PUBLIC_URL)
        .execute(&state.db)
        .await?;

        Ok(())
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
            .header("authorization", test_auth_header(AccountType::Ong))
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
    async fn feed_and_detail_keep_real_post_author_for_other_viewer() {
        let app = test_app().await;
        let auth = test_auth_header_for(
            TEST_SECOND_USER_ID,
            "joao@zoohelp.test",
            AccountType::Person,
        );
        let feed_request = Request::builder()
            .method(Method::GET)
            .uri("/v1/feed?type=emergency")
            .header("authorization", auth.clone())
            .body(Body::empty())
            .unwrap();

        let (feed_status, feed_body) = request_json(app.clone(), feed_request).await;
        assert_eq!(feed_status, StatusCode::OK);
        assert_eq!(feed_body[0]["author"]["id"], TEST_USER_ID);
        assert_eq!(feed_body[0]["author"]["name"], "Instituto Teste ZooHelp");

        let detail_request = Request::builder()
            .method(Method::GET)
            .uri(format!("/v1/posts/{TEST_FEED_POST_ID}"))
            .header("authorization", auth)
            .body(Body::empty())
            .unwrap();

        let (detail_status, detail_body) = request_json(app, detail_request).await;
        assert_eq!(detail_status, StatusCode::OK);
        assert_eq!(detail_body["author"]["id"], TEST_USER_ID);
    }

    #[tokio::test]
    async fn non_author_cannot_delete_post() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::DELETE)
            .uri(format!("/v1/posts/{TEST_FEED_POST_ID}"))
            .header(
                "authorization",
                test_auth_header_for(
                    TEST_SECOND_USER_ID,
                    "joao@zoohelp.test",
                    AccountType::Person,
                ),
            )
            .body(Body::empty())
            .unwrap();

        let (status, _body) = request_json(app, request).await;

        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn comments_keep_real_comment_author() {
        let app = test_app().await;
        let auth = test_auth_header_for(
            TEST_SECOND_USER_ID,
            "joao@zoohelp.test",
            AccountType::Person,
        );
        let create_request = Request::builder()
            .method(Method::POST)
            .uri(format!("/v1/posts/{TEST_FEED_POST_ID}/comments"))
            .header("authorization", auth)
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "body": "Posso ajudar no transporte." }).to_string(),
            ))
            .unwrap();

        let (create_status, _body) = request_json(app.clone(), create_request).await;
        assert_eq!(create_status, StatusCode::CREATED);

        let list_request = Request::builder()
            .method(Method::GET)
            .uri(format!("/v1/posts/{TEST_FEED_POST_ID}/comments"))
            .body(Body::empty())
            .unwrap();
        let (list_status, list_body) = request_json(app, list_request).await;

        assert_eq!(list_status, StatusCode::OK);
        assert!(list_body
            .as_array()
            .expect("comments")
            .iter()
            .any(|comment| comment["author"]["id"] == TEST_SECOND_USER_ID));
    }

    #[tokio::test]
    async fn like_and_unlike_are_scoped_to_authenticated_user() {
        let app = test_app().await;
        let auth = test_auth_header_for(
            TEST_SECOND_USER_ID,
            "joao@zoohelp.test",
            AccountType::Person,
        );
        for _ in 0..2 {
            let request = Request::builder()
                .method(Method::PUT)
                .uri(format!("/v1/posts/{TEST_FEED_POST_ID}/like"))
                .header("authorization", auth.clone())
                .body(Body::empty())
                .unwrap();
            let (status, body) = request_json(app.clone(), request).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["liked"], true);
            assert_eq!(body["likes"], 1);
        }

        let unlike_request = Request::builder()
            .method(Method::DELETE)
            .uri(format!("/v1/posts/{TEST_FEED_POST_ID}/like"))
            .header("authorization", auth)
            .body(Body::empty())
            .unwrap();
        let (status, body) = request_json(app, unlike_request).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["liked"], false);
        assert_eq!(body["likes"], 0);
    }

    #[tokio::test]
    async fn public_profile_lists_only_requested_user_posts() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::GET)
            .uri(format!("/v1/users/{TEST_USER_ID}"))
            .header(
                "authorization",
                test_auth_header_for(
                    TEST_SECOND_USER_ID,
                    "joao@zoohelp.test",
                    AccountType::Person,
                ),
            )
            .body(Body::empty())
            .unwrap();

        let (status, body) = request_json(app, request).await;

        assert_eq!(status, StatusCode::OK);
        assert!(body["posts"]
            .as_array()
            .expect("posts")
            .iter()
            .all(|post| post["author"]["id"] == TEST_USER_ID));
        assert!(body["posts"]
            .as_array()
            .expect("posts")
            .iter()
            .any(|post| post["id"] == TEST_FEED_POST_ID));
    }

    #[tokio::test]
    async fn follow_put_delete_are_idempotent_and_counts_are_correct() {
        let app = test_app().await;
        let auth = test_auth_header_for(
            TEST_SECOND_USER_ID,
            "joao@zoohelp.test",
            AccountType::Person,
        );
        for _ in 0..2 {
            let request = Request::builder()
                .method(Method::PUT)
                .uri(format!("/v1/users/{TEST_USER_ID}/follow"))
                .header("authorization", auth.clone())
                .body(Body::empty())
                .unwrap();
            let (status, body) = request_json(app.clone(), request).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["following"], true);
            assert_eq!(body["followersCount"], 1);
        }

        let followers_request = Request::builder()
            .method(Method::GET)
            .uri(format!("/v1/users/{TEST_USER_ID}/followers"))
            .body(Body::empty())
            .unwrap();
        let (followers_status, followers_body) = request_json(app.clone(), followers_request).await;
        assert_eq!(followers_status, StatusCode::OK);
        assert_eq!(followers_body.as_array().expect("followers").len(), 1);
        assert_eq!(followers_body[0]["id"], TEST_SECOND_USER_ID);

        let self_follow_request = Request::builder()
            .method(Method::PUT)
            .uri(format!("/v1/users/{TEST_SECOND_USER_ID}/follow"))
            .header("authorization", auth.clone())
            .body(Body::empty())
            .unwrap();
        let (self_status, _body) = request_json(app.clone(), self_follow_request).await;
        assert_eq!(self_status, StatusCode::BAD_REQUEST);

        for _ in 0..2 {
            let request = Request::builder()
                .method(Method::DELETE)
                .uri(format!("/v1/users/{TEST_USER_ID}/follow"))
                .header("authorization", auth.clone())
                .body(Body::empty())
                .unwrap();
            let (status, body) = request_json(app.clone(), request).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["following"], false);
            assert_eq!(body["followersCount"], 0);
        }
    }

    #[tokio::test]
    async fn auth_register_returns_frontend_user_shape() {
        let app = test_app().await;
        let email = format!("ong-{}@zoohelp.test", uuid::Uuid::now_v7());
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/auth/register")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "name": "ONG Teste",
                    "email": email,
                    "password": "senha-segura",
                    "accountType": "ong",
                    "ongType": "rescue",
                    "phone": "(11) 99999-0001",
                    "cep": "01001000",
                    "street": "Rua Teste",
                    "number": "100",
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
                    "description": "Animal vacinado para adoção responsavel.",
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
                    "location": "Localização atual",
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
                    "location": "Localização atual",
                    "neighborhood": "Localização atual",
                    "urgent": true,
                    "latitude": -23.5505,
                    "longitude": -46.6333,
                    "geoSource": "gps_confirmed"
                })
                .to_string(),
            ))
            .unwrap();

        let (status, body) = request_json(app, request).await;

        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["post"]["latitude"], -23.5505);
        assert_eq!(body["post"]["longitude"], -46.6333);
        assert!(body["rescueFanoutStateId"].as_str().is_some());
        assert_eq!(body["post"]["rescueOperational"]["fanoutPhase"], 1);
        assert_eq!(
            body["post"]["rescueOperational"]["operationalLabel"],
            "Precisa de ajuda"
        );
    }

    #[tokio::test]
    async fn media_upload_intent_validates_image_contract() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/media/upload-intents")
            .header("content-type", "application/json")
            .header("authorization", test_auth_header(AccountType::Ong))
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
    async fn support_tickets_are_scoped_to_authenticated_user() {
        let app = test_app().await;
        let paulo_auth = test_auth_header_for(TEST_USER_ID, "admin@zoohelp.test", AccountType::Ong);
        let joao_auth = test_auth_header_for(
            TEST_SECOND_USER_ID,
            "joao@zoohelp.test",
            AccountType::Person,
        );
        let create_request = Request::builder()
            .method(Method::POST)
            .uri("/v1/support/tickets")
            .header("content-type", "application/json")
            .header("authorization", &paulo_auth)
            .body(Body::from(
                json!({
                    "subject": "Problema no perfil",
                    "body": "Preciso de ajuda com meu perfil no aplicativo.",
                    "category": "APP",
                    "severity": "LOW"
                })
                .to_string(),
            ))
            .unwrap();

        let (create_status, create_body) = request_json(app.clone(), create_request).await;

        assert_eq!(create_status, StatusCode::CREATED);
        let ticket_id = create_body["id"].as_str().expect("ticket id");

        let paulo_list = Request::builder()
            .method(Method::GET)
            .uri("/v1/support/tickets")
            .header("authorization", &paulo_auth)
            .body(Body::empty())
            .unwrap();
        let (paulo_status, paulo_body) = request_json(app.clone(), paulo_list).await;
        assert_eq!(paulo_status, StatusCode::OK);
        assert_eq!(paulo_body.as_array().unwrap().len(), 1);

        let joao_list = Request::builder()
            .method(Method::GET)
            .uri("/v1/support/tickets")
            .header("authorization", &joao_auth)
            .body(Body::empty())
            .unwrap();
        let (joao_status, joao_body) = request_json(app.clone(), joao_list).await;
        assert_eq!(joao_status, StatusCode::OK);
        assert_eq!(joao_body.as_array().unwrap().len(), 0);

        let joao_get = Request::builder()
            .method(Method::GET)
            .uri(format!("/v1/support/tickets/{ticket_id}"))
            .header("authorization", &joao_auth)
            .body(Body::empty())
            .unwrap();
        let (joao_get_status, _body) = request_json(app, joao_get).await;
        assert_eq!(joao_get_status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn post_assessment_proxies_worker_contract_with_fallback() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/ai/post-assessment")
            .header("content-type", "application/json")
            .header("authorization", test_auth_header(AccountType::Ong))
            .body(Body::from(
                json!({
                    "description": "Cachorro atropelado precisa de ajuda urgente.",
                    "location": "Sao Paulo, SP",
                    "declaredType": "emergency",
                    "images": []
                })
                .to_string(),
            ))
            .unwrap();

        let (status, body) = request_json(app, request).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["suggestedType"], "emergency");
        assert_eq!(body["urgency"], "high");
        assert_eq!(body["promptVersion"], "post-assessment-v1");
    }

    #[tokio::test]
    async fn rescue_brief_proxies_worker_contract_with_fallback() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/v1/rescue/{TEST_RESCUE_ID}/brief"))
            .header("authorization", test_auth_header(AccountType::Ong))
            .body(Body::empty())
            .unwrap();

        let (status, body) = request_json(app, request).await;

        assert_eq!(status, StatusCode::OK);
        assert!(body["summary"]
            .as_str()
            .expect("summary")
            .contains("Resgate em acompanhamento"));
        assert_eq!(body["promptVersion"], "rescue-brief-v1");
        assert!(body["checklist"].as_array().is_some());
    }

    #[tokio::test]
    async fn ong_detail_matches_frontend_profile_need() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::GET)
            .uri(format!("/v1/ongs/{TEST_ONG_ID}"))
            .body(Body::empty())
            .unwrap();

        let (status, body) = request_json(app, request).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["id"], TEST_ONG_ID);
        assert_eq!(body["verified"], true);
        assert!(body["animalsRescued"].as_u64().unwrap() > 0);
    }
}
