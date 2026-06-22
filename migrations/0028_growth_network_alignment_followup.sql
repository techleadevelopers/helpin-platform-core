CREATE TABLE IF NOT EXISTS post_shares (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  post_id uuid NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
  user_id uuid REFERENCES users(id) ON DELETE SET NULL,
  channel text NOT NULL DEFAULT 'system_share',
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS post_shares_post_created_idx
  ON post_shares (post_id, created_at DESC);

CREATE INDEX IF NOT EXISTS post_shares_user_created_idx
  ON post_shares (user_id, created_at DESC)
  WHERE user_id IS NOT NULL;

ALTER TABLE donations ALTER COLUMN ong_id DROP NOT NULL;
ALTER TABLE donations ADD COLUMN IF NOT EXISTS purpose text NOT NULL DEFAULT 'ong_donation';
ALTER TABLE donations ADD COLUMN IF NOT EXISTS recurrence text NOT NULL DEFAULT 'one_time';
ALTER TABLE donations ADD COLUMN IF NOT EXISTS public_message text;
ALTER TABLE donations DROP CONSTRAINT IF EXISTS donations_purpose_check;
ALTER TABLE donations
  ADD CONSTRAINT donations_purpose_check
  CHECK (purpose IN ('ong_donation', 'platform_maintenance'));
ALTER TABLE donations DROP CONSTRAINT IF EXISTS donations_recurrence_check;
ALTER TABLE donations
  ADD CONSTRAINT donations_recurrence_check
  CHECK (recurrence IN ('one_time', 'monthly'));
ALTER TABLE donations DROP CONSTRAINT IF EXISTS donations_platform_maintenance_bounds;
ALTER TABLE donations
  ADD CONSTRAINT donations_platform_maintenance_bounds
  CHECK (
    purpose <> 'platform_maintenance'
    OR (
      ong_id IS NULL
      AND currency = 'BRL'
      AND recurrence = 'monthly'
      AND amount_cents BETWEEN 10 AND 100
    )
  );

CREATE TABLE IF NOT EXISTS volunteer_profiles (
  user_id uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  active boolean NOT NULL DEFAULT true,
  service_radius_km double precision NOT NULL DEFAULT 8 CHECK (service_radius_km BETWEEN 0.3 AND 100),
  animal_scopes text[] NOT NULL DEFAULT ARRAY['general']::text[],
  capabilities text[] NOT NULL DEFAULT '{}'::text[],
  notes text CHECK (notes IS NULL OR char_length(notes) <= 280),
  verified boolean NOT NULL DEFAULT false,
  responses_count bigint NOT NULL DEFAULT 0,
  arrived_count bigint NOT NULL DEFAULT 0,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS volunteer_profiles_active_scopes_idx
  ON volunteer_profiles USING gin (animal_scopes)
  WHERE active = true;

ALTER TABLE push_delivery_jobs ADD COLUMN IF NOT EXISTS provider_response jsonb;
ALTER TABLE push_delivery_jobs ADD COLUMN IF NOT EXISTS provider_ticket_id text;
ALTER TABLE push_delivery_jobs ADD COLUMN IF NOT EXISTS delivered_at timestamptz;
ALTER TABLE push_subscriptions ADD COLUMN IF NOT EXISTS invalidated_at timestamptz;
ALTER TABLE push_subscriptions ADD COLUMN IF NOT EXISTS last_delivery_error text;
