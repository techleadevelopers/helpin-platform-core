-- One reporter has at most one active session for a case.  The application
-- also serializes trigger commands so it can return that existing session.
CREATE UNIQUE INDEX IF NOT EXISTS rescue_sessions_one_active_per_reporter_post_idx
  ON rescue_sessions (post_id, reporter_user_id)
  WHERE status = 'active';
