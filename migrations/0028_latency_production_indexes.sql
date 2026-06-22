CREATE INDEX IF NOT EXISTS notification_events_user_kind_created_idx
  ON notification_events (user_id, kind, created_at DESC);

CREATE INDEX IF NOT EXISTS notification_events_post_kind_created_idx
  ON notification_events (post_id, kind, created_at DESC);

CREATE INDEX IF NOT EXISTS push_delivery_jobs_due_created_idx
  ON push_delivery_jobs (status, next_attempt_at, created_at ASC)
  WHERE status IN ('queued', 'failed');

CREATE INDEX IF NOT EXISTS push_subscriptions_active_location_idx
  ON push_subscriptions (updated_at DESC, lat, lng)
  WHERE invalidated_at IS NULL;

CREATE INDEX IF NOT EXISTS rescue_responses_status_created_post_idx
  ON rescue_responses (status, created_at DESC, post_id);

CREATE INDEX IF NOT EXISTS rescue_final_reports_publication_status_status_post_idx
  ON rescue_final_reports (publication_status, status, post_id);

CREATE INDEX IF NOT EXISTS posts_moderation_created_idx
  ON posts (moderation_status, created_at DESC);

CREATE INDEX IF NOT EXISTS posts_operational_feed_idx
  ON posts (moderation_status, urgent, rescue_status, created_at DESC);
