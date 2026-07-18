ALTER TABLE webhook_events
    ADD COLUMN IF NOT EXISTS content_sha256 text,
    ADD COLUMN IF NOT EXISTS attempts integer NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS next_attempt_at timestamptz NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS locked_at timestamptz,
    ADD COLUMN IF NOT EXISTS last_error text;

CREATE UNIQUE INDEX IF NOT EXISTS webhook_events_content_sha256_idx
    ON webhook_events (content_sha256)
    WHERE content_sha256 IS NOT NULL;

ALTER TABLE messages
    ADD COLUMN IF NOT EXISTS sender_name text,
    ADD COLUMN IF NOT EXISTS source_timestamp timestamptz;

CREATE INDEX IF NOT EXISTS messages_group_source_timestamp_idx
    ON messages (group_id, source_timestamp DESC);

CREATE TABLE IF NOT EXISTS attachments (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    message_id uuid NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    provider_media_id text NOT NULL UNIQUE,
    filename text,
    mime_type text,
    provider_sha256 text,
    content_sha256 text,
    caption text,
    object_key text,
    extracted_text text,
    processing_status text NOT NULL DEFAULT 'pending',
    last_error text,
    created_at timestamptz NOT NULL DEFAULT now(),
    processed_at timestamptz
);

CREATE TABLE IF NOT EXISTS jobs (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    job_type text NOT NULL,
    dedupe_key text NOT NULL,
    payload jsonb NOT NULL DEFAULT '{}'::jsonb,
    status text NOT NULL DEFAULT 'pending',
    attempts integer NOT NULL DEFAULT 0,
    max_attempts integer NOT NULL DEFAULT 8,
    next_attempt_at timestamptz NOT NULL DEFAULT now(),
    locked_at timestamptz,
    last_error text,
    created_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz,
    UNIQUE (job_type, dedupe_key)
);

CREATE INDEX IF NOT EXISTS jobs_pending_idx
    ON jobs (next_attempt_at, created_at)
    WHERE status = 'pending';

CREATE TABLE IF NOT EXISTS outgoing_messages (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    source_message_id uuid REFERENCES messages(id) ON DELETE SET NULL,
    group_id text NOT NULL,
    provider_message_id text UNIQUE,
    body text NOT NULL,
    status text NOT NULL DEFAULT 'pending',
    last_error text,
    created_at timestamptz NOT NULL DEFAULT now(),
    sent_at timestamptz
);

ALTER TABLE document_chunks
    ADD COLUMN IF NOT EXISTS embedding_model text,
    ADD COLUMN IF NOT EXISTS content_tsv tsvector
        GENERATED ALWAYS AS (to_tsvector('spanish', content)) STORED;

CREATE INDEX IF NOT EXISTS document_chunks_content_tsv_idx
    ON document_chunks USING gin (content_tsv);
