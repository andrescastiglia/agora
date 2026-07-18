use std::env;

use agora::{
    repository::{
        apply_outgoing_status, attachment_details, claim_job, claim_webhook_event, complete_job,
        complete_webhook_event, create_outgoing_message, enqueue_job, fail_job, fail_webhook_event,
        mark_outgoing_sent, message_text, persist_document, persist_group_message,
        persist_webhook_event, ping, replace_chunks, save_attachment_object_key,
        save_extracted_text, search_group,
    },
    whatsapp::{Document, IncomingGroupMessage, IncomingStatus},
};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;

async fn database() -> Option<sqlx::PgPool> {
    let url = env::var("TEST_DATABASE_URL").ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
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
    Some(pool)
}

fn message(external_id: &str, text: Option<&str>) -> IncomingGroupMessage {
    IncomingGroupMessage {
        waba_id: "waba-test".into(),
        phone_number_id: Some("phone-test".into()),
        sender_id: "sender-test".into(),
        sender_name: Some("Ana".into()),
        group_id: "group-test".into(),
        message_id: external_id.into(),
        timestamp: "1700000000".into(),
        kind: if text.is_some() { "text" } else { "document" }.into(),
        text: text.map(str::to_owned),
        document: None,
        reply_to_message_id: None,
    }
}

#[tokio::test]
async fn repository_supports_idempotent_ingestion_jobs_search_and_statuses() {
    let Some(db) = database().await else {
        eprintln!("TEST_DATABASE_URL is not set; repository integration test skipped");
        return;
    };

    ping(&db).await.unwrap();

    let webhook = json!({"object": "whatsapp_business_account", "entry": []});
    assert!(
        persist_webhook_event(&db, &webhook, "hash-complete")
            .await
            .unwrap()
    );
    assert!(
        !persist_webhook_event(&db, &webhook, "hash-complete")
            .await
            .unwrap()
    );
    let event = claim_webhook_event(&db).await.unwrap().unwrap();
    assert_eq!(event.payload, webhook);
    complete_webhook_event(&db, event.id).await.unwrap();

    assert!(
        persist_webhook_event(&db, &json!({"failed": true}), "hash-fail")
            .await
            .unwrap()
    );
    let event = claim_webhook_event(&db).await.unwrap().unwrap();
    fail_webhook_event(&db, event.id, "temporary secret-looking error")
        .await
        .unwrap();
    let state: (String, i32, Option<String>) = sqlx::query_as(
        "SELECT processing_status, attempts, last_error FROM webhook_events WHERE id = $1",
    )
    .bind(event.id)
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(state.0, "pending");
    assert_eq!(state.1, 1);
    assert!(state.2.unwrap().contains("temporary"));

    let source = message("wamid.source", Some("@agora: ¿qué se decidió?"));
    let (source_id, inserted) = persist_group_message(&db, &source).await.unwrap();
    assert!(inserted);
    assert_eq!(
        persist_group_message(&db, &source).await.unwrap(),
        (source_id, false)
    );
    assert_eq!(
        message_text(&db, source_id).await.unwrap().as_deref(),
        source.text.as_deref()
    );

    let knowledge = message("wamid.knowledge", Some("La reunión es el viernes."));
    let (knowledge_id, inserted) = persist_group_message(&db, &knowledge).await.unwrap();
    assert!(inserted);

    let document = Document {
        media_id: "media-1".into(),
        filename: Some("informe.pdf".into()),
        mime_type: Some("application/pdf".into()),
        sha256: Some("provider-hash".into()),
        caption: Some("Informe".into()),
    };
    let attachment_id = persist_document(&db, knowledge_id, &document)
        .await
        .unwrap();
    assert_eq!(
        attachment_details(&db, attachment_id)
            .await
            .unwrap()
            .unwrap()
            .1,
        "media-1"
    );
    save_extracted_text(&db, attachment_id, "contenido extraído", "content-hash")
        .await
        .unwrap();
    save_attachment_object_key(&db, attachment_id, "documents/content-hash.pdf")
        .await
        .unwrap();

    assert!(
        enqueue_job(
            &db,
            "embed_message",
            "wamid.knowledge",
            json!({"message_id": knowledge_id})
        )
        .await
        .unwrap()
    );
    assert!(
        !enqueue_job(
            &db,
            "embed_message",
            "wamid.knowledge",
            json!({"message_id": knowledge_id})
        )
        .await
        .unwrap()
    );
    let job = claim_job(&db).await.unwrap().unwrap();
    assert_eq!(job.job_type, "embed_message");
    assert_eq!(job.payload["message_id"], knowledge_id.to_string());
    complete_job(&db, job.id).await.unwrap();

    assert!(
        enqueue_job(&db, "retry", "retry-1", json!({}))
            .await
            .unwrap()
    );
    let retry = claim_job(&db).await.unwrap().unwrap();
    fail_job(&db, retry.id, "provider unavailable")
        .await
        .unwrap();
    let state: (String, i32) = sqlx::query_as("SELECT status, attempts FROM jobs WHERE id = $1")
        .bind(retry.id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(state, ("pending".into(), 1));

    let vector = vec![0.1_f32; 1536];
    replace_chunks(
        &db,
        knowledge_id,
        &[("La reunión es el viernes.".into(), vector.clone())],
        "text-embedding-3-small",
    )
    .await
    .unwrap();
    let hits = search_group(&db, "group-test", "reunión viernes", vector, source_id, 5)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].message_id, knowledge_id);
    assert_eq!(hits[0].whatsapp_message_id, "wamid.knowledge");
    assert!(hits[0].score.is_finite());

    let (outgoing_id, status, provider_id) =
        create_outgoing_message(&db, source_id, "group-test", "Respuesta [1]")
            .await
            .unwrap();
    assert_eq!(status, "pending");
    assert!(provider_id.is_none());
    let duplicate =
        create_outgoing_message(&db, source_id, "group-test", "Respuesta actualizada [1]")
            .await
            .unwrap();
    assert_eq!(duplicate.0, outgoing_id);

    mark_outgoing_sent(&db, outgoing_id, "wamid.out")
        .await
        .unwrap();
    assert!(
        apply_outgoing_status(
            &db,
            &IncomingStatus {
                provider_message_id: "wamid.out".into(),
                status: "delivered".into(),
                timestamp: "1700000100".into(),
                recipient_id: Some("group-test".into()),
                recipient_type: Some("group".into()),
                error: None,
            }
        )
        .await
        .unwrap()
    );
    assert!(
        !apply_outgoing_status(
            &db,
            &IncomingStatus {
                provider_message_id: "unknown".into(),
                status: "read".into(),
                timestamp: "1700000200".into(),
                recipient_id: None,
                recipient_type: None,
                error: None,
            }
        )
        .await
        .unwrap()
    );
    let final_status: String =
        sqlx::query_scalar("SELECT status FROM outgoing_messages WHERE id = $1")
            .bind(outgoing_id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(final_status, "delivered");
}
