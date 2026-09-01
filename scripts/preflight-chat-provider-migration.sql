-- Run only on databases that have applied 0007 but not 0008.
-- It normalizes legacy states that the old schema allowed but 0008/0009 reject.

BEGIN;

UPDATE messages m
SET group_id = 'legacy'
WHERE m.group_id IS NULL
  AND EXISTS (
      SELECT 1 FROM document_chunks dc WHERE dc.message_id = m.id
  );

UPDATE webhook_events
SET content_sha256 = encode(
    digest(payload::text || ':' || id::text, 'sha256'),
    'hex'
)
WHERE content_sha256 IS NULL;

COMMIT;
