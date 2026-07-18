ALTER TABLE attachments
    ADD COLUMN IF NOT EXISTS original_data bytea;

ALTER TABLE attachments
    DROP COLUMN IF EXISTS object_key;

ALTER TABLE attachments
    DROP CONSTRAINT IF EXISTS attachments_original_data_size;

ALTER TABLE attachments
    ADD CONSTRAINT attachments_original_data_size
    CHECK (
        original_data IS NULL
        OR octet_length(original_data) <= 26214400
    );
