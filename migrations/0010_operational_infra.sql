CREATE TABLE IF NOT EXISTS push_delivery_jobs (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  notification_event_id uuid REFERENCES notification_events(id) ON DELETE CASCADE,
  user_id uuid REFERENCES users(id) ON DELETE CASCADE,
  push_token text NOT NULL,
  platform text NOT NULL,
  payload jsonb NOT NULL,
  status text NOT NULL DEFAULT 'queued' CHECK (status IN ('queued', 'sent', 'failed', 'dead_letter')),
  attempts integer NOT NULL DEFAULT 0,
  next_attempt_at timestamptz NOT NULL DEFAULT now(),
  last_error text,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS push_delivery_jobs_status_next_idx
  ON push_delivery_jobs (status, next_attempt_at);

CREATE TABLE IF NOT EXISTS payment_webhook_events (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  provider text NOT NULL,
  provider_event_id text NOT NULL,
  event_type text NOT NULL,
  donation_id uuid REFERENCES donations(id) ON DELETE SET NULL,
  payload jsonb NOT NULL,
  processed_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (provider, provider_event_id)
);
CREATE INDEX IF NOT EXISTS payment_webhook_events_donation_idx
  ON payment_webhook_events (donation_id, created_at DESC);

CREATE TABLE IF NOT EXISTS ong_kyb_documents (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  ong_id uuid NOT NULL REFERENCES ong_profiles(id) ON DELETE CASCADE,
  document_type text NOT NULL,
  object_key text NOT NULL,
  public_url text NOT NULL,
  status text NOT NULL DEFAULT 'pending_review',
  reviewer_user_id uuid REFERENCES users(id) ON DELETE SET NULL,
  rejection_reason text,
  created_at timestamptz NOT NULL DEFAULT now(),
  reviewed_at timestamptz
);
CREATE INDEX IF NOT EXISTS ong_kyb_documents_ong_status_idx
  ON ong_kyb_documents (ong_id, status, created_at DESC);
