CREATE TABLE IF NOT EXISTS rescue_specialist_providers (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id uuid REFERENCES users(id) ON DELETE SET NULL,
  name text NOT NULL,
  provider_type text NOT NULL CHECK (
    provider_type IN (
      'ong',
      'cetas',
      'ibama',
      'environmental_police',
      'fire_department',
      'vet',
      'rural_rescue',
      'wildlife_rescue',
      'marine_rescue',
      'independent'
    )
  ),
  animal_scopes text[] NOT NULL DEFAULT ARRAY['general']::text[],
  city text,
  state_code text,
  lat double precision CHECK (lat IS NULL OR lat BETWEEN -90 AND 90),
  lng double precision CHECK (lng IS NULL OR lng BETWEEN -180 AND 180),
  service_radius_km numeric NOT NULL DEFAULT 50 CHECK (service_radius_km BETWEEN 1 AND 1000),
  phone text,
  email text,
  verified boolean NOT NULL DEFAULT false,
  active boolean NOT NULL DEFAULT true,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

ALTER TABLE rescue_fanout_states
  DROP CONSTRAINT IF EXISTS rescue_fanout_states_current_phase_check;

ALTER TABLE rescue_fanout_states
  ADD CONSTRAINT rescue_fanout_states_current_phase_check
  CHECK (current_phase BETWEEN 1 AND 20);

ALTER TABLE rescue_fanout_attempts
  DROP CONSTRAINT IF EXISTS rescue_fanout_attempts_phase_check;

ALTER TABLE rescue_fanout_attempts
  ADD CONSTRAINT rescue_fanout_attempts_phase_check
  CHECK (phase BETWEEN 1 AND 20);

CREATE INDEX IF NOT EXISTS rescue_specialist_providers_location_idx
  ON rescue_specialist_providers (lat, lng)
  WHERE active = true AND lat IS NOT NULL AND lng IS NOT NULL;

CREATE INDEX IF NOT EXISTS rescue_specialist_providers_state_scope_idx
  ON rescue_specialist_providers USING gin (animal_scopes);

CREATE INDEX IF NOT EXISTS rescue_specialist_providers_user_idx
  ON rescue_specialist_providers (user_id)
  WHERE user_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS rescue_escalation_attempts (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  fanout_state_id uuid NOT NULL REFERENCES rescue_fanout_states(id) ON DELETE CASCADE,
  post_id uuid NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
  phase int NOT NULL,
  strategy text NOT NULL,
  radius_km numeric NOT NULL,
  candidate_count int NOT NULL DEFAULT 0,
  contacted_count int NOT NULL DEFAULT 0,
  animal_scopes text[] NOT NULL DEFAULT '{}',
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS rescue_escalation_attempts_post_created_idx
  ON rescue_escalation_attempts (post_id, created_at DESC);

CREATE INDEX IF NOT EXISTS rescue_escalation_attempts_state_created_idx
  ON rescue_escalation_attempts (fanout_state_id, created_at DESC);
