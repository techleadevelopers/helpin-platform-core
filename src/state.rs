use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, PgPool};
use tokio::sync::broadcast;

use crate::{
    config::Config,
    services::{email::EmailService, event_bus::EventBus},
};

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub db: PgPool,
    pub started_at: DateTime<Utc>,
    pub chat_tx: broadcast::Sender<ChatEvent>,
    pub rescue_tx: broadcast::Sender<RescueEvent>,
    pub email: EmailService,
    pub event_bus: EventBus,
    pub redis: Option<redis::Client>,
    pub rate_limiter: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
}

impl AppState {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        let db = PgPoolOptions::new()
            .max_connections(config.database_max_connections)
            .min_connections(config.database_min_connections)
            .acquire_timeout(Duration::from_secs(5))
            .connect_lazy(&config.database_url)?;
        ensure_runtime_schema(&db, config.postgis_enabled).await?;

        let (chat_tx, _) = broadcast::channel(1024);
        let (rescue_tx, _) = broadcast::channel(4096);
        let email = EmailService::new(config.clone());
        let event_bus = EventBus::connect(&config).await?;
        event_bus.spawn_bridge(chat_tx.clone(), rescue_tx.clone());
        let redis = if config.app_env == "test" {
            None
        } else {
            Some(redis::Client::open(config.redis_url.as_str())?)
        };
        crate::services::push_worker::spawn(config.clone(), db.clone());

        Ok(Self {
            config,
            db,
            started_at: Utc::now(),
            chat_tx,
            rescue_tx,
            email,
            event_bus,
            redis,
            rate_limiter: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}

async fn ensure_runtime_schema(db: &PgPool, postgis_enabled: bool) -> anyhow::Result<()> {
    let base_statements = [
        r#"
        DO $$
        BEGIN
            IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'moderation_status') THEN
                CREATE TYPE moderation_status AS ENUM ('queued', 'approved', 'rejected', 'needs_review');
            END IF;
        END
        $$;
        "#,
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS avatar_url text;",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS deleted_at timestamptz;",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS anonymized_at timestamptz;",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS retention_delete_after timestamptz;",
        "ALTER TABLE ong_profiles ADD COLUMN IF NOT EXISTS cep text;",
        "ALTER TABLE ong_profiles ADD COLUMN IF NOT EXISTS street text;",
        "ALTER TABLE ong_profiles ADD COLUMN IF NOT EXISTS number text;",
        "ALTER TABLE ong_profiles ADD COLUMN IF NOT EXISTS complement text;",
        "ALTER TABLE ong_profiles ADD COLUMN IF NOT EXISTS neighborhood text;",
        "ALTER TABLE ong_profiles ADD COLUMN IF NOT EXISTS foundation_year integer;",
        "ALTER TABLE posts ADD COLUMN IF NOT EXISTS tags text[] NOT NULL DEFAULT '{}';",
        "ALTER TABLE posts ADD COLUMN IF NOT EXISTS urgent boolean NOT NULL DEFAULT false;",
        "ALTER TABLE posts ADD COLUMN IF NOT EXISTS rescue_status text NOT NULL DEFAULT 'open';",
        "ALTER TABLE posts ADD COLUMN IF NOT EXISTS resolved_at timestamptz;",
        "ALTER TABLE posts DROP CONSTRAINT IF EXISTS posts_rescue_status_check;",
        "ALTER TABLE posts ADD CONSTRAINT posts_rescue_status_check CHECK (rescue_status IN ('open', 'active', 'resolved', 'cancelled'));",
        "ALTER TABLE posts ADD COLUMN IF NOT EXISTS text_only boolean NOT NULL DEFAULT false;",
        "ALTER TABLE posts ADD COLUMN IF NOT EXISTS likes_count integer NOT NULL DEFAULT 0 CHECK (likes_count >= 0);",
        "ALTER TABLE posts ADD COLUMN IF NOT EXISTS comments_count integer NOT NULL DEFAULT 0 CHECK (comments_count >= 0);",
        "ALTER TABLE posts ADD COLUMN IF NOT EXISTS shares_count integer NOT NULL DEFAULT 0 CHECK (shares_count >= 0);",
        "ALTER TABLE posts ADD COLUMN IF NOT EXISTS moderation_status moderation_status NOT NULL DEFAULT 'approved';",
        "ALTER TABLE posts ALTER COLUMN moderation_status SET DEFAULT 'approved';",
        "UPDATE posts SET moderation_status = 'approved' WHERE moderation_status IN ('queued', 'needs_review');",
        "ALTER TABLE posts DROP CONSTRAINT IF EXISTS posts_no_pending_visibility;",
        "ALTER TABLE posts ADD CONSTRAINT posts_no_pending_visibility CHECK (moderation_status NOT IN ('queued', 'needs_review'));",
        "ALTER TABLE posts ADD COLUMN IF NOT EXISTS fraud_risk smallint NOT NULL DEFAULT 0 CHECK (fraud_risk BETWEEN 0 AND 100);",
        "ALTER TABLE posts ADD COLUMN IF NOT EXISTS idempotency_key text;",
        "CREATE UNIQUE INDEX IF NOT EXISTS posts_author_idempotency_idx ON posts (author_id, idempotency_key) WHERE idempotency_key IS NOT NULL;",
        r#"
        CREATE TABLE IF NOT EXISTS media_upload_intents (
          id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
          user_id uuid REFERENCES users(id) ON DELETE SET NULL,
          provider text NOT NULL DEFAULT 'cloudinary',
          resource_type text NOT NULL CHECK (resource_type IN ('image', 'video')),
          object_key text NOT NULL UNIQUE,
          file_name text NOT NULL,
          content_type text NOT NULL,
          size_bytes bigint NOT NULL CHECK (size_bytes > 0),
          checksum_sha256 text,
          upload_url text NOT NULL,
          public_url text NOT NULL,
          expires_at timestamptz NOT NULL,
          consumed_at timestamptz,
          created_at timestamptz NOT NULL DEFAULT now()
        );
        "#,
        "CREATE INDEX IF NOT EXISTS media_upload_intents_user_idx ON media_upload_intents (user_id, created_at DESC);",
        r#"
        CREATE TABLE IF NOT EXISTS audit_events (
          id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
          actor_user_id uuid REFERENCES users(id) ON DELETE SET NULL,
          action text NOT NULL,
          target_type text,
          target_id text,
          metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
          created_at timestamptz NOT NULL DEFAULT now()
        );
        "#,
        "CREATE INDEX IF NOT EXISTS audit_events_actor_created_idx ON audit_events (actor_user_id, created_at DESC);",
        "CREATE INDEX IF NOT EXISTS audit_events_action_created_idx ON audit_events (action, created_at DESC);",
        r#"
        CREATE TABLE IF NOT EXISTS push_subscriptions (
          id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
          user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
          push_token text NOT NULL UNIQUE,
          platform text NOT NULL CHECK (platform IN ('ios', 'android', 'expo', 'web')),
          lat double precision NOT NULL CHECK (lat BETWEEN -90 AND 90),
          lng double precision NOT NULL CHECK (lng BETWEEN -180 AND 180),
          radius_km double precision NOT NULL DEFAULT 8 CHECK (radius_km BETWEEN 0.03 AND 50),
          critical_alerts boolean NOT NULL DEFAULT false,
          updated_at timestamptz NOT NULL DEFAULT now(),
          created_at timestamptz NOT NULL DEFAULT now()
        );
        "#,
        "CREATE INDEX IF NOT EXISTS push_subscriptions_user_idx ON push_subscriptions (user_id, updated_at DESC);",
        "CREATE INDEX IF NOT EXISTS push_subscriptions_location_idx ON push_subscriptions (lat, lng);",
        r#"
        CREATE TABLE IF NOT EXISTS notification_events (
          id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
          user_id uuid REFERENCES users(id) ON DELETE CASCADE,
          kind text NOT NULL,
          title text NOT NULL,
          body text NOT NULL,
          post_id text,
          image_url text,
          distance_km double precision,
          critical boolean NOT NULL DEFAULT false,
          deeplink text,
          dedupe_key text,
          ttl_seconds integer,
          category text,
          payload jsonb NOT NULL DEFAULT '{}'::jsonb,
          read_at timestamptz,
          acked_at timestamptz,
          created_at timestamptz NOT NULL DEFAULT now()
        );
        "#,
        "CREATE INDEX IF NOT EXISTS notification_events_user_created_idx ON notification_events (user_id, created_at DESC);",
        "CREATE INDEX IF NOT EXISTS notification_events_dedupe_idx ON notification_events (dedupe_key, user_id);",
        r#"
        CREATE TABLE IF NOT EXISTS push_delivery_jobs (
          id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
          notification_event_id uuid REFERENCES notification_events(id) ON DELETE CASCADE,
          user_id uuid REFERENCES users(id) ON DELETE CASCADE,
          push_token text NOT NULL,
          platform text NOT NULL,
          payload jsonb NOT NULL,
          status text NOT NULL DEFAULT 'queued' CHECK (status IN ('queued', 'sent', 'failed', 'dead_letter')),
          attempts integer NOT NULL DEFAULT 0,
          next_attempt_at timestamptz NOT NULL DEFAULT now(),
          last_error text,
          created_at timestamptz NOT NULL DEFAULT now(),
          updated_at timestamptz NOT NULL DEFAULT now()
        );
        "#,
        "CREATE INDEX IF NOT EXISTS push_delivery_jobs_status_next_idx ON push_delivery_jobs (status, next_attempt_at);",
        r#"
        CREATE TABLE IF NOT EXISTS user_ong_follows (
          user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
          ong_id uuid NOT NULL REFERENCES ong_profiles(id) ON DELETE CASCADE,
          active boolean NOT NULL DEFAULT true,
          created_at timestamptz NOT NULL DEFAULT now(),
          updated_at timestamptz NOT NULL DEFAULT now(),
          PRIMARY KEY (user_id, ong_id)
        );
        "#,
        "CREATE INDEX IF NOT EXISTS user_ong_follows_ong_idx ON user_ong_follows (ong_id, active);",
        r#"
        CREATE TABLE IF NOT EXISTS post_media (
          id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
          post_id uuid NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
          provider text NOT NULL DEFAULT 'cloudinary',
          resource_type text NOT NULL DEFAULT 'image' CHECK (resource_type IN ('image', 'video')),
          object_key text NOT NULL,
          public_url text NOT NULL,
          content_type text NOT NULL,
          width integer CHECK (width IS NULL OR width > 0),
          height integer CHECK (height IS NULL OR height > 0),
          size_bytes bigint CHECK (size_bytes IS NULL OR size_bytes > 0),
          checksum_sha256 text,
          sort_order smallint NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
          moderation_status moderation_status NOT NULL DEFAULT 'approved',
          moderation_labels text[] NOT NULL DEFAULT '{}',
          created_at timestamptz NOT NULL DEFAULT now()
        );
        "#,
        "ALTER TABLE post_media ALTER COLUMN moderation_status SET DEFAULT 'approved';",
        "UPDATE post_media SET moderation_status = 'approved' WHERE moderation_status IN ('queued', 'needs_review');",
        "ALTER TABLE post_media DROP CONSTRAINT IF EXISTS post_media_no_pending_visibility;",
        "ALTER TABLE post_media ADD CONSTRAINT post_media_no_pending_visibility CHECK (moderation_status NOT IN ('queued', 'needs_review'));",
        "CREATE INDEX IF NOT EXISTS post_media_post_idx ON post_media (post_id, sort_order);",
        "CREATE INDEX IF NOT EXISTS post_media_moderation_idx ON post_media (moderation_status, created_at);",
        r#"
        CREATE TABLE IF NOT EXISTS post_likes (
          post_id uuid NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
          user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
          created_at timestamptz NOT NULL DEFAULT now(),
          PRIMARY KEY (post_id, user_id)
        );
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS post_comments (
          id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
          post_id uuid NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
          user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
          body text NOT NULL CHECK (char_length(body) BETWEEN 1 AND 2000),
          moderation_status text NOT NULL DEFAULT 'visible_monitored',
          created_at timestamptz NOT NULL DEFAULT now()
        );
        "#,
        "CREATE INDEX IF NOT EXISTS post_comments_post_created_idx ON post_comments (post_id, created_at DESC);",
        r#"
        CREATE TABLE IF NOT EXISTS post_reports (
          id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
          post_id uuid NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
          reporter_user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
          reason text NOT NULL,
          details text,
          severity text NOT NULL DEFAULT 'normal',
          status text NOT NULL DEFAULT 'queued_review',
          created_at timestamptz NOT NULL DEFAULT now()
        );
        "#,
        "CREATE INDEX IF NOT EXISTS post_reports_status_created_idx ON post_reports (status, created_at);",
        r#"
        CREATE TABLE IF NOT EXISTS rescue_sessions (
          id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
          post_id text NOT NULL,
          reporter_user_id uuid REFERENCES users(id) ON DELETE SET NULL,
          status text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'ended')),
          lat double precision NOT NULL CHECK (lat BETWEEN -90 AND 90),
          lng double precision NOT NULL CHECK (lng BETWEEN -180 AND 180),
          accuracy double precision CHECK (accuracy IS NULL OR accuracy >= 0),
          created_at timestamptz NOT NULL DEFAULT now(),
          updated_at timestamptz NOT NULL DEFAULT now(),
          ended_at timestamptz
        );
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS rescue_location_points (
          id bigserial PRIMARY KEY,
          rescue_id uuid NOT NULL REFERENCES rescue_sessions(id) ON DELETE CASCADE,
          lat double precision NOT NULL CHECK (lat BETWEEN -90 AND 90),
          lng double precision NOT NULL CHECK (lng BETWEEN -180 AND 180),
          accuracy double precision CHECK (accuracy IS NULL OR accuracy >= 0),
          recorded_at timestamptz NOT NULL DEFAULT now()
        );
        "#,
        r#"
        CREATE TABLE IF NOT EXISTS rescue_incidents (
          id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
          rescue_id uuid NOT NULL REFERENCES rescue_sessions(id) ON DELETE CASCADE,
          description text NOT NULL CHECK (char_length(description) BETWEEN 1 AND 2000),
          attachments jsonb NOT NULL DEFAULT '[]'::jsonb,
          status text NOT NULL DEFAULT 'queued_review',
          created_at timestamptz NOT NULL DEFAULT now()
        );
        "#,
        "CREATE INDEX IF NOT EXISTS rescue_sessions_active_updated_idx ON rescue_sessions (updated_at DESC) WHERE status = 'active';",
        "CREATE INDEX IF NOT EXISTS rescue_sessions_post_idx ON rescue_sessions (post_id, created_at DESC);",
        "CREATE INDEX IF NOT EXISTS rescue_sessions_location_idx ON rescue_sessions (lat, lng) WHERE status = 'active';",
        "CREATE INDEX IF NOT EXISTS rescue_location_points_rescue_recorded_idx ON rescue_location_points (rescue_id, recorded_at DESC);",
        "CREATE INDEX IF NOT EXISTS rescue_incidents_rescue_created_idx ON rescue_incidents (rescue_id, created_at DESC);",
        "ALTER TABLE donations ADD COLUMN IF NOT EXISTS idempotency_key text;",
        "CREATE UNIQUE INDEX IF NOT EXISTS donations_donor_idempotency_idx ON donations (donor_id, idempotency_key) WHERE idempotency_key IS NOT NULL;",
        r#"
        CREATE TABLE IF NOT EXISTS donation_ledger_entries (
          id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
          donation_id uuid NOT NULL REFERENCES donations(id) ON DELETE CASCADE,
          entry_type text NOT NULL,
          amount_cents bigint NOT NULL,
          currency char(3) NOT NULL,
          metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
          created_at timestamptz NOT NULL DEFAULT now()
        );
        "#,
        "CREATE INDEX IF NOT EXISTS donation_ledger_entries_donation_idx ON donation_ledger_entries (donation_id, created_at);",
        r#"
        CREATE TABLE IF NOT EXISTS payment_webhook_events (
          id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
          provider text NOT NULL,
          provider_event_id text NOT NULL,
          event_type text NOT NULL,
          donation_id uuid REFERENCES donations(id) ON DELETE SET NULL,
          payload jsonb NOT NULL,
          processed_at timestamptz,
          created_at timestamptz NOT NULL DEFAULT now(),
          UNIQUE (provider, provider_event_id)
        );
        "#,
        "CREATE INDEX IF NOT EXISTS payment_webhook_events_donation_idx ON payment_webhook_events (donation_id, created_at DESC);",
        r#"
        CREATE TABLE IF NOT EXISTS ong_kyb_documents (
          id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
          ong_id uuid NOT NULL REFERENCES ong_profiles(id) ON DELETE CASCADE,
          document_type text NOT NULL,
          object_key text NOT NULL,
          public_url text NOT NULL,
          status text NOT NULL DEFAULT 'pending_review',
          reviewer_user_id uuid REFERENCES users(id) ON DELETE SET NULL,
          rejection_reason text,
          created_at timestamptz NOT NULL DEFAULT now(),
          reviewed_at timestamptz
        );
        "#,
        "CREATE INDEX IF NOT EXISTS ong_kyb_documents_ong_status_idx ON ong_kyb_documents (ong_id, status, created_at DESC);",
        r#"
        CREATE TABLE IF NOT EXISTS moderation_jobs (
          id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
          subject_type text NOT NULL,
          subject_id text NOT NULL,
          image_url text,
          status text NOT NULL DEFAULT 'queued',
          score smallint CHECK (score IS NULL OR score BETWEEN 0 AND 100),
          labels text[] NOT NULL DEFAULT '{}',
          provider text,
          error text,
          created_at timestamptz NOT NULL DEFAULT now(),
          updated_at timestamptz NOT NULL DEFAULT now()
        );
        "#,
        "CREATE INDEX IF NOT EXISTS moderation_jobs_status_created_idx ON moderation_jobs (status, created_at);",
    ];

    for statement in base_statements {
        sqlx::query(statement).execute(db).await?;
    }

    if postgis_enabled {
        for statement in [
            "CREATE EXTENSION IF NOT EXISTS postgis;",
            "ALTER TABLE posts ADD COLUMN IF NOT EXISTS geo geography(Point, 4326);",
            "UPDATE posts SET geo = ST_SetSRID(ST_MakePoint(longitude, latitude), 4326)::geography WHERE geo IS NULL AND latitude IS NOT NULL AND longitude IS NOT NULL;",
            "CREATE INDEX IF NOT EXISTS posts_geo_gist_idx ON posts USING gist (geo);",
        ] {
            sqlx::query(statement).execute(db).await?;
        }
    }

    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatEvent {
    pub room_id: String,
    pub message_id: String,
    pub sender_id: String,
    pub body: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RescueEvent {
    pub rescue_id: String,
    pub post_id: String,
    pub status: String,
    pub lat: f64,
    pub lng: f64,
    pub accuracy: Option<f64>,
    pub updated_at: String,
}
