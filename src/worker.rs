use std::{str::FromStr, sync::Arc};

use anyhow::Context;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    chat::{
        ChatClient, ChatProvider, ProviderClient, TELEGRAM_MAX_DOCUMENT_BYTES, parse_event,
        question_for_bot, supported_document,
    },
    config::Config,
    document,
    openai::OpenAiClient,
    repository::{
        apply_outgoing_status, attachment_details, claim_job, claim_webhook_event, complete_job,
        complete_webhook_event, create_outgoing_message, enqueue_job, fail_job, fail_webhook_event,
        mark_outgoing_sent, message_text, persist_document, persist_message, replace_chunks,
        save_attachment_original, save_extracted_text, search_space,
    },
    security::sha256_hex,
    text::{chunks, source_context},
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
    let Some(event) = claim_webhook_event(db, config.chat_provider).await? else {
        return Ok(false);
    };
    let provider = provider(&event.provider)?;
    match process_event(db, config, provider, &event.payload).await {
        Ok(()) => complete_webhook_event(db, event.id).await?,
        Err(error) => {
            fail_webhook_event(db, event.id, &error.to_string()).await?;
            return Err(error);
        }
    }
    Ok(true)
}

pub async fn process_next_job(db: &PgPool, config: &Config) -> Result<bool, anyhow::Error> {
    let Some(job) = claim_job(db, config.chat_provider).await? else {
        return Ok(false);
    };
    let provider = provider(&job.provider)?;
    match process_job(db, config, provider, &job.job_type, &job.payload).await {
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
    provider: ChatProvider,
    payload: &serde_json::Value,
) -> Result<(), anyhow::Error> {
    let event = parse_event(provider, payload, &config.knowledge_space_id)?;
    for status in event.statuses {
        apply_outgoing_status(db, &status).await?;
    }

    for message in event.messages {
        if Some(message.conversation_id.as_str()) != expected_conversation(config, provider) {
            tracing::warn!(provider = %provider, "ignored message from unconfigured conversation");
            continue;
        }
        if !sender_is_allowed(config, provider, &message.sender_id) {
            tracing::warn!(provider = %provider, "ignored message from sender outside allowlist");
            continue;
        }

        let (message_id, _) = persist_message(db, &message).await?;
        if let Some(text) = message.effective_text() {
            enqueue_job(
                db,
                provider,
                "embed_message",
                &message.external_message_id,
                json!({"message_id": message_id}),
            )
            .await?;

            if let Some(question) = question_for_bot(provider, text, config) {
                enqueue_job(
                    db,
                    provider,
                    "answer_question",
                    &message.external_message_id,
                    json!({
                        "message_id": message_id,
                        "conversation_id": message.conversation_id,
                        "space_id": message.space_id,
                        "sender_id": message.sender_id,
                        "reply_to_message_id": message.external_message_id,
                        "question": question,
                    }),
                )
                .await?;
            }
        }

        if let Some(document) = message.document.as_ref() {
            let maximum = provider_document_limit(config, provider);
            if document.file_size.is_some_and(|size| size > maximum) {
                tracing::warn!(provider = %provider, "ignored document above provider limit");
                continue;
            }
            if supported_document(document.filename.as_deref(), document.mime_type.as_deref()) {
                let attachment_id = persist_document(db, provider, message_id, document).await?;
                enqueue_job(
                    db,
                    provider,
                    "process_document",
                    &document.provider_media_id,
                    json!({
                        "attachment_id": attachment_id,
                        "message_id": message_id,
                    }),
                )
                .await?;
            } else {
                tracing::warn!(provider = %provider, "ignored unsupported document");
            }
        }
    }
    Ok(())
}

fn expected_conversation(config: &Config, provider: ChatProvider) -> Option<&str> {
    match provider {
        ChatProvider::Telegram => config.telegram_group_id.as_deref(),
        ChatProvider::WhatsApp => config.whatsapp_group_id.as_deref(),
    }
}

fn sender_is_allowed(config: &Config, provider: ChatProvider, sender_id: &str) -> bool {
    let allowed = match provider {
        ChatProvider::Telegram => &config.telegram_allowed_user_ids,
        ChatProvider::WhatsApp => &config.whatsapp_allowed_user_ids,
    };
    allowed.iter().any(|candidate| candidate == sender_id)
}

fn provider_document_limit(config: &Config, provider: ChatProvider) -> u64 {
    match provider {
        ChatProvider::Telegram => TELEGRAM_MAX_DOCUMENT_BYTES,
        ChatProvider::WhatsApp => config.document_max_bytes,
    }
}

async fn process_job(
    db: &PgPool,
    config: &Config,
    provider: ChatProvider,
    job_type: &str,
    payload: &serde_json::Value,
) -> Result<(), anyhow::Error> {
    match job_type {
        "embed_message" => embed_message(db, config, uuid(payload, "message_id")?).await,
        "process_document" => {
            process_document(db, config, provider, uuid(payload, "attachment_id")?).await
        }
        "answer_question" => answer_question(db, config, provider, payload).await,
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
    job_provider: ChatProvider,
    attachment_id: Uuid,
) -> Result<(), anyhow::Error> {
    let attachment = attachment_details(db, attachment_id)
        .await?
        .context("attachment does not exist")?;
    let attachment_provider = provider(&attachment.provider)?;
    if attachment_provider != job_provider {
        anyhow::bail!("attachment provider does not match persisted job provider");
    }
    let filename = attachment.filename.context("document has no filename")?;
    let bytes = if let Some(bytes) = attachment.original_data {
        bytes
    } else {
        let client = ProviderClient::from_config(job_provider, config)?;
        let downloaded = client
            .download_document(
                &attachment.provider_media_id,
                provider_document_limit(config, job_provider),
            )
            .await?;
        let content_sha256 = sha256_hex(&downloaded.bytes);
        save_attachment_original(db, attachment_id, &content_sha256, &downloaded.bytes).await?;
        downloaded.bytes
    };
    let text = document::extract(&bytes, &filename).await?;
    save_extracted_text(db, attachment_id, &text).await?;
    embed_content(db, config, attachment.message_id, &text).await
}

async fn answer_question(
    db: &PgPool,
    config: &Config,
    provider: ChatProvider,
    payload: &serde_json::Value,
) -> Result<(), anyhow::Error> {
    let source_message_id = uuid(payload, "message_id")?;
    let conversation_id = string(payload, "conversation_id")?;
    let space_id = string(payload, "space_id")?;
    let sender_id = string(payload, "sender_id")?;
    let reply_to_message_id = string(payload, "reply_to_message_id")?;
    let question = string(payload, "question")?;
    let openai = OpenAiClient::from_config(config)?;
    let query_embedding = openai.embedding(question).await?;
    let sources = search_space(
        db,
        space_id,
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
            .answer(
                question,
                &source_context(&sources),
                &format!("{provider}:{sender_id}"),
            )
            .await?
    };
    let answer = truncate(&answer, provider.message_limit());
    let (outgoing_id, status, provider_message_id) =
        create_outgoing_message(db, provider, source_message_id, conversation_id, &answer).await?;
    if outgoing_already_sent(&status, provider_message_id.as_deref()) {
        return Ok(());
    }
    let client = ProviderClient::from_config(provider, config)?;
    let sent = client
        .send_text(conversation_id, &answer, Some(reply_to_message_id))
        .await?;
    mark_outgoing_sent(db, outgoing_id, &sent.external_message_id).await?;
    Ok(())
}

fn outgoing_already_sent(status: &str, provider_message_id: Option<&str>) -> bool {
    provider_message_id.is_some() || matches!(status, "sent" | "delivered" | "read")
}

fn provider(value: &str) -> Result<ChatProvider, anyhow::Error> {
    ChatProvider::from_str(value).map_err(anyhow::Error::msg)
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
    use std::collections::HashMap;

    use serde_json::json;
    use sqlx::{Connection, PgConnection, postgres::PgPoolOptions};

    use super::*;

    fn worker_config(provider: &str) -> Config {
        Config::from_map(HashMap::from([
            ("DATABASE_URL".into(), "postgres://localhost/agora".into()),
            ("KNOWLEDGE_SPACE_ID".into(), "agora".into()),
            ("CHAT_PROVIDER".into(), provider.into()),
            ("TELEGRAM_BOT_TOKEN".into(), "telegram-token".into()),
            ("TELEGRAM_WEBHOOK_SECRET".into(), "telegram-secret".into()),
            ("TELEGRAM_GROUP_ID".into(), "-1001".into()),
            ("TELEGRAM_ALLOWED_USER_IDS".into(), "42".into()),
            ("TELEGRAM_BOT_USERNAME".into(), "agora_bot".into()),
            ("WHATSAPP_VERIFY_TOKEN".into(), "verify".into()),
            ("WHATSAPP_APP_SECRET".into(), "secret".into()),
            ("WHATSAPP_ACCESS_TOKEN".into(), "wa-token".into()),
            ("WHATSAPP_PHONE_NUMBER_ID".into(), "phone".into()),
            ("WHATSAPP_WABA_ID".into(), "waba".into()),
            ("WHATSAPP_GROUP_ID".into(), "group-test".into()),
            ("WHATSAPP_ALLOWED_USER_IDS".into(), "allowed-sender".into()),
        ]))
        .unwrap()
    }

    fn whatsapp_text(message_id: &str, sender_id: &str, group_id: &str) -> serde_json::Value {
        json!({
            "object": "whatsapp_business_account",
            "entry": [{
                "id": "waba-test",
                "changes": [{
                    "field": "messages",
                    "value": {"messages": [{
                        "from": sender_id,
                        "group_id": group_id,
                        "id": message_id,
                        "timestamp": "1700000000",
                        "type": "text",
                        "text": {"body": "@agora pregunta"}
                    }]}
                }]
            }]
        })
    }

    fn telegram_text(update_id: i64, sender_id: i64, group_id: i64) -> serde_json::Value {
        json!({
            "update_id": update_id,
            "message": {
                "message_id": update_id,
                "date": 1700000000,
                "chat": {"id": group_id, "type": "supergroup"},
                "from": {"id": sender_id, "first_name": "Ana"},
                "text": "/agora pregunta"
            }
        })
    }

    #[test]
    fn parses_job_fields_providers_limits_and_sent_states() {
        let id = Uuid::new_v4();
        let payload = json!({"message_id": id, "question": "hola"});
        assert_eq!(uuid(&payload, "message_id").unwrap(), id);
        assert_eq!(string(&payload, "question").unwrap(), "hola");
        assert_eq!(provider("telegram").unwrap(), ChatProvider::Telegram);
        assert!(provider("signal").is_err());
        assert_eq!(
            provider_document_limit(&worker_config("telegram"), ChatProvider::Telegram),
            TELEGRAM_MAX_DOCUMENT_BYTES
        );
        assert!(outgoing_already_sent("pending", Some("1")));
        assert!(outgoing_already_sent("delivered", None));
        assert!(!outgoing_already_sent("pending", None));
        assert_eq!(truncate("áéíóú", 4), "áéí…");
    }

    #[test]
    fn checks_provider_specific_allowlists() {
        let config = worker_config("telegram");
        assert!(sender_is_allowed(&config, ChatProvider::Telegram, "42"));
        assert!(!sender_is_allowed(&config, ChatProvider::Telegram, "43"));
        assert!(sender_is_allowed(
            &config,
            ChatProvider::WhatsApp,
            "allowed-sender"
        ));
    }

    #[tokio::test]
    async fn rejects_wrong_conversations_and_senders_before_database_access() {
        let db = PgPoolOptions::new()
            .connect_lazy("postgres://localhost/agora")
            .unwrap();
        let config = worker_config("telegram");
        process_event(
            &db,
            &config,
            ChatProvider::Telegram,
            &telegram_text(1, 42, -9999),
        )
        .await
        .unwrap();
        process_event(
            &db,
            &config,
            ChatProvider::Telegram,
            &telegram_text(2, 99, -1001),
        )
        .await
        .unwrap();
        process_event(
            &db,
            &config,
            ChatProvider::WhatsApp,
            &whatsapp_text("wamid.wrong", "allowed-sender", "wrong-group"),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn persists_only_authorized_content_idempotently() {
        let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
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
        let allowed_id = format!("wamid.allowed.{suffix}");
        let config = worker_config("whatsapp");

        process_event(
            &db,
            &config,
            ChatProvider::WhatsApp,
            &whatsapp_text(&allowed_id, "allowed-sender", "group-test"),
        )
        .await
        .unwrap();
        process_event(
            &db,
            &config,
            ChatProvider::WhatsApp,
            &whatsapp_text(&allowed_id, "allowed-sender", "group-test"),
        )
        .await
        .unwrap();
        let message_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM messages WHERE provider = 'whatsapp' AND external_message_id = $1",
        )
        .bind(&allowed_id)
        .fetch_one(&db)
        .await
        .unwrap();
        let job_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM jobs WHERE provider = 'whatsapp' AND dedupe_key = $1",
        )
        .bind(&allowed_id)
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(message_count, 1);
        assert_eq!(job_count, 2);

        let telegram_config = worker_config("telegram");
        let telegram_update = telegram_text(9_000_000, 42, -1001);
        process_event(
            &db,
            &telegram_config,
            ChatProvider::Telegram,
            &telegram_update,
        )
        .await
        .unwrap();
        process_event(
            &db,
            &telegram_config,
            ChatProvider::Telegram,
            &telegram_update,
        )
        .await
        .unwrap();
        let telegram_messages: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM messages WHERE provider = 'telegram' AND external_message_id = '9000000'",
        )
        .fetch_one(&db)
        .await
        .unwrap();
        let telegram_jobs: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM jobs WHERE provider = 'telegram' AND dedupe_key = '9000000'",
        )
        .fetch_one(&db)
        .await
        .unwrap();
        assert_eq!(telegram_messages, 1);
        assert_eq!(telegram_jobs, 2);
    }
}
