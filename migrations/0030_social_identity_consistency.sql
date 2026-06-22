CREATE INDEX IF NOT EXISTS posts_author_created_idx
  ON posts (author_id, created_at DESC);
