CREATE TABLE IF NOT EXISTS rescue_responses (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  rescue_session_id uuid REFERENCES rescue_sessions(id) ON DELETE CASCADE,
  post_id uuid NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
  user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  action text NOT NULL DEFAULT 'going' CHECK (action IN ('going', 'remote_support', 'unavailable')),
  status text NOT NULL DEFAULT 'confirmed' CHECK (status IN ('confirmed', 'cancelled', 'arrived')),
  lat double precision CHECK (lat IS NULL OR lat BETWEEN -90 AND 90),
  lng double precision CHECK (lng IS NULL OR lng BETWEEN -180 AND 180),
  eta_seconds integer CHECK (eta_seconds IS NULL OR eta_seconds >= 0),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (post_id, user_id, action)
);

CREATE INDEX IF NOT EXISTS rescue_responses_post_status_idx
  ON rescue_responses (post_id, status, created_at DESC);

CREATE INDEX IF NOT EXISTS rescue_responses_rescue_status_idx
  ON rescue_responses (rescue_session_id, status, created_at DESC);

CREATE TABLE IF NOT EXISTS rescue_fanout_states (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  post_id uuid NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
  rescue_session_id uuid REFERENCES rescue_sessions(id) ON DELETE SET NULL,
  current_phase integer NOT NULL DEFAULT 1 CHECK (current_phase BETWEEN 1 AND 5),
  status text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'paused', 'completed', 'cancelled', 'exhausted')),
  last_radius_km numeric NOT NULL,
  next_run_at timestamptz NOT NULL,
  confirmed_count integer NOT NULL DEFAULT 0 CHECK (confirmed_count >= 0),
  arrived_count integer NOT NULL DEFAULT 0 CHECK (arrived_count >= 0),
  attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  completed_at timestamptz,
  UNIQUE (post_id)
);

CREATE INDEX IF NOT EXISTS rescue_fanout_states_due_idx
  ON rescue_fanout_states (status, next_run_at)
  WHERE status = 'active';

CREATE INDEX IF NOT EXISTS rescue_fanout_states_post_idx
  ON rescue_fanout_states (post_id);

CREATE TABLE IF NOT EXISTS rescue_fanout_attempts (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  fanout_state_id uuid NOT NULL REFERENCES rescue_fanout_states(id) ON DELETE CASCADE,
  post_id uuid NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
  phase integer NOT NULL CHECK (phase BETWEEN 1 AND 5),
  radius_km numeric NOT NULL,
  candidate_count integer NOT NULL DEFAULT 0 CHECK (candidate_count >= 0),
  push_jobs_created integer NOT NULL DEFAULT 0 CHECK (push_jobs_created >= 0),
  confirmed_count_at_run integer NOT NULL DEFAULT 0 CHECK (confirmed_count_at_run >= 0),
  reason text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS rescue_fanout_attempts_state_created_idx
  ON rescue_fanout_attempts (fanout_state_id, created_at DESC);
