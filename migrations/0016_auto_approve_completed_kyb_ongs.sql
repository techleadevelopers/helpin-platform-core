WITH ready_ongs AS (
  SELECT op.id, op.user_id
  FROM ong_profiles op
  WHERE op.verification_status <> 'APPROVED'
    AND (
      SELECT count(DISTINCT d.document_type)
      FROM ong_kyb_documents d
      WHERE d.ong_id = op.id
        AND d.status = 'approved'
        AND d.document_type IN ('document_front', 'document_back', 'selfie_with_document')
    ) >= 3
),
approved AS (
  UPDATE ong_profiles op
  SET verification_status = 'APPROVED',
      verification_reviewed_at = COALESCE(verification_reviewed_at, now()),
      verification_rejection_reason = NULL,
      verified_at = COALESCE(verified_at, now()),
      updated_at = now()
  FROM ready_ongs ready
  WHERE op.id = ready.id
  RETURNING ready.user_id
)
UPDATE users u
SET verified = true
FROM approved
WHERE u.id = approved.user_id;
