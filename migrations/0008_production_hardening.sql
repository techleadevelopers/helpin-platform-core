ALTER TABLE users ADD COLUMN IF NOT EXISTS deleted_at timestamptz;
ALTER TABLE users ADD COLUMN IF NOT EXISTS anonymized_at timestamptz;
ALTER TABLE users ADD COLUMN IF NOT EXISTS retention_delete_after timestamptz;

ALTER TABLE posts ADD COLUMN IF NOT EXISTS idempotency_key text;
CREATE UNIQUE INDEX IF NOT EXISTS posts_author_idempotency_idx
  ON posts (author_id, idempotency_key)
  WHERE idempotency_key IS NOT NULL;

CREATE TABLE IF NOT EXISTS post_likes (
  post_id uuid NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
  user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (post_id, user_id)
);

CREATE TABLE IF NOT EXISTS post_comments (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  post_id uuid NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
  user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  body text NOT NULL CHECK (char_length(body) BETWEEN 1 AND 2000),
  moderation_status text NOT NULL DEFAULT 'visible_monitored',
  created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS post_comments_post_created_idx ON post_comments (post_id, created_at DESC);

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
CREATE INDEX IF NOT EXISTS post_reports_status_created_idx ON post_reports (status, created_at);

CREATE TABLE IF NOT EXISTS audit_events (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  actor_user_id uuid REFERENCES users(id) ON DELETE SET NULL,
  action text NOT NULL,
  target_type text,
  target_id text,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
  created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS audit_events_actor_created_idx ON audit_events (actor_user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS audit_events_action_created_idx ON audit_events (action, created_at DESC);

CREATE TABLE IF NOT EXISTS push_subscriptions (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  push_token text NOT NULL UNIQUE,
  platform text NOT NULL CHECK (platform IN ('ios', 'android', 'expo', 'web')),
  lat double precision NOT NULL CHECK (lat BETWEEN -90 AND 90),
  lng double precision NOT NULL CHECK (lng BETWEEN -180 AND 180),
  radius_km double precision NOT NULL DEFAULT 8 CHECK (radius_km BETWEEN 1 AND 50),
  critical_alerts boolean NOT NULL DEFAULT false,
  updated_at timestamptz NOT NULL DEFAULT now(),
  created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS push_subscriptions_user_idx ON push_subscriptions (user_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS push_subscriptions_location_idx ON push_subscriptions (lat, lng);

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
CREATE INDEX IF NOT EXISTS notification_events_user_created_idx ON notification_events (user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS notification_events_dedupe_idx ON notification_events (dedupe_key, user_id);

CREATE TABLE IF NOT EXISTS user_ong_follows (
  user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  ong_id uuid NOT NULL REFERENCES ong_profiles(id) ON DELETE CASCADE,
  active boolean NOT NULL DEFAULT true,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (user_id, ong_id)
);
CREATE INDEX IF NOT EXISTS user_ong_follows_ong_idx ON user_ong_follows (ong_id, active);

ALTER TABLE donations ADD COLUMN IF NOT EXISTS idempotency_key text;
CREATE UNIQUE INDEX IF NOT EXISTS donations_donor_idempotency_idx
  ON donations (donor_id, idempotency_key)
  WHERE idempotency_key IS NOT NULL;

CREATE TABLE IF NOT EXISTS donation_ledger_entries (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  donation_id uuid NOT NULL REFERENCES donations(id) ON DELETE CASCADE,
  entry_type text NOT NULL,
  amount_cents bigint NOT NULL,
  currency char(3) NOT NULL,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
  created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS donation_ledger_entries_donation_idx
  ON donation_ledger_entries (donation_id, created_at);

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
CREATE INDEX IF NOT EXISTS moderation_jobs_status_created_idx
  ON moderation_jobs (status, created_at);
