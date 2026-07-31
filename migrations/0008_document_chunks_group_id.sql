ALTER TABLE document_chunks
    ADD COLUMN group_id text;

UPDATE document_chunks dc
SET group_id = m.group_id
FROM messages m
WHERE m.id = dc.message_id;

ALTER TABLE document_chunks
    ALTER COLUMN group_id SET NOT NULL;

CREATE INDEX document_chunks_group_id_idx
    ON document_chunks (group_id);
