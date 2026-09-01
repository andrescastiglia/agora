BEGIN;

CREATE TEMP TABLE target_messages ON COMMIT DROP AS
SELECT id
FROM messages
WHERE provider = current_setting('agora.provider')
  AND sender_id = current_setting('agora.participant_id');

CREATE TEMP TABLE target_attachments ON COMMIT DROP AS
SELECT id
FROM attachments
WHERE message_id IN (SELECT id FROM target_messages);

CREATE TEMP TABLE deletion_counts (
    object_type text PRIMARY KEY,
    deleted_count bigint NOT NULL
) ON COMMIT DROP;

WITH deleted AS (
    DELETE FROM jobs
    WHERE provider = current_setting('agora.provider')
      AND (
          payload->>'message_id' IN (SELECT id::text FROM target_messages)
          OR payload->>'attachment_id' IN (SELECT id::text FROM target_attachments)
          OR payload->>'sender_id' = current_setting('agora.participant_id')
      )
    RETURNING 1
)
INSERT INTO deletion_counts SELECT 'jobs', count(*) FROM deleted;

WITH deleted AS (
    DELETE FROM outgoing_messages
    WHERE source_message_id IN (SELECT id FROM target_messages)
    RETURNING 1
)
INSERT INTO deletion_counts SELECT 'outgoing_messages', count(*) FROM deleted;

WITH target_webhooks AS MATERIALIZED (
    SELECT
        we.id,
        CASE
            WHEN we.provider = 'telegram' THEN '{}'::jsonb
            ELSE agora_filter_whatsapp_participant_payload(
                we.payload,
                current_setting('agora.participant_id'),
                false
            )
        END AS filtered_payload
    FROM webhook_events we
    WHERE we.provider = current_setting('agora.provider')
      AND (
          (
              we.provider = 'telegram'
              AND we.payload #>> '{message,from,id}' = current_setting('agora.participant_id')
          )
          OR (
              we.provider = 'whatsapp'
              AND (
                  EXISTS (
                      SELECT 1
                      FROM jsonb_path_query(
                          we.payload,
                          '$.entry[*].changes[*].value.messages[*]'
                      ) message
                      WHERE message->>'from' = current_setting('agora.participant_id')
                  )
                  OR EXISTS (
                      SELECT 1
                      FROM jsonb_path_query(
                          we.payload,
                          '$.entry[*].changes[*].value.contacts[*]'
                      ) contact
                      WHERE contact->>'wa_id' = current_setting('agora.participant_id')
                  )
                  OR EXISTS (
                      SELECT 1
                      FROM jsonb_path_query(
                          we.payload,
                          '$.entry[*].changes[*].value.statuses[*]'
                      ) status
                      WHERE status->>'recipient_id' = current_setting('agora.participant_id')
                  )
                  OR EXISTS (
                      SELECT 1
                      FROM jsonb_path_query(
                          we.payload,
                          '$.entry[*].changes[*].value.groups.**'
                      ) group_value
                      WHERE group_value = to_jsonb(current_setting('agora.participant_id'))
                  )
              )
          )
      )
), minimized AS (
    UPDATE webhook_events we
    SET payload = CASE
        WHEN jsonb_path_exists(
            target_webhooks.filtered_payload,
            '$.entry[*].changes[*].value.messages[*]'
        ) OR jsonb_path_exists(
            target_webhooks.filtered_payload,
            '$.entry[*].changes[*].value.contacts[*]'
        ) OR jsonb_path_exists(
            target_webhooks.filtered_payload,
            '$.entry[*].changes[*].value.statuses[*]'
        ) THEN target_webhooks.filtered_payload
        ELSE '{}'::jsonb
    END
    FROM target_webhooks
    WHERE we.id = target_webhooks.id
    RETURNING 1
)
INSERT INTO deletion_counts SELECT 'webhook_payloads', count(*) FROM minimized;

WITH deleted AS (
    DELETE FROM messages
    WHERE id IN (SELECT id FROM target_messages)
    RETURNING 1
)
INSERT INTO deletion_counts SELECT 'messages', count(*) FROM deleted;

SELECT jsonb_build_object(
    'deleted_at', now(),
    'provider', current_setting('agora.provider'),
    'counts', COALESCE(jsonb_object_agg(object_type, deleted_count), '{}'::jsonb)
)
FROM deletion_counts;

COMMIT;
