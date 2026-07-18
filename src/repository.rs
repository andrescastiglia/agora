use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, types::Json};
use uuid::Uuid;

use crate::whatsapp::{IncomingGroupMessage, IncomingStatus};

#[derive(Debug, sqlx::FromRow)]
pub struct ClaimedEvent {
    pub id: Uuid,
    pub payload: Value,
}

#[derive(Debug, sqlx::FromRow)]
pub struct ClaimedJob {
    pub id: Uuid,
    pub job_type: String,
    pub payload: Value,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SearchHit {
    pub chunk_id: Uuid,
    pub message_id: Uuid,
    pub whatsapp_message_id: String,
    pub sender_name: Option<String>,
    pub source_timestamp: Option<DateTime<Utc>>,
    pub content: String,
    pub score: f64,
}

pub async fn persist_webhook_event(
    db: &PgPool,
    payload: &Value,
    content_sha256: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        INSERT INTO webhook_events (provider, payload, content_sha256)
        VALUES ('whatsapp', $1, $2)
        ON CONFLICT (content_sha256) DO NOTHING
        "#,
    )
    .bind(payload)
    .bind(content_sha256)
    .execute(db)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn claim_webhook_event(db: &PgPool) -> Result<Option<ClaimedEvent>, sqlx::Error> {
    sqlx::query_as(
        r#"
        UPDATE webhook_events
        SET processing_status = 'processing',
            attempts = attempts + 1,
            locked_at = now(),
            last_error = NULL
        WHERE id = (
            SELECT id
            FROM webhook_events
            WHERE attempts < 8
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
        RETURNING id, payload
        "#,
    )
    .fetch_optional(db)
    .await
}

pub async fn complete_webhook_event(db: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE webhook_events
        SET processing_status = 'completed',
            processed_at = now(),
            locked_at = NULL
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

pub async fn persist_group_message(
    db: &PgPool,
    message: &IncomingGroupMessage,
) -> Result<(Uuid, bool), sqlx::Error> {
    let source_timestamp = message
        .timestamp
        .parse::<i64>()
        .ok()
        .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0));
    let raw_text = message.text.as_deref().or_else(|| {
        message
            .document
            .as_ref()
            .and_then(|document| document.caption.as_deref())
    });
    let metadata = serde_json::json!({
        "waba_id": message.waba_id,
        "phone_number_id": message.phone_number_id,
        "reply_to_message_id": message.reply_to_message_id,
    });

    let row: (Uuid, bool) = sqlx::query_as(
        r#"
        WITH inserted AS (
            INSERT INTO messages (
                whatsapp_message_id,
                group_id,
                sender_id,
                sender_name,
                message_type,
                raw_text,
                normalized_text,
                source_timestamp,
                metadata
            )
            VALUES ($1, $2, $3, $4, $5, $6, $6, $7, $8)
            ON CONFLICT (whatsapp_message_id) DO NOTHING
            RETURNING id
        )
        SELECT id, true FROM inserted
        UNION ALL
        SELECT id, false FROM messages
        WHERE whatsapp_message_id = $1
          AND NOT EXISTS (SELECT 1 FROM inserted)
        LIMIT 1
        "#,
    )
    .bind(&message.message_id)
    .bind(&message.group_id)
    .bind(&message.sender_id)
    .bind(&message.sender_name)
    .bind(&message.kind)
    .bind(raw_text)
    .bind(source_timestamp)
    .bind(Json(metadata))
    .fetch_one(db)
    .await?;

    Ok(row)
}

pub async fn persist_document(
    db: &PgPool,
    message_id: Uuid,
    document: &crate::whatsapp::Document,
) -> Result<Uuid, sqlx::Error> {
    let row: (Uuid,) = sqlx::query_as(
        r#"
        INSERT INTO attachments (
            message_id, provider_media_id, filename, mime_type, provider_sha256, caption
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (provider_media_id) DO UPDATE
        SET filename = EXCLUDED.filename,
            mime_type = EXCLUDED.mime_type,
            provider_sha256 = EXCLUDED.provider_sha256,
            caption = EXCLUDED.caption
        RETURNING id
        "#,
    )
    .bind(message_id)
    .bind(&document.media_id)
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
    job_type: &str,
    dedupe_key: &str,
    payload: Value,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        r#"
        INSERT INTO jobs (job_type, dedupe_key, payload)
        VALUES ($1, $2, $3)
        ON CONFLICT (job_type, dedupe_key) DO NOTHING
        "#,
    )
    .bind(job_type)
    .bind(dedupe_key)
    .bind(Json(payload))
    .execute(db)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn claim_job(db: &PgPool) -> Result<Option<ClaimedJob>, sqlx::Error> {
    sqlx::query_as(
        r#"
        UPDATE jobs
        SET status = 'processing',
            attempts = attempts + 1,
            locked_at = now(),
            last_error = NULL
        WHERE id = (
            SELECT id
            FROM jobs
            WHERE attempts < max_attempts
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
        RETURNING id, job_type, payload
        "#,
    )
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

pub async fn attachment_details(
    db: &PgPool,
    id: Uuid,
) -> Result<Option<(Uuid, String, Option<String>, Option<String>)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT message_id, provider_media_id, filename, mime_type FROM attachments WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(db)
    .await
}

pub async fn save_extracted_text(
    db: &PgPool,
    attachment_id: Uuid,
    text: &str,
    content_sha256: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE attachments
        SET extracted_text = $2,
            content_sha256 = $3,
            processing_status = 'completed',
            processed_at = now(),
            last_error = NULL
        WHERE id = $1
        "#,
    )
    .bind(attachment_id)
    .bind(text)
    .bind(content_sha256)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn save_attachment_object_key(
    db: &PgPool,
    attachment_id: Uuid,
    object_key: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE attachments SET object_key = $2 WHERE id = $1")
        .bind(attachment_id)
        .bind(object_key)
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
        sqlx::query(
            r#"
            INSERT INTO document_chunks (
                message_id, chunk_index, content, embedding, embedding_model
            )
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(message_id)
        .bind(index as i32)
        .bind(content)
        .bind(pgvector::Vector::from(embedding.clone()))
        .bind(embedding_model)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(())
}

pub async fn search_group(
    db: &PgPool,
    group_id: &str,
    query: &str,
    embedding: Vec<f32>,
    exclude_message_id: Uuid,
    limit: i64,
) -> Result<Vec<SearchHit>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT
            dc.id AS chunk_id,
            dc.message_id,
            m.whatsapp_message_id,
            m.sender_name,
            m.source_timestamp,
            dc.content,
            (
                0.75 * (1 - (dc.embedding <=> $3))
                + 0.25 * ts_rank_cd(dc.content_tsv, websearch_to_tsquery('spanish', $2))
            )::float8 AS score
        FROM document_chunks dc
        JOIN messages m ON m.id = dc.message_id
        WHERE m.group_id = $1
          AND dc.embedding IS NOT NULL
          AND m.id <> $4
        ORDER BY score DESC
        LIMIT $5
        "#,
    )
    .bind(group_id)
    .bind(query)
    .bind(pgvector::Vector::from(embedding))
    .bind(exclude_message_id)
    .bind(limit)
    .fetch_all(db)
    .await
}

pub async fn create_outgoing_message(
    db: &PgPool,
    source_message_id: Uuid,
    group_id: &str,
    body: &str,
) -> Result<(Uuid, String, Option<String>), sqlx::Error> {
    sqlx::query_as(
        r#"
        INSERT INTO outgoing_messages (source_message_id, group_id, body)
        VALUES ($1, $2, $3)
        ON CONFLICT (source_message_id) WHERE source_message_id IS NOT NULL
        DO UPDATE SET body = EXCLUDED.body
        RETURNING id, status, provider_message_id
        "#,
    )
    .bind(source_message_id)
    .bind(group_id)
    .bind(body)
    .fetch_one(db)
    .await
}

pub async fn mark_outgoing_sent(
    db: &PgPool,
    id: Uuid,
    provider_message_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE outgoing_messages
        SET provider_message_id = $2, status = 'sent', sent_at = now()
        WHERE id = $1
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
    let provider_timestamp = status
        .timestamp
        .parse::<i64>()
        .ok()
        .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0));
    let error = status
        .error
        .as_ref()
        .map(|value| value.to_string().chars().take(1000).collect::<String>());
    let result = sqlx::query(
        r#"
        UPDATE outgoing_messages
        SET status = $2,
            last_error = $3,
            provider_status_at = COALESCE($4, provider_status_at)
        WHERE provider_message_id = $1
          AND (
              provider_status_at IS NULL
              OR $4 IS NULL
              OR provider_status_at <= $4
          )
        "#,
    )
    .bind(&status.provider_message_id)
    .bind(&status.status)
    .bind(error)
    .bind(provider_timestamp)
    .execute(db)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn ping(db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").execute(db).await?;
    Ok(())
}
