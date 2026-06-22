ALTER TABLE push_delivery_jobs
  DROP CONSTRAINT IF EXISTS push_delivery_jobs_status_check;

UPDATE push_delivery_jobs
SET status = 'provider_accepted'
WHERE status = 'sent';

ALTER TABLE push_delivery_jobs
  ADD CONSTRAINT push_delivery_jobs_status_check
  CHECK (status IN ('queued', 'provider_accepted', 'delivered', 'failed', 'dead_letter'));

ALTER TABLE push_delivery_jobs
  ADD COLUMN IF NOT EXISTS provider_accepted_at timestamptz;

ALTER TABLE push_delivery_jobs
  ADD COLUMN IF NOT EXISTS receipt_status text;

ALTER TABLE push_delivery_jobs
  ADD COLUMN IF NOT EXISTS receipt_checked_at timestamptz;

ALTER TABLE push_delivery_jobs
  ADD COLUMN IF NOT EXISTS receipt_response jsonb;

CREATE INDEX IF NOT EXISTS push_delivery_jobs_receipt_due_idx
  ON push_delivery_jobs (status, provider_accepted_at)
  WHERE status = 'provider_accepted' AND provider_ticket_id IS NOT NULL;
