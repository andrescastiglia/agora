use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, types::Json};
use uuid::Uuid;

use crate::chat::{ChatProvider, IncomingDocument, IncomingMessage, IncomingStatus};

#[derive(Debug, sqlx::FromRow)]
pub struct ClaimedEvent {
    pub id: Uuid,
    pub provider: String,
    pub payload: Value,
}

#[derive(Debug, sqlx::FromRow)]
pub struct ClaimedJob {
    pub id: Uuid,
    pub provider: String,
    pub job_type: String,
    pub payload: Value,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SearchHit {
    pub chunk_id: Uuid,
    pub message_id: Uuid,
    pub external_message_id: String,
    pub sender_name: Option<String>,
    pub source_timestamp: Option<DateTime<Utc>>,
    pub content: String,
    pub score: f64,
}

pub async fn persist_webhook_event(
    db: &PgPool,
    provider: ChatProvider,
    provider_event_id: &str,
    payload: &Value,
    content_sha256: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        INSERT INTO webhook_events (provider, provider_event_id, payload, content_sha256)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (provider, provider_event_id) DO NOTHING
        "#,
    )
    .bind(provider.as_str())
    .bind(provider_event_id)
    .bind(payload)
    .bind(content_sha256)
    .execute(db)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn claim_webhook_event(
    db: &PgPool,
    provider: ChatProvider,
) -> Result<Option<ClaimedEvent>, sqlx::Error> {
    sqlx::query_as(
        r#"
        WITH dead_lettered AS (
            UPDATE webhook_events
            SET processing_status = 'dead',
                locked_at = NULL,
                payload = '{}'::jsonb,
                last_error = COALESCE(
                    last_error,
                    'worker stopped during the final processing attempt'
                )
            WHERE processing_status = 'processing'
              AND provider = $1
              AND locked_at <= now() - interval '15 minutes'
              AND attempts >= 8
        )
        UPDATE webhook_events
        SET processing_status = 'processing',
            attempts = attempts + 1,
            locked_at = now(),
            last_error = NULL
        WHERE id = (
            SELECT id
            FROM webhook_events
            WHERE provider = $1
              AND attempts < 8
              AND (
                  (processing_status = 'pending' AND next_attempt_at <= now())
                  OR (
                      processing_status = 'processing'
                      AND locked_at <= now() - interval '15 minutes'
                  )
              )
            ORDER BY received_at
            FOR UPDATE SKIP LOCKED
            LIMIT 1
        )
        RETURNING id, provider, payload
        "#,
    )
    .bind(provider.as_str())
    .fetch_optional(db)
    .await
}

pub async fn complete_webhook_event(db: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE webhook_events
        SET processing_status = 'completed',
            processed_at = now(),
            locked_at = NULL,
            payload = '{}'::jsonb
        WHERE id = $1
        "#,
    )
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn fail_webhook_event(db: &PgPool, id: Uuid, error: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE webhook_events
        SET processing_status = CASE WHEN attempts >= 8 THEN 'dead' ELSE 'pending' END,
            next_attempt_at = now() + make_interval(secs => LEAST(300, power(2, attempts)::integer)),
            last_error = left($2, 1000),
            locked_at = NULL,
            payload = CASE WHEN attempts >= 8 THEN '{}'::jsonb ELSE payload END
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(error)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn persist_message(
    db: &PgPool,
    message: &IncomingMessage,
) -> Result<(Uuid, bool), sqlx::Error> {
    let raw_text = message.effective_text();
    let mut metadata = message.metadata.clone();
    metadata["reply_to_message_id"] = message
        .reply_to_message_id
        .as_ref()
        .map_or(Value::Null, |value| Value::String(value.clone()));

    let row: (Uuid, bool) = sqlx::query_as(
        r#"
        WITH inserted AS (
            INSERT INTO messages (
                whatsapp_message_id,
                group_id,
                provider,
                external_message_id,
                conversation_id,
                space_id,
                sender_id,
                sender_name,
                message_type,
                raw_text,
                normalized_text,
                source_timestamp,
                metadata
            )
            VALUES (
                CASE WHEN $1 = 'whatsapp' THEN $2 ELSE NULL END,
                $3, $1, $2, $3, $4, $5, $6, $7, $8, $8, $9, $10
            )
            ON CONFLICT (provider, conversation_id, external_message_id) DO NOTHING
            RETURNING id
        )
        SELECT id, true FROM inserted
        UNION ALL
        SELECT id, false FROM messages
        WHERE provider = $1
          AND conversation_id = $3
          AND external_message_id = $2
          AND NOT EXISTS (SELECT 1 FROM inserted)
        LIMIT 1
        "#,
    )
    .bind(message.provider.as_str())
    .bind(&message.external_message_id)
    .bind(&message.conversation_id)
    .bind(&message.space_id)
    .bind(&message.sender_id)
    .bind(&message.sender_name)
    .bind(&message.kind)
    .bind(raw_text)
    .bind(message.timestamp)
    .bind(Json(metadata))
    .fetch_one(db)
    .await?;

    Ok(row)
}

pub async fn persist_document(
    db: &PgPool,
    provider: ChatProvider,
    message_id: Uuid,
    document: &IncomingDocument,
) -> Result<Uuid, sqlx::Error> {
    let row: (Uuid,) = sqlx::query_as(
        r#"
        INSERT INTO attachments (
            provider, message_id, provider_media_id, filename, mime_type, provider_sha256, caption
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (provider, message_id, provider_media_id) DO UPDATE
        SET filename = EXCLUDED.filename,
            mime_type = EXCLUDED.mime_type,
            provider_sha256 = EXCLUDED.provider_sha256,
            caption = EXCLUDED.caption
        RETURNING id
        "#,
    )
    .bind(provider.as_str())
    .bind(message_id)
    .bind(&document.provider_media_id)
    .bind(&document.filename)
    .bind(&document.mime_type)
    .bind(&document.sha256)
    .bind(&document.caption)
    .fetch_one(db)
    .await?;

    Ok(row.0)
}

pub async fn enqueue_job(
    db: &PgPool,
    provider: ChatProvider,
    job_type: &str,
    dedupe_key: &str,
    payload: Value,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        INSERT INTO jobs (provider, job_type, dedupe_key, payload)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (provider, job_type, dedupe_key) DO NOTHING
        "#,
    )
    .bind(provider.as_str())
    .bind(job_type)
    .bind(dedupe_key)
    .bind(Json(payload))
    .execute(db)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn claim_job(
    db: &PgPool,
    provider: ChatProvider,
) -> Result<Option<ClaimedJob>, sqlx::Error> {
    sqlx::query_as(
        r#"
        WITH dead_lettered AS (
            UPDATE jobs
            SET status = 'dead',
                locked_at = NULL,
                last_error = COALESCE(
                    last_error,
                    'worker stopped during the final processing attempt'
                )
            WHERE status = 'processing'
              AND provider = $1
              AND locked_at <= now() - interval '15 minutes'
              AND attempts >= max_attempts
        )
        UPDATE jobs
        SET status = 'processing',
            attempts = attempts + 1,
            locked_at = now(),
            last_error = NULL
        WHERE id = (
            SELECT id
            FROM jobs
            WHERE provider = $1
              AND attempts < max_attempts
              AND (
                  (status = 'pending' AND next_attempt_at <= now())
                  OR (
                      status = 'processing'
                      AND locked_at <= now() - interval '15 minutes'
                  )
              )
            ORDER BY created_at
            FOR UPDATE SKIP LOCKED
            LIMIT 1
        )
        RETURNING id, provider, job_type, payload
        "#,
    )
    .bind(provider.as_str())
    .fetch_optional(db)
    .await
}

pub async fn complete_job(db: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE jobs
        SET status = 'completed', completed_at = now(), locked_at = NULL
        WHERE id = $1
        "#,
    )
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn fail_job(db: &PgPool, id: Uuid, error: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE jobs
        SET status = CASE WHEN attempts >= max_attempts THEN 'dead' ELSE 'pending' END,
            next_attempt_at = now() + make_interval(secs => LEAST(900, power(2, attempts)::integer)),
            last_error = left($2, 1000),
            locked_at = NULL
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(error)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn message_text(db: &PgPool, id: Uuid) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT normalized_text FROM messages WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await
        .map(Option::flatten)
}

#[derive(Debug, sqlx::FromRow)]
pub struct AttachmentDetails {
    pub provider: String,
    pub message_id: Uuid,
    pub provider_media_id: String,
    pub filename: Option<String>,
    pub caption: Option<String>,
    pub original_data: Option<Vec<u8>>,
}

pub async fn attachment_details(
    db: &PgPool,
    id: Uuid,
) -> Result<Option<AttachmentDetails>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT provider, message_id, provider_media_id, filename, caption, original_data
        FROM attachments
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(db)
    .await
}

pub async fn message_has_attachment(db: &PgPool, message_id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM attachments WHERE message_id = $1)")
        .bind(message_id)
        .fetch_one(db)
        .await
}

pub async fn message_attachments_completed(
    db: &PgPool,
    message_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT NOT EXISTS(
            SELECT 1
            FROM attachments
            WHERE message_id = $1 AND processing_status <> 'completed'
        )
        "#,
    )
    .bind(message_id)
    .fetch_one(db)
    .await
}

pub async fn complete_document_indexing(
    db: &PgPool,
    attachment_id: Uuid,
    message_id: Uuid,
    text: &str,
    chunks: &[(String, Vec<f32>)],
    embedding_model: &str,
) -> Result<(), sqlx::Error> {
    let mut transaction = db.begin().await?;
    sqlx::query("DELETE FROM document_chunks WHERE message_id = $1")
        .bind(message_id)
        .execute(&mut *transaction)
        .await?;
    for (index, (content, embedding)) in chunks.iter().enumerate() {
        let result = sqlx::query(
            r#"
            INSERT INTO document_chunks (
                message_id, group_id, space_id, chunk_index, content, embedding, embedding_model
            )
            SELECT $1, m.conversation_id, m.space_id, $2, $3, $4, $5
            FROM messages m
            WHERE m.id = $1
            "#,
        )
        .bind(message_id)
        .bind(index as i32)
        .bind(content)
        .bind(pgvector::Vector::from(embedding.clone()))
        .bind(embedding_model)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            return Err(sqlx::Error::RowNotFound);
        }
    }
    let result = sqlx::query(
        r#"
        UPDATE attachments
        SET extracted_text = $2,
            processing_status = 'completed',
            processed_at = now(),
            last_error = NULL
        WHERE id = $1
        "#,
    )
    .bind(attachment_id)
    .bind(text)
    .execute(&mut *transaction)
    .await?;
    if result.rows_affected() != 1 {
        return Err(sqlx::Error::RowNotFound);
    }
    transaction.commit().await?;
    Ok(())
}

pub async fn save_attachment_original(
    db: &PgPool,
    attachment_id: Uuid,
    content_sha256: &str,
    original_data: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE attachments
        SET content_sha256 = $2,
            original_data = $3
        WHERE id = $1
        "#,
    )
    .bind(attachment_id)
    .bind(content_sha256)
    .bind(original_data)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn replace_chunks(
    db: &PgPool,
    message_id: Uuid,
    chunks: &[(String, Vec<f32>)],
    embedding_model: &str,
) -> Result<(), sqlx::Error> {
    let mut transaction = db.begin().await?;
    sqlx::query("DELETE FROM document_chunks WHERE message_id = $1")
        .bind(message_id)
        .execute(&mut *transaction)
        .await?;
    for (index, (content, embedding)) in chunks.iter().enumerate() {
        let result = sqlx::query(
            r#"
            INSERT INTO document_chunks (
                message_id, group_id, space_id, chunk_index, content, embedding, embedding_model
            )
            SELECT $1, m.conversation_id, m.space_id, $2, $3, $4, $5
            FROM messages m
            WHERE m.id = $1
            "#,
        )
        .bind(message_id)
        .bind(index as i32)
        .bind(content)
        .bind(pgvector::Vector::from(embedding.clone()))
        .bind(embedding_model)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            return Err(sqlx::Error::RowNotFound);
        }
    }
    transaction.commit().await?;
    Ok(())
}

pub async fn search_space(
    db: &PgPool,
    space_id: &str,
    query: &str,
    embedding: Vec<f32>,
    exclude_message_id: Uuid,
    limit: i64,
) -> Result<Vec<SearchHit>, sqlx::Error> {
    let candidate_limit = limit.saturating_mul(5).max(1);
    sqlx::query_as(
        r#"
        WITH search_query AS MATERIALIZED (
            SELECT websearch_to_tsquery('spanish', $2) AS value
        ),
        vector_candidates AS MATERIALIZED (
            SELECT dc.id
            FROM document_chunks dc
            WHERE dc.space_id = $1
              AND dc.embedding IS NOT NULL
              AND dc.message_id <> $4
            ORDER BY dc.embedding <=> $3
            LIMIT $6
        ),
        text_candidates AS MATERIALIZED (
            SELECT dc.id
            FROM document_chunks dc
            CROSS JOIN search_query sq
            WHERE dc.space_id = $1
              AND dc.embedding IS NOT NULL
              AND dc.message_id <> $4
              AND dc.content_tsv @@ sq.value
            ORDER BY ts_rank_cd(dc.content_tsv, sq.value) DESC
            LIMIT $6
        ),
        candidates AS (
            SELECT id FROM vector_candidates
            UNION
            SELECT id FROM text_candidates
        )
        SELECT
            dc.id AS chunk_id,
            dc.message_id,
            m.external_message_id,
            m.sender_name,
            m.source_timestamp,
            dc.content,
            (
                0.75 * (1 - (dc.embedding <=> $3))
                + 0.25 * ts_rank_cd(dc.content_tsv, sq.value)
            )::float8 AS score
        FROM candidates c
        JOIN document_chunks dc ON dc.id = c.id
        JOIN messages m ON m.id = dc.message_id
        CROSS JOIN search_query sq
        ORDER BY score DESC
        LIMIT $5
        "#,
    )
    .bind(space_id)
    .bind(query)
    .bind(pgvector::Vector::from(embedding))
    .bind(exclude_message_id)
    .bind(limit)
    .bind(candidate_limit)
    .fetch_all(db)
    .await
}

pub async fn create_outgoing_message(
    db: &PgPool,
    provider: ChatProvider,
    source_message_id: Uuid,
    conversation_id: &str,
    body: &str,
) -> Result<(Uuid, String, Option<String>), sqlx::Error> {
    sqlx::query_as(
        r#"
        INSERT INTO outgoing_messages (provider, source_message_id, group_id, body)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (source_message_id) WHERE source_message_id IS NOT NULL
        DO UPDATE SET body = outgoing_messages.body
        WHERE outgoing_messages.provider = EXCLUDED.provider
        RETURNING id, status, provider_message_id
        "#,
    )
    .bind(provider.as_str())
    .bind(source_message_id)
    .bind(conversation_id)
    .bind(body)
    .fetch_one(db)
    .await
}

pub async fn mark_outgoing_sending(db: &PgPool, id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE outgoing_messages
        SET status = 'sending', delivery_attempted_at = now(), last_error = NULL
        WHERE id = $1 AND status = 'pending'
        "#,
    )
    .bind(id)
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn mark_outgoing_delivery_unknown(
    db: &PgPool,
    id: Uuid,
    error: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE outgoing_messages
        SET status = 'delivery_unknown', last_error = left($2, 1000)
        WHERE id = $1 AND status = 'sending'
        "#,
    )
    .bind(id)
    .bind(error)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn mark_outgoing_sent(
    db: &PgPool,
    id: Uuid,
    provider_message_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE outgoing_messages
        SET provider_message_id = $2,
            status = 'sent',
            sent_at = now(),
            last_error = NULL
        WHERE id = $1 AND status = 'sending'
        "#,
    )
    .bind(id)
    .bind(provider_message_id)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn apply_outgoing_status(
    db: &PgPool,
    status: &IncomingStatus,
) -> Result<bool, sqlx::Error> {
    let error = status
        .error
        .as_ref()
        .map(|value| value.to_string().chars().take(1000).collect::<String>());
    let result = sqlx::query(
        r#"
        UPDATE outgoing_messages
        SET status = $3,
            last_error = $4,
            provider_status_at = COALESCE($5, provider_status_at)
        WHERE provider = $1
          AND provider_message_id = $2
          AND CASE $3
              WHEN 'failed' THEN 1
              WHEN 'sent' THEN 2
              WHEN 'delivered' THEN 3
              WHEN 'read' THEN 4
              ELSE -1
          END >= 0
          AND (
              CASE $3
                  WHEN 'failed' THEN 1
                  WHEN 'sent' THEN 2
                  WHEN 'delivered' THEN 3
                  WHEN 'read' THEN 4
                  ELSE -1
              END > CASE status
                  WHEN 'pending' THEN 0
                  WHEN 'sending' THEN 1
                  WHEN 'delivery_unknown' THEN 1
                  WHEN 'failed' THEN 1
                  WHEN 'sent' THEN 2
                  WHEN 'delivered' THEN 3
                  WHEN 'read' THEN 4
                  ELSE -1
              END
              OR (
                  CASE $3
                      WHEN 'failed' THEN 1
                      WHEN 'sent' THEN 2
                      WHEN 'delivered' THEN 3
                      WHEN 'read' THEN 4
                      ELSE -1
                  END = CASE status
                      WHEN 'pending' THEN 0
                      WHEN 'sending' THEN 1
                      WHEN 'delivery_unknown' THEN 1
                      WHEN 'failed' THEN 1
                      WHEN 'sent' THEN 2
                      WHEN 'delivered' THEN 3
                      WHEN 'read' THEN 4
                      ELSE -1
                  END
                  AND $5 IS NOT NULL
                  AND (provider_status_at IS NULL OR provider_status_at < $5)
              )
          )
        "#,
    )
    .bind(status.provider.as_str())
    .bind(&status.provider_message_id)
    .bind(&status.status)
    .bind(error)
    .bind(status.timestamp)
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn ping(db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").execute(db).await?;
    Ok(())
}
