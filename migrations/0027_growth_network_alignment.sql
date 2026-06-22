CREATE TABLE IF NOT EXISTS rescue_events (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  rescue_id UUID NOT NULL REFERENCES rescue_sessions(id) ON DELETE CASCADE,
  post_id UUID NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
  type TEXT NOT NULL,
  actor_id UUID REFERENCES users(id) ON DELETE SET NULL,
  message TEXT,
  metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS rescue_events_rescue_created_idx
  ON rescue_events (rescue_id, created_at ASC);

CREATE INDEX IF NOT EXISTS rescue_events_post_created_idx
  ON rescue_events (post_id, created_at ASC);

CREATE INDEX IF NOT EXISTS rescue_events_type_idx
  ON rescue_events (type);
