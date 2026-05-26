CREATE TABLE IF NOT EXISTS user_follows (
  follower_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  followed_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  active boolean NOT NULL DEFAULT true,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (follower_id, followed_id),
  CHECK (follower_id <> followed_id)
);

CREATE INDEX IF NOT EXISTS user_follows_followed_idx
  ON user_follows (followed_id, active);

