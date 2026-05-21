ALTER TABLE ong_profiles
  ADD COLUMN IF NOT EXISTS verification_status text NOT NULL DEFAULT 'PENDING_MANUAL_REVIEW',
  ADD COLUMN IF NOT EXISTS verification_reviewed_at timestamptz,
  ADD COLUMN IF NOT EXISTS verification_reviewer_user_id uuid REFERENCES users(id) ON DELETE SET NULL,
  ADD COLUMN IF NOT EXISTS verification_rejection_reason text,
  ADD COLUMN IF NOT EXISTS updated_at timestamptz NOT NULL DEFAULT now();

ALTER TABLE ong_profiles
  ADD CONSTRAINT ong_profiles_verification_status_check
  CHECK (verification_status IN ('PENDING_MANUAL_REVIEW', 'APPROVED', 'REJECTED', 'BLOCKED'));

CREATE INDEX IF NOT EXISTS ong_profiles_verification_status_idx
  ON ong_profiles (verification_status, created_at DESC);

UPDATE ong_profiles
SET verification_status = 'APPROVED',
    verification_reviewed_at = COALESCE(verified_at, now()),
    updated_at = now()
WHERE verified_at IS NOT NULL
  AND verification_status = 'PENDING_MANUAL_REVIEW';
