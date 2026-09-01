use std::{env, sync::OnceLock};

use agora::{
    chat::{ChatProvider, IncomingDocument, IncomingMessage, IncomingStatus},
    repository::{
        apply_outgoing_status, attachment_details, claim_job, claim_webhook_event,
        complete_document_indexing, complete_job, complete_webhook_event, create_outgoing_message,
        enqueue_job, fail_job, fail_webhook_event, mark_outgoing_delivery_unknown,
        mark_outgoing_sending, mark_outgoing_sent, persist_document, persist_message,
        persist_webhook_event, ping, replace_chunks, save_attachment_original, search_space,
    },
};
use chrono::{TimeZone, Utc};
use serde_json::json;
use sqlx::{AssertSqlSafe, Connection, PgConnection, PgPool, postgres::PgPoolOptions};
use tokio::sync::{Mutex, MutexGuard};
use uuid::Uuid;

static DATABASE_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

async fn database() -> Option<(PgPool, MutexGuard<'static, ()>)> {
    let database_url = env::var("TEST_DATABASE_URL").ok()?;
    let database_lock = DATABASE_MUTEX.get_or_init(|| Mutex::new(())).lock().await;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await
        .expect("test database must be reachable");
    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("test migrations must succeed");
    sqlx::query(
        "TRUNCATE outgoing_messages, jobs, attachments, document_chunks, messages, webhook_events",
    )
    .execute(&pool)
    .await
    .expect("test database must be resettable");
    Some((pool, database_lock))
}

fn message(
    provider: ChatProvider,
    external_id: &str,
    conversation_id: &str,
    text: Option<&str>,
) -> IncomingMessage {
    IncomingMessage {
        provider,
        external_message_id: external_id.into(),
        conversation_id: conversation_id.into(),
        space_id: "agora".into(),
        sender_id: "sender-test".into(),
        sender_name: Some("Ana".into()),
        kind: if text.is_some() { "text" } else { "document" }.into(),
        text: text.map(str::to_owned),
        document: None,
        timestamp: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
        reply_to_message_id: None,
        metadata: json!({}),
    }
}

#[tokio::test]
async fn repository_is_provider_safe_and_shares_knowledge_by_space() {
    let Some((db, _database_lock)) = database().await else {
        eprintln!("TEST_DATABASE_URL is not set; repository integration test skipped");
        return;
    };
    ping(&db).await.unwrap();

    let payload = json!({"event": "same-external-id"});
    assert!(
        persist_webhook_event(&db, ChatProvider::WhatsApp, "7", &payload, "hash-wa")
            .await
            .unwrap()
    );
    assert!(
        persist_webhook_event(&db, ChatProvider::Telegram, "7", &payload, "hash-tg")
            .await
            .unwrap()
    );
    assert!(
        !persist_webhook_event(
            &db,
            ChatProvider::Telegram,
            "7",
            &payload,
            "hash-tg-duplicate"
        )
        .await
        .unwrap()
    );

    let whatsapp_event = claim_webhook_event(&db, ChatProvider::WhatsApp)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(whatsapp_event.provider, "whatsapp");
    complete_webhook_event(&db, whatsapp_event.id)
        .await
        .unwrap();
    let minimized_payload: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM webhook_events WHERE id = $1")
            .bind(whatsapp_event.id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(minimized_payload, json!({}));
    assert!(
        claim_webhook_event(&db, ChatProvider::WhatsApp)
            .await
            .unwrap()
            .is_none()
    );
    let telegram_event = claim_webhook_event(&db, ChatProvider::Telegram)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(telegram_event.provider, "telegram");
    complete_webhook_event(&db, telegram_event.id)
        .await
        .unwrap();

    let whatsapp = message(
        ChatProvider::WhatsApp,
        "same-message",
        "wa-group",
        Some("La reunión es el viernes."),
    );
    let telegram = message(
        ChatProvider::Telegram,
        "same-message",
        "tg-group",
        Some("¿Cuándo es la reunión?"),
    );
    let (whatsapp_id, inserted) = persist_message(&db, &whatsapp).await.unwrap();
    assert!(inserted);
    assert!(!persist_message(&db, &whatsapp).await.unwrap().1);
    let (telegram_id, inserted) = persist_message(&db, &telegram).await.unwrap();
    assert!(inserted);

    let document = IncomingDocument {
        provider_media_id: "same-file".into(),
        filename: Some("informe.pdf".into()),
        mime_type: Some("application/pdf".into()),
        sha256: None,
        file_size: Some(4),
        caption: Some("Informe".into()),
    };
    let whatsapp_attachment = persist_document(&db, ChatProvider::WhatsApp, whatsapp_id, &document)
        .await
        .unwrap();
    let telegram_attachment = persist_document(&db, ChatProvider::Telegram, telegram_id, &document)
        .await
        .unwrap();
    assert_ne!(whatsapp_attachment, telegram_attachment);

    let forwarded = message(
        ChatProvider::Telegram,
        "forwarded-message",
        "tg-group",
        None,
    );
    let (forwarded_id, inserted) = persist_message(&db, &forwarded).await.unwrap();
    assert!(inserted);
    let forwarded_attachment =
        persist_document(&db, ChatProvider::Telegram, forwarded_id, &document)
            .await
            .unwrap();
    assert_ne!(telegram_attachment, forwarded_attachment);
    assert_eq!(
        attachment_details(&db, forwarded_attachment)
            .await
            .unwrap()
            .unwrap()
            .message_id,
        forwarded_id
    );
    assert_eq!(
        persist_document(&db, ChatProvider::Telegram, forwarded_id, &document)
            .await
            .unwrap(),
        forwarded_attachment
    );

    save_attachment_original(&db, telegram_attachment, "content-hash", b"data")
        .await
        .unwrap();
    let vector = vec![0.1_f32; 1536];
    complete_document_indexing(
        &db,
        telegram_attachment,
        telegram_id,
        "contenido extraído",
        &[("contenido extraído".into(), vector.clone())],
        "text-embedding-3-small",
    )
    .await
    .unwrap();
    let stored = attachment_details(&db, telegram_attachment)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.provider, "telegram");
    assert_eq!(stored.original_data.as_deref(), Some(b"data".as_slice()));

    assert!(
        enqueue_job(
            &db,
            ChatProvider::WhatsApp,
            "embed_message",
            "same-message",
            json!({"message_id": whatsapp_id}),
        )
        .await
        .unwrap()
    );
    assert!(
        enqueue_job(
            &db,
            ChatProvider::Telegram,
            "embed_message",
            "same-message",
            json!({"message_id": telegram_id}),
        )
        .await
        .unwrap()
    );
    let whatsapp_job = claim_job(&db, ChatProvider::WhatsApp)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(whatsapp_job.provider, "whatsapp");
    complete_job(&db, whatsapp_job.id).await.unwrap();
    assert!(
        claim_job(&db, ChatProvider::WhatsApp)
            .await
            .unwrap()
            .is_none()
    );
    let telegram_job = claim_job(&db, ChatProvider::Telegram)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(telegram_job.provider, "telegram");
    complete_job(&db, telegram_job.id).await.unwrap();

    replace_chunks(
        &db,
        whatsapp_id,
        &[("La reunión es el viernes.".into(), vector.clone())],
        "text-embedding-3-small",
    )
    .await
    .unwrap();
    let hits = search_space(&db, "agora", "reunión viernes", vector, telegram_id, 5)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].message_id, whatsapp_id);
    assert_eq!(hits[0].external_message_id, "same-message");

    let (wa_outgoing, _, _) = create_outgoing_message(
        &db,
        ChatProvider::WhatsApp,
        whatsapp_id,
        "wa-group",
        "Respuesta",
    )
    .await
    .unwrap();
    let (tg_outgoing, _, _) = create_outgoing_message(
        &db,
        ChatProvider::Telegram,
        telegram_id,
        "tg-group",
        "Respuesta",
    )
    .await
    .unwrap();
    assert!(mark_outgoing_sending(&db, wa_outgoing).await.unwrap());
    assert!(mark_outgoing_sending(&db, tg_outgoing).await.unwrap());
    mark_outgoing_sent(&db, wa_outgoing, "same-outgoing")
        .await
        .unwrap();
    mark_outgoing_sent(&db, tg_outgoing, "same-outgoing")
        .await
        .unwrap();
    assert!(
        apply_outgoing_status(
            &db,
            &IncomingStatus {
                provider: ChatProvider::WhatsApp,
                provider_message_id: "same-outgoing".into(),
                status: "read".into(),
                timestamp: Some(Utc.timestamp_opt(1_700_000_100, 0).unwrap()),
                recipient_id: Some("wa-group".into()),
                recipient_type: Some("group".into()),
                error: None,
            },
        )
        .await
        .unwrap()
    );
    let states: Vec<(String, String)> =
        sqlx::query_as("SELECT provider, status FROM outgoing_messages ORDER BY provider")
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(
        states,
        vec![
            ("telegram".into(), "sent".into()),
            ("whatsapp".into(), "read".into()),
        ]
    );
}

fn incoming_status(
    provider_message_id: &str,
    status: &str,
    timestamp: Option<i64>,
) -> IncomingStatus {
    IncomingStatus {
        provider: ChatProvider::WhatsApp,
        provider_message_id: provider_message_id.into(),
        status: status.into(),
        timestamp: timestamp.and_then(|seconds| Utc.timestamp_opt(seconds, 0).single()),
        recipient_id: Some("wa-group".into()),
        recipient_type: Some("group".into()),
        error: None,
    }
}

#[tokio::test]
async fn retries_dead_letters_and_claims_jobs_without_collisions() {
    let Some((db, _database_lock)) = database().await else {
        eprintln!("TEST_DATABASE_URL is not set; reliability integration test skipped");
        return;
    };

    assert!(
        enqueue_job(
            &db,
            ChatProvider::Telegram,
            "test_retry",
            "retry",
            json!({"value": 1}),
        )
        .await
        .unwrap()
    );
    let retry = claim_job(&db, ChatProvider::Telegram)
        .await
        .unwrap()
        .unwrap();
    fail_job(&db, retry.id, "temporary failure").await.unwrap();
    let retry_state: (String, i32, String) =
        sqlx::query_as("SELECT status, attempts, last_error FROM jobs WHERE id = $1")
            .bind(retry.id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(
        retry_state,
        ("pending".into(), 1, "temporary failure".into())
    );

    sqlx::query(
        "UPDATE jobs SET status = 'processing', attempts = max_attempts, locked_at = now() - interval '16 minutes' WHERE id = $1",
    )
    .bind(retry.id)
    .execute(&db)
    .await
    .unwrap();
    assert!(
        claim_job(&db, ChatProvider::Telegram)
            .await
            .unwrap()
            .is_none()
    );
    let dead_status: String = sqlx::query_scalar("SELECT status FROM jobs WHERE id = $1")
        .bind(retry.id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(dead_status, "dead");

    for key in ["concurrent-a", "concurrent-b"] {
        assert!(
            enqueue_job(&db, ChatProvider::WhatsApp, "concurrent", key, json!({}),)
                .await
                .unwrap()
        );
    }
    let (first, second) = tokio::join!(
        claim_job(&db, ChatProvider::WhatsApp),
        claim_job(&db, ChatProvider::WhatsApp)
    );
    let first = first.unwrap().unwrap();
    let second = second.unwrap().unwrap();
    assert_ne!(first.id, second.id);

    sqlx::query(
        "UPDATE jobs SET status = 'processing', attempts = 1, locked_at = now() - interval '16 minutes' WHERE id = $1",
    )
    .bind(first.id)
    .execute(&db)
    .await
    .unwrap();
    complete_job(&db, second.id).await.unwrap();
    let reclaimed = claim_job(&db, ChatProvider::WhatsApp)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reclaimed.id, first.id);
    complete_job(&db, reclaimed.id).await.unwrap();

    assert!(
        persist_webhook_event(
            &db,
            ChatProvider::Telegram,
            "dead-event",
            &json!({"message": {"from": {"id": 42}}}),
            "dead-event-hash",
        )
        .await
        .unwrap()
    );
    let event = claim_webhook_event(&db, ChatProvider::Telegram)
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE webhook_events SET attempts = 8 WHERE id = $1")
        .bind(event.id)
        .execute(&db)
        .await
        .unwrap();
    fail_webhook_event(&db, event.id, "permanent failure")
        .await
        .unwrap();
    let dead_event: (String, serde_json::Value) =
        sqlx::query_as("SELECT processing_status, payload FROM webhook_events WHERE id = $1")
            .bind(event.id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(dead_event, ("dead".into(), json!({})));
}

#[tokio::test]
async fn outgoing_delivery_is_single_attempt_and_statuses_are_monotonic() {
    let Some((db, _database_lock)) = database().await else {
        eprintln!("TEST_DATABASE_URL is not set; outgoing integration test skipped");
        return;
    };
    let source = message(
        ChatProvider::WhatsApp,
        "outgoing-source",
        "wa-group",
        Some("pregunta"),
    );
    let (source_id, _) = persist_message(&db, &source).await.unwrap();
    let (outgoing_id, status, provider_id) = create_outgoing_message(
        &db,
        ChatProvider::WhatsApp,
        source_id,
        "wa-group",
        "respuesta original",
    )
    .await
    .unwrap();
    assert_eq!(status, "pending");
    assert!(provider_id.is_none());
    let (same_id, _, _) = create_outgoing_message(
        &db,
        ChatProvider::WhatsApp,
        source_id,
        "wa-group",
        "respuesta regenerada",
    )
    .await
    .unwrap();
    assert_eq!(same_id, outgoing_id);
    let body: String = sqlx::query_scalar("SELECT body FROM outgoing_messages WHERE id = $1")
        .bind(outgoing_id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(body, "respuesta original");

    assert!(mark_outgoing_sending(&db, outgoing_id).await.unwrap());
    assert!(!mark_outgoing_sending(&db, outgoing_id).await.unwrap());
    mark_outgoing_delivery_unknown(&db, outgoing_id, "ambiguous transport failure")
        .await
        .unwrap();
    let (_, status, provider_id) = create_outgoing_message(
        &db,
        ChatProvider::WhatsApp,
        source_id,
        "wa-group",
        "otro intento",
    )
    .await
    .unwrap();
    assert_eq!(status, "delivery_unknown");
    assert!(provider_id.is_none());

    let second_source = message(
        ChatProvider::WhatsApp,
        "status-source",
        "wa-group",
        Some("otra pregunta"),
    );
    let (second_source_id, _) = persist_message(&db, &second_source).await.unwrap();
    let (second_outgoing, _, _) = create_outgoing_message(
        &db,
        ChatProvider::WhatsApp,
        second_source_id,
        "wa-group",
        "respuesta",
    )
    .await
    .unwrap();
    assert!(mark_outgoing_sending(&db, second_outgoing).await.unwrap());
    mark_outgoing_sent(&db, second_outgoing, "status-message")
        .await
        .unwrap();
    assert!(
        apply_outgoing_status(&db, &incoming_status("status-message", "read", Some(200)))
            .await
            .unwrap()
    );
    for stale in [
        incoming_status("status-message", "delivered", Some(201)),
        incoming_status("status-message", "sent", None),
        incoming_status("status-message", "read", Some(200)),
        incoming_status("status-message", "unknown", Some(300)),
    ] {
        assert!(!apply_outgoing_status(&db, &stale).await.unwrap());
    }
    assert!(
        apply_outgoing_status(&db, &incoming_status("status-message", "read", Some(202)))
            .await
            .unwrap()
    );
    let final_state: (String, chrono::DateTime<Utc>) =
        sqlx::query_as("SELECT status, provider_status_at FROM outgoing_messages WHERE id = $1")
            .bind(second_outgoing)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(final_state.0, "read");
    assert_eq!(final_state.1, Utc.timestamp_opt(202, 0).unwrap());
}

#[tokio::test]
async fn hybrid_search_isolated_by_space_and_document_completion_is_atomic() {
    let Some((db, _database_lock)) = database().await else {
        eprintln!("TEST_DATABASE_URL is not set; search integration test skipped");
        return;
    };
    let source = message(ChatProvider::Telegram, "document-source", "tg-group", None);
    let (source_id, _) = persist_message(&db, &source).await.unwrap();
    let document = IncomingDocument {
        provider_media_id: "document-media".into(),
        filename: Some("document.pdf".into()),
        mime_type: Some("application/pdf".into()),
        sha256: None,
        file_size: Some(4),
        caption: Some("caption".into()),
    };
    let attachment_id = persist_document(&db, ChatProvider::Telegram, source_id, &document)
        .await
        .unwrap();
    let before: String =
        sqlx::query_scalar("SELECT processing_status FROM attachments WHERE id = $1")
            .bind(attachment_id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(before, "pending");
    let vector = vec![0.2_f32; 1536];
    complete_document_indexing(
        &db,
        attachment_id,
        source_id,
        "palabraunica dentro del documento",
        &[(
            "caption palabraunica dentro del documento".into(),
            vector.clone(),
        )],
        "text-embedding-3-small",
    )
    .await
    .unwrap();
    let completed: (String, String, i64) = sqlx::query_as(
        r#"
        SELECT a.processing_status, a.extracted_text, count(dc.id)
        FROM attachments a
        LEFT JOIN document_chunks dc ON dc.message_id = a.message_id
        WHERE a.id = $1
        GROUP BY a.processing_status, a.extracted_text
        "#,
    )
    .bind(attachment_id)
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(
        completed,
        (
            "completed".into(),
            "palabraunica dentro del documento".into(),
            1
        )
    );

    let mut other_space = message(
        ChatProvider::WhatsApp,
        "other-space-source",
        "wa-group",
        Some("palabraunica ajena"),
    );
    other_space.space_id = "otro-espacio".into();
    let (other_id, _) = persist_message(&db, &other_space).await.unwrap();
    replace_chunks(
        &db,
        other_id,
        &[("palabraunica ajena".into(), vector.clone())],
        "text-embedding-3-small",
    )
    .await
    .unwrap();
    let excluded_question = message(
        ChatProvider::Telegram,
        "search-question",
        "tg-group",
        Some("palabraunica"),
    );
    let (question_id, _) = persist_message(&db, &excluded_question).await.unwrap();
    let hits = search_space(&db, "agora", "palabraunica", vector, question_id, 5)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].message_id, source_id);
}

#[tokio::test]
async fn participant_export_and_deletion_cover_related_data() {
    let Some((db, _database_lock)) = database().await else {
        eprintln!("TEST_DATABASE_URL is not set; data-rights integration test skipped");
        return;
    };
    let target = message(
        ChatProvider::Telegram,
        "privacy-target",
        "tg-group",
        Some("contenido privado"),
    );
    let (target_id, _) = persist_message(&db, &target).await.unwrap();
    let mut other = message(
        ChatProvider::Telegram,
        "privacy-other",
        "tg-group",
        Some("contenido conservado"),
    );
    other.sender_id = "other-sender".into();
    let (other_id, _) = persist_message(&db, &other).await.unwrap();
    let document = IncomingDocument {
        provider_media_id: "privacy-file".into(),
        filename: Some("privacy.pdf".into()),
        mime_type: Some("application/pdf".into()),
        sha256: None,
        file_size: Some(4),
        caption: None,
    };
    let attachment_id = persist_document(&db, ChatProvider::Telegram, target_id, &document)
        .await
        .unwrap();
    save_attachment_original(&db, attachment_id, "privacy-hash", b"data")
        .await
        .unwrap();
    complete_document_indexing(
        &db,
        attachment_id,
        target_id,
        "contenido privado",
        &[("contenido privado".into(), vec![0.3_f32; 1536])],
        "text-embedding-3-small",
    )
    .await
    .unwrap();
    enqueue_job(
        &db,
        ChatProvider::Telegram,
        "privacy-job",
        "privacy-job",
        json!({"message_id": target_id, "attachment_id": attachment_id, "sender_id": "sender-test"}),
    )
    .await
    .unwrap();
    create_outgoing_message(
        &db,
        ChatProvider::Telegram,
        target_id,
        "tg-group",
        "respuesta privada",
    )
    .await
    .unwrap();
    persist_webhook_event(
        &db,
        ChatProvider::Telegram,
        "privacy-event",
        &json!({"message": {"from": {"id": "sender-test"}}}),
        "privacy-event-hash",
    )
    .await
    .unwrap();

    let mut connection = db.acquire().await.unwrap();
    sqlx::raw_sql("SET agora.provider = 'telegram'; SET agora.participant_id = 'sender-test';")
        .execute(&mut *connection)
        .await
        .unwrap();
    let export: String = sqlx::query_scalar(include_str!("../scripts/export-participant-data.sql"))
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    let export: serde_json::Value = serde_json::from_str(&export).unwrap();
    assert_eq!(export["messages"].as_array().unwrap().len(), 1);
    assert_eq!(export["attachments"].as_array().unwrap().len(), 1);
    assert_eq!(export["document_chunks"].as_array().unwrap().len(), 1);
    assert_eq!(export["jobs"].as_array().unwrap().len(), 1);
    assert_eq!(export["outgoing_messages"].as_array().unwrap().len(), 1);
    assert_eq!(
        export["pending_webhook_events"].as_array().unwrap().len(),
        1
    );

    sqlx::raw_sql(include_str!("../scripts/delete-participant-data.sql"))
        .execute(&mut *connection)
        .await
        .unwrap();
    drop(connection);

    let target_count: i64 = sqlx::query_scalar("SELECT count(*) FROM messages WHERE id = $1")
        .bind(target_id)
        .fetch_one(&db)
        .await
        .unwrap();
    let other_count: i64 = sqlx::query_scalar("SELECT count(*) FROM messages WHERE id = $1")
        .bind(other_id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(target_count, 0);
    assert_eq!(other_count, 1);
    let attachment_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM attachments WHERE message_id = $1")
            .bind(target_id)
            .fetch_one(&db)
            .await
            .unwrap();
    let chunk_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM document_chunks WHERE message_id = $1")
            .bind(target_id)
            .fetch_one(&db)
            .await
            .unwrap();
    let job_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM jobs WHERE payload->>'message_id' = $1")
            .bind(target_id.to_string())
            .fetch_one(&db)
            .await
            .unwrap();
    let outgoing_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM outgoing_messages WHERE body = 'respuesta privada'",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(
        (attachment_count, chunk_count, job_count, outgoing_count),
        (0, 0, 0, 0)
    );
    let minimized: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM webhook_events WHERE provider_event_id = $1")
            .bind("privacy-event")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(minimized, json!({}));
}

#[tokio::test]
async fn participant_rights_filter_mixed_whatsapp_webhook_payloads() {
    let Some((db, _database_lock)) = database().await else {
        eprintln!("TEST_DATABASE_URL is not set; data-rights integration test skipped");
        return;
    };
    let payload = json!({
        "entry": [{
            "id": "waba",
            "changes": [{
                "field": "messages",
                "value": {
                    "metadata": {"phone_number_id": "phone"},
                    "messages": [
                        {
                            "from": "target-wa",
                            "id": "target-message",
                            "text": {"body": "target secret"},
                            "context": {"id": "other-parent", "from": "other-wa"}
                        },
                        {
                            "from": "other-wa",
                            "id": "other-message",
                            "text": {"body": "other secret"},
                            "context": {"id": "target-parent", "from": "target-wa"}
                        }
                    ],
                    "contacts": [
                        {"wa_id": "target-wa", "profile": {"name": "Target"}},
                        {"wa_id": "other-wa", "profile": {"name": "Other"}}
                    ],
                    "statuses": [
                        {"recipient_id": "target-wa", "id": "target-status"},
                        {"recipient_id": "other-wa", "id": "other-status"}
                    ],
                    "groups": [
                        {"type": "participants_update", "participants": ["target-wa", "other-wa"]}
                    ],
                    "future_identity_field": {"participant": "other-wa"}
                }
            }]
        }]
    });
    persist_webhook_event(
        &db,
        ChatProvider::WhatsApp,
        "mixed-privacy-event",
        &payload,
        "mixed-privacy-hash",
    )
    .await
    .unwrap();
    let group_payload = json!({
        "object": "whatsapp_business_account",
        "entry": [{
            "id": "waba",
            "changes": [{
                "field": "group_participants_update",
                "value": {
                    "groups": [{
                        "type": "participants_update",
                        "participants": ["target-wa", "other-wa"]
                    }]
                }
            }]
        }]
    });
    persist_webhook_event(
        &db,
        ChatProvider::WhatsApp,
        "group-privacy-event",
        &group_payload,
        "group-privacy-hash",
    )
    .await
    .unwrap();

    let mut connection = db.acquire().await.unwrap();
    sqlx::raw_sql("SET agora.provider = 'whatsapp'; SET agora.participant_id = 'target-wa';")
        .execute(&mut *connection)
        .await
        .unwrap();
    let export: String = sqlx::query_scalar(include_str!("../scripts/export-participant-data.sql"))
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    let export: serde_json::Value = serde_json::from_str(&export).unwrap();
    let exported_events = export["pending_webhook_events"].as_array().unwrap();
    assert_eq!(exported_events.len(), 2);
    let exported_event = exported_events
        .iter()
        .find(|event| event["provider_event_id"] == "mixed-privacy-event")
        .unwrap();
    let exported_payload = &exported_event["payload"];
    let exported_payload = serde_json::to_string(exported_payload).unwrap();
    assert!(exported_payload.contains("target-message"));
    assert!(exported_payload.contains("target-status"));
    assert!(exported_payload.contains("target-wa"));
    assert!(exported_payload.contains("other-parent"));
    assert!(!exported_payload.contains("other-message"));
    assert!(!exported_payload.contains("other-status"));
    assert!(!exported_payload.contains("other-wa"));
    assert!(!exported_payload.contains("groups"));
    assert!(!exported_payload.contains("future_identity_field"));
    let group_export = exported_events
        .iter()
        .find(|event| event["provider_event_id"] == "group-privacy-event")
        .unwrap();
    assert!(
        !serde_json::to_string(&group_export["payload"])
            .unwrap()
            .contains("target-wa")
    );

    sqlx::raw_sql(include_str!("../scripts/delete-participant-data.sql"))
        .execute(&mut *connection)
        .await
        .unwrap();
    drop(connection);

    let retained: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM webhook_events WHERE provider_event_id = $1")
            .bind("mixed-privacy-event")
            .fetch_one(&db)
            .await
            .unwrap();
    let retained = serde_json::to_string(&retained).unwrap();
    assert!(retained.contains("other-message"));
    assert!(retained.contains("other-status"));
    assert!(retained.contains("other-wa"));
    assert!(retained.contains("target-parent"));
    assert!(!retained.contains("target-message"));
    assert!(!retained.contains("target-status"));
    assert!(!retained.contains("target-wa"));
    assert!(!retained.contains("groups"));
    assert!(!retained.contains("future_identity_field"));
    let minimized_group: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM webhook_events WHERE provider_event_id = $1")
            .bind("group-privacy-event")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(minimized_group, json!({}));
}

#[tokio::test]
async fn provider_migration_backfills_all_legacy_records_as_whatsapp() {
    let Ok(database_url) = env::var("TEST_DATABASE_URL") else {
        eprintln!("TEST_DATABASE_URL is not set; migration integration test skipped");
        return;
    };
    let _database_lock = DATABASE_MUTEX.get_or_init(|| Mutex::new(())).lock().await;
    let mut connection = PgConnection::connect(&database_url).await.unwrap();
    let schema = format!("provider_migration_{}", Uuid::new_v4().simple());
    sqlx::query(AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query(AssertSqlSafe(format!(
        "SET search_path TO {schema}, public"
    )))
    .execute(&mut connection)
    .await
    .unwrap();

    for migration in [
        include_str!("../migrations/0001_init.sql"),
        include_str!("../migrations/0002_ingestion_jobs.sql"),
        include_str!("../migrations/0003_webhook_hash_constraint.sql"),
        include_str!("../migrations/0004_outgoing_idempotency.sql"),
        include_str!("../migrations/0005_outgoing_status_timestamp.sql"),
        include_str!("../migrations/0006_processing_queue_recovery.sql"),
        include_str!("../migrations/0007_store_attachment_binaries.sql"),
    ] {
        sqlx::raw_sql(migration)
            .execute(&mut connection)
            .await
            .unwrap();
    }
    sqlx::raw_sql(
        r#"
        INSERT INTO webhook_events (provider, payload, content_sha256)
        VALUES ('whatsapp', '{}', 'legacy-event');
        INSERT INTO messages (whatsapp_message_id, group_id, message_type)
        VALUES ('legacy-message', 'legacy-group', 'text');
        INSERT INTO attachments (message_id, provider_media_id, original_data)
        SELECT id, 'legacy-file', '\x64617461' FROM messages;
        INSERT INTO jobs (job_type, dedupe_key) VALUES ('legacy-job', 'legacy-key');
        INSERT INTO outgoing_messages (source_message_id, group_id, body)
        SELECT id, 'legacy-group', 'legacy response' FROM messages;
        INSERT INTO document_chunks (message_id, chunk_index, content)
        SELECT id, 0, 'legacy content' FROM messages;
        "#,
    )
    .execute(&mut connection)
    .await
    .unwrap();
    for migration in [
        include_str!("../migrations/0008_document_chunks_group_id.sql"),
        include_str!("../migrations/0009_chat_providers.sql"),
        include_str!("../migrations/0010_attachment_message_identity.sql"),
        include_str!("../migrations/0011_outgoing_delivery_attempt.sql"),
        include_str!("../migrations/0012_filter_participant_webhook_payload.sql"),
    ] {
        sqlx::raw_sql(migration)
            .execute(&mut connection)
            .await
            .unwrap();
    }

    let providers: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT provider FROM webhook_events
        UNION ALL SELECT provider FROM messages
        UNION ALL SELECT provider FROM attachments
        UNION ALL SELECT provider FROM jobs
        UNION ALL SELECT provider FROM outgoing_messages
        "#,
    )
    .fetch_all(&mut connection)
    .await
    .unwrap();
    assert_eq!(providers, vec!["whatsapp"; 5]);
    let message: (String, String, String) =
        sqlx::query_as("SELECT external_message_id, conversation_id, space_id FROM messages")
            .fetch_one(&mut connection)
            .await
            .unwrap();
    assert_eq!(
        message,
        (
            "legacy-message".into(),
            "legacy-group".into(),
            "agora".into()
        )
    );
    let original: Vec<u8> = sqlx::query_scalar("SELECT original_data FROM attachments")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(original, b"data");
    let event_id: String = sqlx::query_scalar("SELECT provider_event_id FROM webhook_events")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(event_id, "legacy-event");
    let chunk_scope: (String, String) =
        sqlx::query_as("SELECT group_id, space_id FROM document_chunks")
            .fetch_one(&mut connection)
            .await
            .unwrap();
    assert_eq!(chunk_scope, ("legacy-group".into(), "agora".into()));

    sqlx::query("SET search_path TO public")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query(AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&mut connection)
        .await
        .unwrap();
}

#[tokio::test]
async fn provider_migration_preflight_normalizes_legacy_edge_cases() {
    let Ok(database_url) = env::var("TEST_DATABASE_URL") else {
        eprintln!("TEST_DATABASE_URL is not set; preflight integration test skipped");
        return;
    };
    let _database_lock = DATABASE_MUTEX.get_or_init(|| Mutex::new(())).lock().await;
    let mut connection = PgConnection::connect(&database_url).await.unwrap();
    let schema = format!("provider_preflight_{}", Uuid::new_v4().simple());
    sqlx::query(AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query(AssertSqlSafe(format!(
        "SET search_path TO {schema}, public"
    )))
    .execute(&mut connection)
    .await
    .unwrap();

    for migration in [
        include_str!("../migrations/0001_init.sql"),
        include_str!("../migrations/0002_ingestion_jobs.sql"),
        include_str!("../migrations/0003_webhook_hash_constraint.sql"),
        include_str!("../migrations/0004_outgoing_idempotency.sql"),
        include_str!("../migrations/0005_outgoing_status_timestamp.sql"),
        include_str!("../migrations/0006_processing_queue_recovery.sql"),
        include_str!("../migrations/0007_store_attachment_binaries.sql"),
    ] {
        sqlx::raw_sql(migration)
            .execute(&mut connection)
            .await
            .unwrap();
    }
    sqlx::raw_sql(
        r#"
        INSERT INTO webhook_events (provider, payload, content_sha256)
        VALUES ('whatsapp', '{"same":true}', NULL),
               ('whatsapp', '{"same":true}', NULL);
        INSERT INTO messages (whatsapp_message_id, group_id, message_type)
        VALUES ('legacy-null-group', NULL, 'text');
        INSERT INTO document_chunks (message_id, chunk_index, content)
        SELECT id, 0, 'legacy content' FROM messages;
        "#,
    )
    .execute(&mut connection)
    .await
    .unwrap();

    sqlx::raw_sql(include_str!(
        "../scripts/preflight-chat-provider-migration.sql"
    ))
    .execute(&mut connection)
    .await
    .unwrap();
    for migration in [
        include_str!("../migrations/0008_document_chunks_group_id.sql"),
        include_str!("../migrations/0009_chat_providers.sql"),
        include_str!("../migrations/0010_attachment_message_identity.sql"),
        include_str!("../migrations/0011_outgoing_delivery_attempt.sql"),
        include_str!("../migrations/0012_filter_participant_webhook_payload.sql"),
    ] {
        sqlx::raw_sql(migration)
            .execute(&mut connection)
            .await
            .unwrap();
    }

    let group_id: String = sqlx::query_scalar("SELECT group_id FROM document_chunks")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(group_id, "legacy");
    let event_ids: Vec<String> =
        sqlx::query_scalar("SELECT provider_event_id FROM webhook_events ORDER BY id")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert_eq!(event_ids.len(), 2);
    assert_ne!(event_ids[0], event_ids[1]);

    sqlx::query("SET search_path TO public")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query(AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&mut connection)
        .await
        .unwrap();
}
