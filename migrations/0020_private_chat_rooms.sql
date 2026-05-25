ALTER TABLE chat_rooms
  ADD COLUMN IF NOT EXISTS requester_id uuid REFERENCES users(id) ON DELETE CASCADE;

CREATE UNIQUE INDEX IF NOT EXISTS chat_rooms_private_post_requester_idx
  ON chat_rooms (post_id, requester_id)
  WHERE post_id IS NOT NULL AND requester_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS chat_room_members (
  room_id uuid NOT NULL REFERENCES chat_rooms(id) ON DELETE CASCADE,
  user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  last_read_at timestamptz,
  joined_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (room_id, user_id)
);

CREATE INDEX IF NOT EXISTS chat_room_members_user_idx
  ON chat_room_members (user_id, room_id);
