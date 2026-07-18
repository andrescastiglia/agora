CREATE INDEX IF NOT EXISTS webhook_events_processing_locked_idx
    ON webhook_events (locked_at)
    WHERE processing_status = 'processing';

CREATE INDEX IF NOT EXISTS jobs_processing_locked_idx
    ON jobs (locked_at)
    WHERE status = 'processing';
