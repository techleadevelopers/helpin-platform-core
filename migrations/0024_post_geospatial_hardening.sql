ALTER TABLE posts
  ADD COLUMN IF NOT EXISTS geo_status text NOT NULL DEFAULT 'unavailable',
  ADD COLUMN IF NOT EXISTS geo_source text,
  ADD COLUMN IF NOT EXISTS geo_provider text,
  ADD COLUMN IF NOT EXISTS geo_confidence double precision,
  ADD COLUMN IF NOT EXISTS geo_resolved_at timestamptz,
  ADD COLUMN IF NOT EXISTS route_public boolean NOT NULL DEFAULT false;

ALTER TABLE posts DROP CONSTRAINT IF EXISTS posts_geo_status_check;
ALTER TABLE posts
  ADD CONSTRAINT posts_geo_status_check
  CHECK (geo_status IN ('unavailable', 'pending', 'confirmed', 'failed'));

ALTER TABLE posts DROP CONSTRAINT IF EXISTS posts_geo_source_check;
ALTER TABLE posts
  ADD CONSTRAINT posts_geo_source_check
  CHECK (geo_source IS NULL OR geo_source IN ('gps_confirmed', 'address_geocoded'));

CREATE TABLE IF NOT EXISTS post_geocode_jobs (
  post_id uuid PRIMARY KEY REFERENCES posts(id) ON DELETE CASCADE,
  address_label text NOT NULL,
  status text NOT NULL DEFAULT 'pending'
    CHECK (status IN ('pending', 'processing', 'completed', 'failed')),
  attempts integer NOT NULL DEFAULT 0,
  last_error text,
  next_run_at timestamptz NOT NULL DEFAULT now(),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS post_geocode_jobs_due_idx
  ON post_geocode_jobs (status, next_run_at)
  WHERE status IN ('pending', 'processing');
