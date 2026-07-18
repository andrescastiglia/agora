ALTER TABLE outgoing_messages
    ADD COLUMN IF NOT EXISTS provider_status_at timestamptz;

CREATE INDEX IF NOT EXISTS outgoing_messages_provider_status_idx
    ON outgoing_messages (provider_message_id, provider_status_at)
    WHERE provider_message_id IS NOT NULL;
