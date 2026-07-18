DROP INDEX IF EXISTS webhook_events_content_sha256_idx;

CREATE UNIQUE INDEX webhook_events_content_sha256_idx
    ON webhook_events (content_sha256);
