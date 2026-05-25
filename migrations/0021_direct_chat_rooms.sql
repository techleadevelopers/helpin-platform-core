ALTER TABLE chat_rooms
  ADD COLUMN IF NOT EXISTS direct_pair_key text;

CREATE UNIQUE INDEX IF NOT EXISTS chat_rooms_direct_pair_idx
  ON chat_rooms (direct_pair_key)
  WHERE direct_pair_key IS NOT NULL;
