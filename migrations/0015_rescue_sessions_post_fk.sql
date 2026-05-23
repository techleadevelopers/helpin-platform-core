DELETE FROM rescue_location_points rlp
USING rescue_sessions rs
WHERE rlp.rescue_id = rs.id
  AND NOT EXISTS (
    SELECT 1
    FROM posts p
    WHERE p.id::text = rs.post_id::text
  );

DELETE FROM rescue_incidents ri
USING rescue_sessions rs
WHERE ri.rescue_id = rs.id
  AND NOT EXISTS (
    SELECT 1
    FROM posts p
    WHERE p.id::text = rs.post_id::text
  );

DELETE FROM rescue_sessions rs
WHERE NOT EXISTS (
  SELECT 1
  FROM posts p
  WHERE p.id::text = rs.post_id::text
);

ALTER TABLE rescue_sessions
  ALTER COLUMN post_id TYPE uuid USING post_id::uuid;

ALTER TABLE rescue_sessions
  DROP CONSTRAINT IF EXISTS rescue_sessions_post_id_fkey;

ALTER TABLE rescue_sessions
  ADD CONSTRAINT rescue_sessions_post_id_fkey
  FOREIGN KEY (post_id) REFERENCES posts(id) ON DELETE CASCADE;
