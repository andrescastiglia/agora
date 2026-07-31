use std::{env, sync::OnceLock};

use agora::{
    chat::{ChatProvider, IncomingDocument, IncomingMessage, IncomingStatus},
    repository::{
        apply_outgoing_status, attachment_details, claim_job, claim_webhook_event, complete_job,
        complete_webhook_event, create_outgoing_message, enqueue_job, mark_outgoing_sent,
        persist_document, persist_message, persist_webhook_event, ping, replace_chunks,
        save_attachment_original, save_extracted_text, search_space,
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
    save_extracted_text(&db, telegram_attachment, "contenido extraído")
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

    let vector = vec![0.1_f32; 1536];
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
        include_str!("../migrations/0008_document_chunks_group_id.sql"),
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
        INSERT INTO document_chunks (message_id, group_id, chunk_index, content)
        SELECT id, 'legacy-group', 0, 'legacy content' FROM messages;
        "#,
    )
    .execute(&mut connection)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!("../migrations/0009_chat_providers.sql"))
        .execute(&mut connection)
        .await
        .unwrap();

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

    sqlx::query("SET search_path TO public")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query(AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&mut connection)
        .await
        .unwrap();
}
