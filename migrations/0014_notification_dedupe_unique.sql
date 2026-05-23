DELETE FROM notification_events newer
USING notification_events older
WHERE newer.id > older.id
  AND newer.dedupe_key IS NOT NULL
  AND newer.user_id IS NOT NULL
  AND newer.dedupe_key = older.dedupe_key
  AND newer.user_id = older.user_id;

CREATE UNIQUE INDEX IF NOT EXISTS notification_events_user_dedupe_unique_idx
  ON notification_events (dedupe_key, user_id)
  WHERE dedupe_key IS NOT NULL AND user_id IS NOT NULL;
