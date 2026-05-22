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

CREATE TABLE IF NOT EXISTS rescue_location_points (
  id bigserial PRIMARY KEY,
  rescue_id uuid NOT NULL REFERENCES rescue_sessions(id) ON DELETE CASCADE,
  lat double precision NOT NULL CHECK (lat BETWEEN -90 AND 90),
  lng double precision NOT NULL CHECK (lng BETWEEN -180 AND 180),
  accuracy double precision CHECK (accuracy IS NULL OR accuracy >= 0),
  recorded_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS rescue_incidents (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  rescue_id uuid NOT NULL REFERENCES rescue_sessions(id) ON DELETE CASCADE,
  description text NOT NULL CHECK (char_length(description) BETWEEN 1 AND 2000),
  attachments jsonb NOT NULL DEFAULT '[]'::jsonb,
  status text NOT NULL DEFAULT 'queued_review',
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS rescue_sessions_active_updated_idx
  ON rescue_sessions (updated_at DESC)
  WHERE status = 'active';

CREATE INDEX IF NOT EXISTS rescue_sessions_post_idx
  ON rescue_sessions (post_id, created_at DESC);

CREATE INDEX IF NOT EXISTS rescue_sessions_location_idx
  ON rescue_sessions (lat, lng)
  WHERE status = 'active';

CREATE INDEX IF NOT EXISTS rescue_location_points_rescue_recorded_idx
  ON rescue_location_points (rescue_id, recorded_at DESC);

CREATE INDEX IF NOT EXISTS rescue_incidents_rescue_created_idx
  ON rescue_incidents (rescue_id, created_at DESC);
