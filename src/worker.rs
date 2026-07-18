use std::sync::Arc;

use anyhow::Context;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    config::Config,
    document,
    openai::OpenAiClient,
    repository::{
        apply_outgoing_status, attachment_details, claim_job, claim_webhook_event, complete_job,
        complete_webhook_event, create_outgoing_message, enqueue_job, fail_job, fail_webhook_event,
        mark_outgoing_sent, message_text, persist_document, persist_group_message, replace_chunks,
        save_attachment_original, save_extracted_text, search_group,
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
        let Some(expected_group) = config.whatsapp_group_id.as_deref() else {
            tracing::warn!("ignored WhatsApp group message because no group is configured");
            continue;
        };
        if message.group_id != expected_group {
            tracing::warn!(
                group_id = %message.group_id,
                "ignored message from unconfigured WhatsApp group"
            );
            continue;
        }

        if !sender_is_allowed(config, &message.sender_id) {
            tracing::warn!("ignored message from sender outside the WhatsApp allowlist");
            continue;
        }

        let (message_id, _) = persist_group_message(db, &message).await?;

        if let Some(text) = message.text.as_deref() {
            enqueue_job(
                db,
                "embed_message",
                &message.message_id,
                json!({"message_id": message_id}),
            )
            .await?;

            if let Some(question) = question_for_bot(text, &config.bot_mention) {
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

fn sender_is_allowed(config: &Config, sender_id: &str) -> bool {
    config.allowed_whatsapp_ids.is_empty()
        || config
            .allowed_whatsapp_ids
            .iter()
            .any(|allowed| allowed == sender_id)
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
    let attachment = attachment_details(db, attachment_id)
        .await?
        .context("attachment does not exist")?;
    let filename = attachment.filename.context("document has no filename")?;
    let bytes = if let Some(bytes) = attachment.original_data {
        bytes
    } else {
        let whatsapp = WhatsAppClient::from_config(config)?;
        let (bytes, _, _) = whatsapp
            .download_media(&attachment.provider_media_id, config.document_max_bytes)
            .await?;
        let content_sha256 = sha256_hex(&bytes);
        save_attachment_original(db, attachment_id, &content_sha256, &bytes).await?;
        bytes
    };
    let text = document::extract(&bytes, &filename).await?;
    save_extracted_text(db, attachment_id, &text).await?;
    embed_content(db, config, attachment.message_id, &text).await
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
    let (outgoing_id, status, provider_message_id) =
        create_outgoing_message(db, source_message_id, group_id, &answer).await?;
    if outgoing_already_sent(&status, provider_message_id.as_deref()) {
        return Ok(());
    }
    let whatsapp = WhatsAppClient::from_config(config)?;
    let sent = whatsapp.send_group_text(group_id, &answer).await?;
    mark_outgoing_sent(db, outgoing_id, &sent.id).await?;
    Ok(())
}

fn outgoing_already_sent(status: &str, provider_message_id: Option<&str>) -> bool {
    provider_message_id.is_some() || matches!(status, "sent" | "delivered" | "read")
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
    use std::{collections::HashMap, env};

    use serde_json::json;
    use sqlx::{Connection, PgConnection, postgres::PgPoolOptions};

    use super::*;

    fn worker_config(allowed_ids: &str) -> Config {
        Config::from_map(HashMap::from([
            ("DATABASE_URL".into(), "postgres://localhost/agora".into()),
            ("WHATSAPP_VERIFY_TOKEN".into(), "verify".into()),
            ("WHATSAPP_APP_SECRET".into(), "secret".into()),
            ("WHATSAPP_GROUP_ID".into(), "group-test".into()),
            ("ALLOWED_WHATSAPP_IDS".into(), allowed_ids.into()),
        ]))
        .unwrap()
    }

    fn group_text(message_id: &str, sender_id: &str, text: &str) -> serde_json::Value {
        json!({
            "object": "whatsapp_business_account",
            "entry": [{
                "id": "waba-test",
                "changes": [{
                    "field": "messages",
                    "value": {
                        "messages": [{
                            "from": sender_id,
                            "group_id": "group-test",
                            "id": message_id,
                            "timestamp": "1700000000",
                            "type": "text",
                            "text": {"body": text}
                        }]
                    }
                }]
            }]
        })
    }

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

    #[test]
    fn checks_the_sender_allowlist() {
        assert!(sender_is_allowed(&worker_config(""), "any-sender"));
        let restricted = worker_config("sender-1,sender-2");
        assert!(sender_is_allowed(&restricted, "sender-2"));
        assert!(!sender_is_allowed(&restricted, "sender-3"));
    }

    #[test]
    fn recognizes_outgoing_messages_that_must_not_be_resent() {
        assert!(outgoing_already_sent("pending", Some("wamid.out")));
        assert!(outgoing_already_sent("sent", None));
        assert!(outgoing_already_sent("delivered", None));
        assert!(outgoing_already_sent("read", None));
        assert!(!outgoing_already_sent("pending", None));
        assert!(!outgoing_already_sent("failed", None));
    }

    #[tokio::test]
    async fn ignores_group_messages_until_a_group_is_configured() {
        let mut config = worker_config("");
        config.whatsapp_group_id = None;
        let db = PgPoolOptions::new()
            .connect_lazy("postgres://localhost/agora")
            .unwrap();

        process_event(
            &db,
            &config,
            &group_text("wamid.unconfigured", "sender", "contenido"),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn rejects_unauthorized_content_and_reenqueues_missing_jobs() {
        let Ok(database_url) = env::var("TEST_DATABASE_URL") else {
            eprintln!("TEST_DATABASE_URL is not set; worker integration test skipped");
            return;
        };
        let mut database_lock = PgConnection::connect(&database_url).await.unwrap();
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(4_242_421_234_i64)
            .execute(&mut database_lock)
            .await
            .unwrap();
        let db = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .unwrap();
        sqlx::migrate!().run(&db).await.unwrap();
        let suffix = Uuid::new_v4();
        let denied_id = format!("wamid.denied.{suffix}");
        let allowed_id = format!("wamid.allowed.{suffix}");
        let config = worker_config("allowed-sender");

        process_event(
            &db,
            &config,
            &group_text(&denied_id, "denied-sender", "contenido privado"),
        )
        .await
        .unwrap();
        let denied_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM messages WHERE whatsapp_message_id = $1")
                .bind(&denied_id)
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(denied_count, 0);

        let parsed = parse_group_messages(&group_text(
            &allowed_id,
            "allowed-sender",
            "@agora: pregunta",
        ))
        .unwrap();
        persist_group_message(&db, &parsed[0]).await.unwrap();
        process_event(
            &db,
            &config,
            &group_text(&allowed_id, "allowed-sender", "@agora: pregunta"),
        )
        .await
        .unwrap();
        let job_count: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE dedupe_key = $1")
            .bind(&allowed_id)
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(job_count, 2);

        process_event(
            &db,
            &config,
            &group_text(&allowed_id, "allowed-sender", "@agora: pregunta"),
        )
        .await
        .unwrap();
        let job_count: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE dedupe_key = $1")
            .bind(&allowed_id)
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(job_count, 2);
    }
}
