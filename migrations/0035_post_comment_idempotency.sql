ALTER TABLE post_comments ADD COLUMN IF NOT EXISTS idempotency_key varchar(128);

CREATE UNIQUE INDEX IF NOT EXISTS post_comments_idempotency_per_author_idx
  ON post_comments (post_id, user_id, idempotency_key)
  WHERE idempotency_key IS NOT NULL;
