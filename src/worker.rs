use std::sync::Arc;

use anyhow::Context;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    config::Config,
    document,
    object_storage::MediaStore,
    openai::OpenAiClient,
    repository::{
        apply_outgoing_status, attachment_details, claim_job, claim_webhook_event, complete_job,
        complete_webhook_event, create_outgoing_message, enqueue_job, fail_job, fail_webhook_event,
        mark_outgoing_sent, message_text, persist_document, persist_group_message, replace_chunks,
        save_attachment_object_key, save_extracted_text, search_group,
    },
    security::sha256_hex,
    text::{chunks, source_context},
    whatsapp::{
        WhatsAppClient, parse_group_messages, parse_statuses, question_for_bot, supported_document,
    },
};

pub async fn run(db: PgPool, config: Arc<Config>) {
    loop {
        let result = match process_next_webhook(&db, &config).await {
            Ok(true) => continue,
            Ok(false) => process_next_job(&db, &config).await,
            Err(error) => Err(error),
        };
        match result {
            Ok(true) => continue,
            Ok(false) => {}
            Err(error) => tracing::error!(%error, "worker iteration failed"),
        }
        tokio::time::sleep(config.worker_poll_interval).await;
    }
}

pub async fn process_next_webhook(db: &PgPool, config: &Config) -> Result<bool, anyhow::Error> {
    let Some(event) = claim_webhook_event(db).await? else {
        return Ok(false);
    };

    match process_event(db, config, &event.payload).await {
        Ok(()) => complete_webhook_event(db, event.id).await?,
        Err(error) => {
            fail_webhook_event(db, event.id, &error.to_string()).await?;
            return Err(error);
        }
    }

    Ok(true)
}

pub async fn process_next_job(db: &PgPool, config: &Config) -> Result<bool, anyhow::Error> {
    let Some(job) = claim_job(db).await? else {
        return Ok(false);
    };

    match process_job(db, config, &job.job_type, &job.payload).await {
        Ok(()) => complete_job(db, job.id).await?,
        Err(error) => {
            fail_job(db, job.id, &error.to_string()).await?;
            return Err(error);
        }
    }

    Ok(true)
}

async fn process_event(
    db: &PgPool,
    config: &Config,
    payload: &serde_json::Value,
) -> Result<(), anyhow::Error> {
    for status in parse_statuses(payload)? {
        apply_outgoing_status(db, &status).await?;
    }

    for message in parse_group_messages(payload)? {
        if let Some(expected_group) = config.whatsapp_group_id.as_deref()
            && message.group_id != expected_group
        {
            tracing::warn!(
                group_id = %message.group_id,
                "ignored message from unconfigured WhatsApp group"
            );
            continue;
        }

        let (message_id, inserted) = persist_group_message(db, &message).await?;
        if !inserted {
            continue;
        }

        if let Some(text) = message.text.as_deref() {
            enqueue_job(
                db,
                "embed_message",
                &message.message_id,
                json!({"message_id": message_id}),
            )
            .await?;

            let sender_allowed = config.allowed_whatsapp_ids.is_empty()
                || config.allowed_whatsapp_ids.contains(&message.sender_id);
            if sender_allowed && let Some(question) = question_for_bot(text, &config.bot_mention) {
                enqueue_job(
                    db,
                    "answer_question",
                    &message.message_id,
                    json!({
                        "message_id": message_id,
                        "group_id": message.group_id,
                        "sender_id": message.sender_id,
                        "question": question,
                    }),
                )
                .await?;
            }
        }

        if let Some(document) = message.document.as_ref() {
            if supported_document(document.filename.as_deref(), document.mime_type.as_deref()) {
                let attachment_id = persist_document(db, message_id, document).await?;
                enqueue_job(
                    db,
                    "process_document",
                    &document.media_id,
                    json!({
                        "attachment_id": attachment_id,
                        "message_id": message_id,
                    }),
                )
                .await?;
            } else {
                tracing::warn!(
                    filename = ?document.filename,
                    mime_type = ?document.mime_type,
                    "ignored unsupported WhatsApp document"
                );
            }
        }
    }

    Ok(())
}

async fn process_job(
    db: &PgPool,
    config: &Config,
    job_type: &str,
    payload: &serde_json::Value,
) -> Result<(), anyhow::Error> {
    match job_type {
        "embed_message" => embed_message(db, config, uuid(payload, "message_id")?).await,
        "process_document" => process_document(db, config, uuid(payload, "attachment_id")?).await,
        "answer_question" => answer_question(db, config, payload).await,
        other => anyhow::bail!("unknown job type: {other}"),
    }
}

async fn embed_message(
    db: &PgPool,
    config: &Config,
    message_id: Uuid,
) -> Result<(), anyhow::Error> {
    let text = message_text(db, message_id)
        .await?
        .context("message has no text to embed")?;
    embed_content(db, config, message_id, &text).await
}

async fn embed_content(
    db: &PgPool,
    config: &Config,
    message_id: Uuid,
    content: &str,
) -> Result<(), anyhow::Error> {
    let openai = OpenAiClient::from_config(config)?;
    let mut embedded = Vec::new();
    for chunk in chunks(content, 1_200, 150) {
        let embedding = openai.embedding(&chunk).await?;
        embedded.push((chunk, embedding));
    }
    replace_chunks(db, message_id, &embedded, openai.embedding_model()).await?;
    Ok(())
}

async fn process_document(
    db: &PgPool,
    config: &Config,
    attachment_id: Uuid,
) -> Result<(), anyhow::Error> {
    let (message_id, media_id, filename, _mime_type) = attachment_details(db, attachment_id)
        .await?
        .context("attachment does not exist")?;
    let filename = filename.context("document has no filename")?;
    let whatsapp = WhatsAppClient::from_config(config)?;
    let (bytes, _, _) = whatsapp
        .download_media(&media_id, config.document_max_bytes)
        .await?;
    let content_sha256 = sha256_hex(&bytes);
    let object_key = MediaStore::from_config(config)?
        .put_document(&content_sha256, &filename, &bytes)
        .await?;
    save_attachment_object_key(db, attachment_id, &object_key).await?;
    let text = document::extract(&bytes, &filename).await?;
    save_extracted_text(db, attachment_id, &text, &content_sha256).await?;
    embed_content(db, config, message_id, &text).await
}

async fn answer_question(
    db: &PgPool,
    config: &Config,
    payload: &serde_json::Value,
) -> Result<(), anyhow::Error> {
    let source_message_id = uuid(payload, "message_id")?;
    let group_id = string(payload, "group_id")?;
    let sender_id = string(payload, "sender_id")?;
    let question = string(payload, "question")?;
    let openai = OpenAiClient::from_config(config)?;
    let query_embedding = openai.embedding(question).await?;
    let sources = search_group(
        db,
        group_id,
        question,
        query_embedding,
        source_message_id,
        6,
    )
    .await?;
    let answer = if sources.is_empty() {
        "No encontré información suficiente en la comunidad para responder.".to_owned()
    } else {
        openai
            .answer(question, &source_context(&sources), sender_id)
            .await?
    };
    let answer = truncate(&answer, 4_000);
    let (outgoing_id, status, _) =
        create_outgoing_message(db, source_message_id, group_id, &answer).await?;
    if status == "sent" {
        return Ok(());
    }
    let whatsapp = WhatsAppClient::from_config(config)?;
    let sent = whatsapp.send_group_text(group_id, &answer).await?;
    mark_outgoing_sent(db, outgoing_id, &sent.id).await?;
    Ok(())
}

fn uuid(payload: &serde_json::Value, field: &'static str) -> Result<Uuid, anyhow::Error> {
    payload
        .get(field)
        .and_then(serde_json::Value::as_str)
        .context(format!("job payload is missing {field}"))?
        .parse()
        .with_context(|| format!("job payload has invalid {field}"))
}

fn string<'a>(
    payload: &'a serde_json::Value,
    field: &'static str,
) -> Result<&'a str, anyhow::Error> {
    payload
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .context(format!("job payload is missing {field}"))
}

fn truncate(value: &str, maximum_chars: usize) -> String {
    if value.chars().count() <= maximum_chars {
        value.to_owned()
    } else {
        value
            .chars()
            .take(maximum_chars.saturating_sub(1))
            .chain(std::iter::once('…'))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_required_job_fields() {
        let id = Uuid::new_v4();
        let payload = json!({"message_id": id, "question": "hola"});

        assert_eq!(uuid(&payload, "message_id").unwrap(), id);
        assert_eq!(string(&payload, "question").unwrap(), "hola");
        assert!(uuid(&payload, "missing").is_err());
        assert!(string(&json!({"question": ""}), "question").is_err());
    }

    #[test]
    fn truncates_on_unicode_boundaries() {
        assert_eq!(truncate("áéíóú", 4), "áéí…");
        assert_eq!(truncate("corto", 10), "corto");
    }
}
