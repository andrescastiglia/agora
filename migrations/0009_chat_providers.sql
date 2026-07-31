ALTER TABLE webhook_events
    ADD COLUMN provider_event_id text;

UPDATE webhook_events
SET provider_event_id = COALESCE(
    content_sha256,
    encode(digest(payload::text, 'sha256'), 'hex')
);

ALTER TABLE webhook_events
    ALTER COLUMN provider_event_id SET NOT NULL;

DROP INDEX IF EXISTS webhook_events_content_sha256_idx;

ALTER TABLE webhook_events
    ADD CONSTRAINT webhook_events_provider_event_unique
    UNIQUE (provider, provider_event_id),
    ADD CONSTRAINT webhook_events_provider_valid
    CHECK (provider IN ('telegram', 'whatsapp'));

CREATE INDEX webhook_events_provider_pending_idx
    ON webhook_events (provider, received_at)
    WHERE processing_status = 'pending';

ALTER TABLE messages
    ADD COLUMN provider text,
    ADD COLUMN external_message_id text,
    ADD COLUMN conversation_id text,
    ADD COLUMN space_id text;

UPDATE messages
SET provider = 'whatsapp',
    external_message_id = COALESCE(whatsapp_message_id, id::text),
    conversation_id = COALESCE(group_id, 'legacy'),
    space_id = 'agora';

ALTER TABLE messages
    ALTER COLUMN provider SET NOT NULL,
    ALTER COLUMN external_message_id SET NOT NULL,
    ALTER COLUMN conversation_id SET NOT NULL,
    ALTER COLUMN space_id SET NOT NULL,
    DROP CONSTRAINT IF EXISTS messages_whatsapp_message_id_key,
    ADD CONSTRAINT messages_provider_valid
        CHECK (provider IN ('telegram', 'whatsapp')),
    ADD CONSTRAINT messages_provider_conversation_external_unique
        UNIQUE (provider, conversation_id, external_message_id);

CREATE INDEX messages_space_source_timestamp_idx
    ON messages (space_id, source_timestamp DESC);

ALTER TABLE attachments
    ADD COLUMN provider text;

UPDATE attachments
SET provider = 'whatsapp';

ALTER TABLE attachments
    ALTER COLUMN provider SET NOT NULL,
    DROP CONSTRAINT IF EXISTS attachments_provider_media_id_key,
    ADD CONSTRAINT attachments_provider_valid
        CHECK (provider IN ('telegram', 'whatsapp')),
    ADD CONSTRAINT attachments_provider_media_unique
        UNIQUE (provider, provider_media_id);

ALTER TABLE jobs
    ADD COLUMN provider text;

UPDATE jobs
SET provider = 'whatsapp';

ALTER TABLE jobs
    ALTER COLUMN provider SET NOT NULL,
    DROP CONSTRAINT IF EXISTS jobs_job_type_dedupe_key_key,
    ADD CONSTRAINT jobs_provider_valid
        CHECK (provider IN ('telegram', 'whatsapp')),
    ADD CONSTRAINT jobs_provider_type_dedupe_unique
        UNIQUE (provider, job_type, dedupe_key);

CREATE INDEX jobs_provider_pending_idx
    ON jobs (provider, next_attempt_at, created_at)
    WHERE status = 'pending';

ALTER TABLE outgoing_messages
    ADD COLUMN provider text;

UPDATE outgoing_messages
SET provider = 'whatsapp';

ALTER TABLE outgoing_messages
    ALTER COLUMN provider SET NOT NULL,
    DROP CONSTRAINT IF EXISTS outgoing_messages_provider_message_id_key,
    ADD CONSTRAINT outgoing_messages_provider_valid
        CHECK (provider IN ('telegram', 'whatsapp')),
    ADD CONSTRAINT outgoing_messages_provider_message_unique
        UNIQUE (provider, provider_message_id);

ALTER TABLE document_chunks
    ADD COLUMN space_id text;

UPDATE document_chunks dc
SET space_id = m.space_id
FROM messages m
WHERE m.id = dc.message_id;

ALTER TABLE document_chunks
    ALTER COLUMN space_id SET NOT NULL;

CREATE INDEX document_chunks_space_id_idx
    ON document_chunks (space_id);
