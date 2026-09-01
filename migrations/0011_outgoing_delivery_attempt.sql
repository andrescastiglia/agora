ALTER TABLE outgoing_messages
    ADD COLUMN delivery_attempted_at timestamptz;

CREATE INDEX outgoing_messages_manual_review_idx
    ON outgoing_messages (status, delivery_attempted_at)
    WHERE status IN ('sending', 'delivery_unknown');
