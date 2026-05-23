ALTER TABLE posts ADD COLUMN IF NOT EXISTS rescue_status text NOT NULL DEFAULT 'open';
ALTER TABLE posts ADD COLUMN IF NOT EXISTS resolved_at timestamptz;

ALTER TABLE posts DROP CONSTRAINT IF EXISTS posts_rescue_status_check;
ALTER TABLE posts
  ADD CONSTRAINT posts_rescue_status_check
  CHECK (rescue_status IN ('open', 'active', 'resolved', 'cancelled'));

CREATE INDEX IF NOT EXISTS posts_rescue_status_created_idx
  ON posts (rescue_status, created_at DESC);
