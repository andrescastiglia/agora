ALTER TABLE attachments
    DROP CONSTRAINT IF EXISTS attachments_provider_media_unique,
    ADD CONSTRAINT attachments_provider_message_media_unique
        UNIQUE (provider, message_id, provider_media_id);
