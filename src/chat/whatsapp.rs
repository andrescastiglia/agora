pub fn parse_event(
    payload: &serde_json::Value,
    space_id: &str,
) -> anyhow::Result<super::ParsedEvent> {
    let messages = parse_group_messages(payload)?
        .into_iter()
        .filter_map(|message| {
            let timestamp = message.timestamp.parse::<i64>().ok()?;
            let timestamp = chrono::DateTime::from_timestamp(timestamp, 0)?;
            let document = message.document.map(|document| super::IncomingDocument {
                provider_media_id: document.media_id,
                filename: document.filename,
                mime_type: document.mime_type,
                sha256: document.sha256,
                file_size: None,
                caption: document.caption,
            });
            Some(super::IncomingMessage {
                provider: super::ChatProvider::WhatsApp,
                external_message_id: message.message_id,
                conversation_id: message.group_id,
                space_id: space_id.to_owned(),
                sender_id: message.sender_id,
                sender_name: message.sender_name,
                kind: message.kind,
                text: message.text,
                document,
                timestamp,
                reply_to_message_id: message.reply_to_message_id,
                metadata: serde_json::json!({
                    "waba_id": message.waba_id,
                    "phone_number_id": message.phone_number_id,
                }),
            })
        })
        .collect();
    let statuses = parse_statuses(payload)?
        .into_iter()
        .map(|status| super::IncomingStatus {
            provider: super::ChatProvider::WhatsApp,
            provider_message_id: status.provider_message_id,
            status: status.status,
            timestamp: status
                .timestamp
                .parse::<i64>()
                .ok()
                .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0)),
            recipient_id: status.recipient_id,
            recipient_type: status.recipient_type,
            error: status.error,
        })
        .collect();
    Ok(super::ParsedEvent { messages, statuses })
}

include!("../whatsapp.rs");
