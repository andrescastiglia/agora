CREATE UNIQUE INDEX IF NOT EXISTS outgoing_messages_source_message_idx
    ON outgoing_messages (source_message_id)
    WHERE source_message_id IS NOT NULL;
