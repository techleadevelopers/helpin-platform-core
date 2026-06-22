ALTER TABLE support_tickets
  ADD COLUMN IF NOT EXISTS user_id uuid REFERENCES users(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS support_tickets_user_created_idx
  ON support_tickets (user_id, created_at DESC)
  WHERE user_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS media_upload_intents_user_active_idx
  ON media_upload_intents (user_id, expires_at DESC)
  WHERE consumed_at IS NULL;
