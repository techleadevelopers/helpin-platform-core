CREATE TABLE IF NOT EXISTS rescue_final_reports (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  rescue_id uuid REFERENCES rescue_sessions(id) ON DELETE SET NULL,
  post_id uuid NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
  status text NOT NULL,
  summary text NOT NULL,
  public_update text NOT NULL,
  generated_by_ai boolean NOT NULL DEFAULT false,
  publication_status text NOT NULL DEFAULT 'pending_approval',
  rejection_reason text,
  approved_by uuid REFERENCES users(id) ON DELETE SET NULL,
  approved_at timestamptz,
  rejected_by uuid REFERENCES users(id) ON DELETE SET NULL,
  rejected_at timestamptz,
  created_by uuid REFERENCES users(id) ON DELETE SET NULL,
  updated_by uuid REFERENCES users(id) ON DELETE SET NULL,
  admin_notes text,
  ai_model text,
  ai_latency_ms integer,
  ai_cost_cents integer,
  prompt_version text,
  schema_version text NOT NULL DEFAULT '1.0.0',
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  CONSTRAINT rescue_final_reports_status_check CHECK (
    status IN ('rescued', 'not_found', 'died', 'referred', 'cancelled', 'false_alarm')
  ),
  CONSTRAINT rescue_final_reports_publication_status_check CHECK (
    publication_status IN ('draft', 'pending_approval', 'published', 'rejected')
  ),
  CONSTRAINT rescue_final_reports_rejected_reason_check CHECK (
    publication_status <> 'rejected' OR rejection_reason IS NOT NULL
  ),
  CONSTRAINT rescue_final_reports_post_unique UNIQUE (post_id)
);

CREATE INDEX IF NOT EXISTS rescue_final_reports_rescue_id_idx
  ON rescue_final_reports (rescue_id);

CREATE INDEX IF NOT EXISTS rescue_final_reports_post_id_idx
  ON rescue_final_reports (post_id);

CREATE INDEX IF NOT EXISTS rescue_final_reports_publication_status_idx
  ON rescue_final_reports (publication_status);

CREATE INDEX IF NOT EXISTS rescue_final_reports_approved_at_idx
  ON rescue_final_reports (approved_at);
