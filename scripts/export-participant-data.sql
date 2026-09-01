WITH participant_messages AS MATERIALIZED (
    SELECT *
    FROM messages
    WHERE provider = current_setting('agora.provider')
      AND sender_id = current_setting('agora.participant_id')
),
participant_attachments AS MATERIALIZED (
    SELECT a.*
    FROM attachments a
    JOIN participant_messages m ON m.id = a.message_id
),
participant_chunks AS MATERIALIZED (
    SELECT dc.*
    FROM document_chunks dc
    JOIN participant_messages m ON m.id = dc.message_id
),
participant_jobs AS MATERIALIZED (
    SELECT j.*
    FROM jobs j
    WHERE j.provider = current_setting('agora.provider')
      AND (
          j.payload->>'message_id' IN (SELECT id::text FROM participant_messages)
          OR j.payload->>'attachment_id' IN (SELECT id::text FROM participant_attachments)
          OR j.payload->>'sender_id' = current_setting('agora.participant_id')
      )
),
participant_outgoing AS MATERIALIZED (
    SELECT o.*
    FROM outgoing_messages o
    JOIN participant_messages m ON m.id = o.source_message_id
),
participant_webhooks AS MATERIALIZED (
    SELECT
        we.*,
        CASE
            WHEN we.provider = 'whatsapp' THEN
                agora_filter_whatsapp_participant_payload(
                    we.payload,
                    current_setting('agora.participant_id'),
                    true
                )
            ELSE we.payload
        END AS export_payload
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
              )
          )
      )
)
SELECT jsonb_pretty(jsonb_build_object(
    'exported_at', now(),
    'provider', current_setting('agora.provider'),
    'participant_id', current_setting('agora.participant_id'),
    'messages', COALESCE((
        SELECT jsonb_agg(to_jsonb(m) ORDER BY m.created_at) FROM participant_messages m
    ), '[]'::jsonb),
    'attachments', COALESCE((
        SELECT jsonb_agg(
            (to_jsonb(a) - 'original_data') || jsonb_build_object(
                'original_data_base64',
                CASE WHEN a.original_data IS NULL THEN NULL ELSE encode(a.original_data, 'base64') END
            )
            ORDER BY a.created_at
        )
        FROM participant_attachments a
    ), '[]'::jsonb),
    'document_chunks', COALESCE((
        SELECT jsonb_agg(to_jsonb(dc) - 'embedding' ORDER BY dc.created_at)
        FROM participant_chunks dc
    ), '[]'::jsonb),
    'jobs', COALESCE((
        SELECT jsonb_agg(to_jsonb(j) ORDER BY j.created_at) FROM participant_jobs j
    ), '[]'::jsonb),
    'outgoing_messages', COALESCE((
        SELECT jsonb_agg(to_jsonb(o) ORDER BY o.created_at) FROM participant_outgoing o
    ), '[]'::jsonb),
    'pending_webhook_events', COALESCE((
        SELECT jsonb_agg(
            (to_jsonb(we) - 'payload' - 'export_payload') ||
                jsonb_build_object('payload', we.export_payload)
            ORDER BY we.received_at
        )
        FROM participant_webhooks we
    ), '[]'::jsonb)
));
