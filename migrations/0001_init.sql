CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS citext;

CREATE TYPE account_type AS ENUM ('person', 'ong', 'vet', 'admin');
CREATE TYPE post_type AS ENUM ('adoption', 'lost', 'found', 'emergency', 'campaign', 'post');
CREATE TYPE moderation_status AS ENUM ('queued', 'approved', 'rejected', 'needs_review');

CREATE TABLE users (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  name text NOT NULL,
  email citext UNIQUE,
  password_hash text NOT NULL,
  account_type account_type NOT NULL DEFAULT 'person',
  verified boolean NOT NULL DEFAULT false,
  trust_score smallint NOT NULL DEFAULT 20 CHECK (trust_score BETWEEN 0 AND 100),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE refresh_tokens (
  token text PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  expires_at timestamptz NOT NULL,
  revoked_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX refresh_tokens_user_idx ON refresh_tokens (user_id, expires_at DESC);

CREATE TABLE ong_profiles (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  legal_name text NOT NULL,
  cnpj text UNIQUE,
  mission text,
  city text,
  state text,
  location geography(Point, 4326),
  verified_at timestamptz,`r`n  area_type text,`r`n  contact_phone text,`r`n  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE posts (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  author_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  post_type post_type NOT NULL,
  animal_type text NOT NULL,
  title text,
  description text NOT NULL,
  location geography(Point, 4326),
  neighborhood text,
  urgent boolean NOT NULL DEFAULT false,
  moderation_status moderation_status NOT NULL DEFAULT 'queued',
  fraud_risk smallint NOT NULL DEFAULT 0 CHECK (fraud_risk BETWEEN 0 AND 100),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX posts_location_idx ON posts USING gist(location);
CREATE INDEX posts_feed_idx ON posts (created_at DESC, post_type, urgent);
CREATE INDEX ong_profiles_location_idx ON ong_profiles USING gist(location);

CREATE TABLE post_media (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  post_id uuid NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
  object_key text NOT NULL,
  content_type text NOT NULL,
  moderation_status moderation_status NOT NULL DEFAULT 'queued',
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE chat_rooms (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  post_id uuid REFERENCES posts(id) ON DELETE SET NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE chat_messages (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  room_id uuid NOT NULL REFERENCES chat_rooms(id) ON DELETE CASCADE,
  sender_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  body text NOT NULL CHECK (char_length(body) BETWEEN 1 AND 2000),
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX chat_messages_room_created_idx ON chat_messages (room_id, created_at DESC);

CREATE TABLE donations (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  donor_id uuid REFERENCES users(id) ON DELETE SET NULL,
  ong_id uuid NOT NULL REFERENCES ong_profiles(id) ON DELETE CASCADE,
  amount_cents bigint NOT NULL CHECK (amount_cents > 0),
  currency char(3) NOT NULL DEFAULT 'BRL',
  provider text,
  provider_reference text,
  status text NOT NULL DEFAULT 'pending',
  created_at timestamptz NOT NULL DEFAULT now()
);

