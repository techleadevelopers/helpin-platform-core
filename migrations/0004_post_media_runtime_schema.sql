DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'moderation_status') THEN
        CREATE TYPE moderation_status AS ENUM ('queued', 'approved', 'rejected', 'needs_review');
    END IF;
END
$$;

ALTER TABLE users ADD COLUMN IF NOT EXISTS avatar_url text;

ALTER TABLE posts ADD COLUMN IF NOT EXISTS tags text[] NOT NULL DEFAULT '{}';
ALTER TABLE posts ADD COLUMN IF NOT EXISTS urgent boolean NOT NULL DEFAULT false;
ALTER TABLE posts ADD COLUMN IF NOT EXISTS text_only boolean NOT NULL DEFAULT false;
ALTER TABLE posts ADD COLUMN IF NOT EXISTS likes_count integer NOT NULL DEFAULT 0 CHECK (likes_count >= 0);
ALTER TABLE posts ADD COLUMN IF NOT EXISTS comments_count integer NOT NULL DEFAULT 0 CHECK (comments_count >= 0);
ALTER TABLE posts ADD COLUMN IF NOT EXISTS shares_count integer NOT NULL DEFAULT 0 CHECK (shares_count >= 0);
ALTER TABLE posts ADD COLUMN IF NOT EXISTS moderation_status moderation_status NOT NULL DEFAULT 'queued';
ALTER TABLE posts ADD COLUMN IF NOT EXISTS fraud_risk smallint NOT NULL DEFAULT 0 CHECK (fraud_risk BETWEEN 0 AND 100);

CREATE TABLE IF NOT EXISTS media_upload_intents (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id uuid REFERENCES users(id) ON DELETE SET NULL,
  provider text NOT NULL DEFAULT 'cloudinary',
  resource_type text NOT NULL CHECK (resource_type IN ('image', 'video')),
  object_key text NOT NULL UNIQUE,
  file_name text NOT NULL,
  content_type text NOT NULL,
  size_bytes bigint NOT NULL CHECK (size_bytes > 0),
  checksum_sha256 text,
  upload_url text NOT NULL,
  public_url text NOT NULL,
  expires_at timestamptz NOT NULL,
  consumed_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS media_upload_intents_user_idx
  ON media_upload_intents (user_id, created_at DESC);

CREATE TABLE IF NOT EXISTS post_media (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  post_id uuid NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
  provider text NOT NULL DEFAULT 'cloudinary',
  resource_type text NOT NULL DEFAULT 'image' CHECK (resource_type IN ('image', 'video')),
  object_key text NOT NULL,
  public_url text NOT NULL,
  content_type text NOT NULL,
  width integer CHECK (width IS NULL OR width > 0),
  height integer CHECK (height IS NULL OR height > 0),
  size_bytes bigint CHECK (size_bytes IS NULL OR size_bytes > 0),
  checksum_sha256 text,
  sort_order smallint NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
  moderation_status moderation_status NOT NULL DEFAULT 'queued',
  moderation_labels text[] NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS post_media_post_idx ON post_media (post_id, sort_order);
CREATE INDEX IF NOT EXISTS post_media_moderation_idx ON post_media (moderation_status, created_at);
