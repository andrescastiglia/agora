use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::config::Config;

#[derive(Debug, Clone, Deserialize)]
pub struct Webhook {
    pub object: String,
    #[serde(default)]
    pub entry: Vec<Entry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    pub id: String,
    #[serde(default)]
    pub changes: Vec<Change>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Change {
    pub field: String,
    pub value: ChangeValue,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChangeValue {
    #[serde(default)]
    pub metadata: Option<Metadata>,
    #[serde(default)]
    pub contacts: Vec<Contact>,
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub statuses: Vec<Status>,
    #[serde(default)]
    pub groups: Vec<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Metadata {
    pub display_phone_number: Option<String>,
    pub phone_number_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Contact {
    pub wa_id: String,
    #[serde(default)]
    pub profile: Option<Profile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    pub from: String,
    pub group_id: Option<String>,
    pub id: String,
    pub timestamp: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub text: Option<TextContent>,
    pub document: Option<DocumentContent>,
    pub context: Option<MessageContext>,
    #[serde(default)]
    pub errors: Vec<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TextContent {
    pub body: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DocumentContent {
    pub id: String,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub sha256: Option<String>,
    pub caption: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageContext {
    pub id: Option<String>,
    pub from: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Status {
    pub id: String,
    pub status: String,
    pub timestamp: String,
    pub recipient_id: Option<String>,
    pub recipient_type: Option<String>,
    #[serde(default)]
    pub errors: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingGroupMessage {
    pub waba_id: String,
    pub phone_number_id: Option<String>,
    pub sender_id: String,
    pub sender_name: Option<String>,
    pub group_id: String,
    pub message_id: String,
    pub timestamp: String,
    pub kind: String,
    pub text: Option<String>,
    pub document: Option<Document>,
    pub reply_to_message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingStatus {
    pub provider_message_id: String,
    pub status: String,
    pub timestamp: String,
    pub recipient_id: Option<String>,
    pub recipient_type: Option<String>,
    pub error: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Document {
    pub media_id: String,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub sha256: Option<String>,
    pub caption: Option<String>,
}

pub fn parse_group_messages(
    payload: &Value,
) -> Result<Vec<IncomingGroupMessage>, serde_json::Error> {
    let webhook: Webhook = serde_json::from_value(payload.clone())?;
    let mut result = Vec::new();

    if webhook.object != "whatsapp_business_account" {
        return Ok(result);
    }

    for entry in webhook.entry {
        for change in entry.changes {
            if change.field != "messages" {
                continue;
            }
            let phone_number_id = change
                .value
                .metadata
                .and_then(|metadata| metadata.phone_number_id);
            for message in change.value.messages {
                let Some(group_id) = message.group_id else {
                    continue;
                };
                let sender_name = change
                    .value
                    .contacts
                    .iter()
                    .find(|contact| contact.wa_id == message.from)
                    .and_then(|contact| contact.profile.as_ref())
                    .and_then(|profile| profile.name.clone());
                let text = message.text.map(|content| content.body);
                let document = message.document.map(|content| Document {
                    media_id: content.id,
                    filename: content.filename,
                    mime_type: content.mime_type,
                    sha256: content.sha256,
                    caption: content.caption,
                });

                result.push(IncomingGroupMessage {
                    waba_id: entry.id.clone(),
                    phone_number_id: phone_number_id.clone(),
                    sender_id: message.from,
                    sender_name,
                    group_id,
                    message_id: message.id,
                    timestamp: message.timestamp,
                    kind: message.kind,
                    text,
                    document,
                    reply_to_message_id: message.context.and_then(|context| context.id),
                });
            }
        }
    }

    Ok(result)
}

pub fn parse_statuses(payload: &Value) -> Result<Vec<IncomingStatus>, serde_json::Error> {
    let webhook: Webhook = serde_json::from_value(payload.clone())?;
    if webhook.object != "whatsapp_business_account" {
        return Ok(Vec::new());
    }

    Ok(webhook
        .entry
        .into_iter()
        .flat_map(|entry| entry.changes)
        .filter(|change| change.field == "messages")
        .flat_map(|change| change.value.statuses)
        .map(|status| IncomingStatus {
            provider_message_id: status.id,
            status: status.status,
            timestamp: status.timestamp,
            recipient_id: status.recipient_id,
            recipient_type: status.recipient_type,
            error: status.errors.into_iter().next(),
        })
        .collect())
}

pub fn question_for_bot(text: &str, mention: &str) -> Option<String> {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();
    let mention = mention.trim().to_lowercase();
    let prefixes = [
        format!("{mention}:"),
        format!("{mention},"),
        "agora:".into(),
        "agora,".into(),
        mention.clone(),
    ];

    prefixes.iter().find_map(|prefix| {
        lower.strip_prefix(prefix).and_then(|_| {
            let question = trimmed.get(prefix.len()..)?.trim();
            (!question.is_empty()).then(|| question.to_owned())
        })
    })
}

pub fn supported_document(filename: Option<&str>, mime_type: Option<&str>) -> bool {
    const EXTENSIONS: [&str; 5] = ["doc", "docx", "pdf", "xls", "xlsx"];
    const MIME_TYPES: [&str; 7] = [
        "application/msword",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "application/pdf",
        "application/vnd.ms-excel",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "application/octet-stream",
        "application/zip",
    ];

    let extension_supported = filename
        .and_then(|name| name.rsplit_once('.').map(|(_, extension)| extension))
        .map(|extension| EXTENSIONS.contains(&extension.to_lowercase().as_str()))
        .unwrap_or(false);
    let mime_supported = mime_type
        .map(|mime| MIME_TYPES.contains(&mime.to_lowercase().as_str()))
        .unwrap_or(false);

    extension_supported && mime_supported
}

#[derive(Clone)]
pub struct WhatsAppClient {
    http: reqwest::Client,
    access_token: String,
    phone_number_id: String,
    graph_version: String,
    base_url: String,
}

#[derive(Debug, Error)]
pub enum WhatsAppError {
    #[error("WhatsApp API configuration is incomplete")]
    NotConfigured,
    #[error("WhatsApp request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("WhatsApp returned {status}: {body}")]
    Api {
        status: reqwest::StatusCode,
        body: String,
    },
    #[error("WhatsApp response did not include the expected field")]
    InvalidResponse,
}

#[derive(Debug, Deserialize)]
pub struct SentMessage {
    pub id: String,
}

#[derive(Debug, Deserialize)]
struct SendResponse {
    messages: Vec<SentMessage>,
}

#[derive(Debug, Deserialize)]
struct MediaMetadata {
    url: String,
    mime_type: Option<String>,
    sha256: Option<String>,
    file_size: Option<u64>,
}

impl WhatsAppClient {
    pub fn from_config(config: &Config) -> Result<Self, WhatsAppError> {
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()?,
            access_token: config
                .whatsapp_access_token
                .as_ref()
                .ok_or(WhatsAppError::NotConfigured)?
                .expose()
                .to_owned(),
            phone_number_id: config
                .whatsapp_phone_number_id
                .clone()
                .ok_or(WhatsAppError::NotConfigured)?,
            graph_version: config.meta_graph_api_version.clone(),
            base_url: "https://graph.facebook.com".into(),
        })
    }

    #[cfg(test)]
    pub fn with_base_url(config: &Config, base_url: String) -> Result<Self, WhatsAppError> {
        let mut client = Self::from_config(config)?;
        client.base_url = base_url;
        Ok(client)
    }

    pub async fn send_group_text(
        &self,
        group_id: &str,
        body: &str,
    ) -> Result<SentMessage, WhatsAppError> {
        let response = self
            .http
            .post(format!(
                "{}/{}/{}/messages",
                self.base_url, self.graph_version, self.phone_number_id
            ))
            .bearer_auth(&self.access_token)
            .json(&serde_json::json!({
                "messaging_product": "whatsapp",
                "recipient_type": "group",
                "to": group_id,
                "type": "text",
                "text": {
                    "preview_url": false,
                    "body": body,
                }
            }))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(WhatsAppError::Api {
                status,
                body: response.text().await?.chars().take(1000).collect(),
            });
        }
        response
            .json::<SendResponse>()
            .await?
            .messages
            .into_iter()
            .next()
            .ok_or(WhatsAppError::InvalidResponse)
    }

    pub async fn download_media(
        &self,
        media_id: &str,
        maximum_bytes: u64,
    ) -> Result<(Vec<u8>, Option<String>, Option<String>), WhatsAppError> {
        let metadata_response = self
            .http
            .get(format!(
                "{}/{}/{}",
                self.base_url, self.graph_version, media_id
            ))
            .bearer_auth(&self.access_token)
            .send()
            .await?;
        let status = metadata_response.status();
        if !status.is_success() {
            return Err(WhatsAppError::Api {
                status,
                body: metadata_response.text().await?.chars().take(1000).collect(),
            });
        }
        let metadata = metadata_response.json::<MediaMetadata>().await?;
        if metadata.file_size.is_some_and(|size| size > maximum_bytes) {
            return Err(WhatsAppError::Api {
                status: reqwest::StatusCode::PAYLOAD_TOO_LARGE,
                body: "media exceeds configured maximum".into(),
            });
        }
        let media_response = self
            .http
            .get(metadata.url)
            .bearer_auth(&self.access_token)
            .send()
            .await?;
        let status = media_response.status();
        if !status.is_success() {
            return Err(WhatsAppError::Api {
                status,
                body: media_response.text().await?.chars().take(1000).collect(),
            });
        }
        if media_response
            .content_length()
            .is_some_and(|size| size > maximum_bytes)
        {
            return Err(WhatsAppError::Api {
                status: reqwest::StatusCode::PAYLOAD_TOO_LARGE,
                body: "media exceeds configured maximum".into(),
            });
        }
        let bytes = media_response.bytes().await?;
        if bytes.len() as u64 > maximum_bytes {
            return Err(WhatsAppError::Api {
                status: reqwest::StatusCode::PAYLOAD_TOO_LARGE,
                body: "media exceeds configured maximum".into(),
            });
        }
        Ok((bytes.to_vec(), metadata.mime_type, metadata.sha256))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use axum::{
        Json, Router,
        body::Body,
        extract::State,
        http::{HeaderMap, Response, StatusCode},
        routing::{get, post},
    };
    use serde_json::json;

    use super::*;

    fn client_config(with_credentials: bool) -> Config {
        let mut values = HashMap::from([
            ("DATABASE_URL".into(), "postgres://localhost/agora".into()),
            ("WHATSAPP_VERIFY_TOKEN".into(), "verify".into()),
            ("WHATSAPP_APP_SECRET".into(), "secret".into()),
        ]);
        if with_credentials {
            values.insert("WHATSAPP_ACCESS_TOKEN".into(), "test-token".into());
            values.insert("WHATSAPP_PHONE_NUMBER_ID".into(), "phone-test".into());
            values.insert("WHATSAPP_WABA_ID".into(), "waba-test".into());
        }
        Config::from_map(values).unwrap()
    }

    async fn serve(build: impl FnOnce(String) -> Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let router = build(base_url.clone());
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        base_url
    }

    #[test]
    fn parses_group_text_and_ignores_individual_messages() {
        let payload = json!({
            "object": "whatsapp_business_account",
            "entry": [{
                "id": "waba-1",
                "changes": [{
                    "field": "messages",
                    "value": {
                        "metadata": {"phone_number_id": "phone-1"},
                        "contacts": [{"wa_id": "54911", "profile": {"name": "Ana"}}],
                        "messages": [
                            {
                                "from": "54911",
                                "group_id": "group-1",
                                "id": "wamid.1",
                                "timestamp": "1700000000",
                                "type": "text",
                                "text": {"body": "Hola"}
                            },
                            {
                                "from": "54912",
                                "id": "wamid.2",
                                "timestamp": "1700000001",
                                "type": "text",
                                "text": {"body": "privado"}
                            }
                        ]
                    }
                }]
            }]
        });

        let messages = parse_group_messages(&payload).unwrap();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].group_id, "group-1");
        assert_eq!(messages[0].sender_name.as_deref(), Some("Ana"));
        assert_eq!(messages[0].text.as_deref(), Some("Hola"));
    }

    #[test]
    fn parses_supported_document_and_reply_context() {
        let payload = json!({
            "object": "whatsapp_business_account",
            "entry": [{
                "id": "waba",
                "changes": [{
                    "field": "messages",
                    "value": {
                        "messages": [{
                            "from": "54911",
                            "group_id": "group",
                            "id": "wamid.doc",
                            "timestamp": "1",
                            "type": "document",
                            "context": {"id": "wamid.parent"},
                            "document": {
                                "id": "media",
                                "filename": "informe.pdf",
                                "mime_type": "application/pdf",
                                "sha256": "hash"
                            }
                        }]
                    }
                }]
            }]
        });

        let messages = parse_group_messages(&payload).unwrap();
        let message = &messages[0];

        assert_eq!(message.reply_to_message_id.as_deref(), Some("wamid.parent"));
        assert_eq!(message.document.as_ref().unwrap().media_id, "media");
    }

    #[test]
    fn ignores_wrong_object_and_metadata_events() {
        assert!(
            parse_group_messages(&json!({"object": "page", "entry": []}))
                .unwrap()
                .is_empty()
        );
        assert!(
            parse_group_messages(&json!({
                "object": "whatsapp_business_account",
                "entry": [{"id": "waba", "changes": [{
                    "field": "group_lifecycle_update",
                    "value": {"groups": [{"type": "group_create"}]}
                }]}]
            }))
            .unwrap()
            .is_empty()
        );
    }

    #[test]
    fn identifies_bot_questions() {
        assert_eq!(
            question_for_bot("@Agora: ¿qué se decidió?", "@agora"),
            Some("¿qué se decidió?".into())
        );
        assert_eq!(
            question_for_bot("Agora, resumí el PDF", "@agora"),
            Some("resumí el PDF".into())
        );
        assert_eq!(question_for_bot("conversación normal", "@agora"), None);
        assert_eq!(question_for_bot("@agora", "@agora"), None);
    }

    #[test]
    fn validates_document_extension_and_mime_together() {
        assert!(supported_document(
            Some("plan.XLSX"),
            Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")
        ));
        assert!(!supported_document(
            Some("malware.exe"),
            Some("application/octet-stream")
        ));
        assert!(!supported_document(Some("informe.pdf"), Some("image/png")));
    }

    #[test]
    fn parses_delivery_statuses_and_preserves_first_error() {
        let payload = json!({
            "object": "whatsapp_business_account",
            "entry": [{
                "id": "waba-1",
                "changes": [{
                    "field": "messages",
                    "value": {
                        "statuses": [{
                            "id": "wamid.out",
                            "status": "failed",
                            "timestamp": "1700000002",
                            "recipient_id": "group-1",
                            "recipient_type": "group",
                            "errors": [{"code": 131000, "title": "error"}]
                        }]
                    }
                }]
            }]
        });

        let statuses = parse_statuses(&payload).unwrap();

        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].provider_message_id, "wamid.out");
        assert_eq!(statuses[0].status, "failed");
        assert_eq!(statuses[0].recipient_type.as_deref(), Some("group"));
        assert_eq!(statuses[0].error.as_ref().unwrap()["code"], 131000);
    }

    #[test]
    fn requires_whatsapp_credentials() {
        assert!(matches!(
            WhatsAppClient::from_config(&client_config(false)),
            Err(WhatsAppError::NotConfigured)
        ));
    }

    #[tokio::test]
    async fn sends_group_messages_using_the_official_payload() {
        async fn handler(headers: HeaderMap, Json(body): Json<Value>) -> Json<Value> {
            assert_eq!(headers.get("authorization").unwrap(), "Bearer test-token");
            assert_eq!(body["recipient_type"], "group");
            assert_eq!(body["to"], "group-test");
            assert_eq!(body["text"]["body"], "Respuesta");
            Json(json!({"messages": [{"id": "wamid.out"}]}))
        }
        let base_url =
            serve(|_| Router::new().route("/v25.0/phone-test/messages", post(handler))).await;
        let client = WhatsAppClient::with_base_url(&client_config(true), base_url).unwrap();

        assert_eq!(
            client
                .send_group_text("group-test", "Respuesta")
                .await
                .unwrap()
                .id,
            "wamid.out"
        );
    }

    #[tokio::test]
    async fn handles_send_errors_and_missing_message_ids() {
        async fn failure() -> (StatusCode, &'static str) {
            (StatusCode::TOO_MANY_REQUESTS, "rate limited")
        }
        let base_url =
            serve(|_| Router::new().route("/v25.0/phone-test/messages", post(failure))).await;
        let client = WhatsAppClient::with_base_url(&client_config(true), base_url).unwrap();
        assert!(matches!(
            client.send_group_text("group", "body").await,
            Err(WhatsAppError::Api {
                status: StatusCode::TOO_MANY_REQUESTS,
                ..
            })
        ));

        async fn empty() -> Json<Value> {
            Json(json!({"messages": []}))
        }
        let base_url =
            serve(|_| Router::new().route("/v25.0/phone-test/messages", post(empty))).await;
        let client = WhatsAppClient::with_base_url(&client_config(true), base_url).unwrap();
        assert!(matches!(
            client.send_group_text("group", "body").await,
            Err(WhatsAppError::InvalidResponse)
        ));
    }

    #[tokio::test]
    async fn downloads_media_after_validating_metadata_and_size() {
        async fn metadata(State(base): State<String>) -> Json<Value> {
            Json(json!({
                "url": format!("{base}/download"),
                "mime_type": "application/pdf",
                "sha256": "hash",
                "file_size": 4
            }))
        }
        async fn download(headers: HeaderMap) -> &'static [u8] {
            assert_eq!(headers.get("authorization").unwrap(), "Bearer test-token");
            b"data"
        }
        let base_url = serve(|base| {
            Router::new()
                .route("/v25.0/media-1", get(metadata))
                .route("/download", get(download))
                .with_state(base)
        })
        .await;
        let client = WhatsAppClient::with_base_url(&client_config(true), base_url).unwrap();

        let result = client.download_media("media-1", 10).await.unwrap();
        assert_eq!(result.0, b"data");
        assert_eq!(result.1.as_deref(), Some("application/pdf"));
        assert_eq!(result.2.as_deref(), Some("hash"));
    }

    #[tokio::test]
    async fn rejects_oversized_media_and_graph_failures() {
        async fn oversized_metadata() -> Json<Value> {
            Json(json!({"url": "http://invalid", "file_size": 11}))
        }
        let base_url =
            serve(|_| Router::new().route("/v25.0/media", get(oversized_metadata))).await;
        let client = WhatsAppClient::with_base_url(&client_config(true), base_url).unwrap();
        assert!(matches!(
            client.download_media("media", 10).await,
            Err(WhatsAppError::Api {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                ..
            })
        ));

        async fn metadata_failure() -> (StatusCode, &'static str) {
            (StatusCode::UNAUTHORIZED, "expired")
        }
        let base_url = serve(|_| Router::new().route("/v25.0/media", get(metadata_failure))).await;
        let client = WhatsAppClient::with_base_url(&client_config(true), base_url).unwrap();
        assert!(matches!(
            client.download_media("media", 10).await,
            Err(WhatsAppError::Api {
                status: StatusCode::UNAUTHORIZED,
                ..
            })
        ));

        async fn metadata(State(base): State<String>) -> Json<Value> {
            Json(json!({"url": format!("{base}/download"), "file_size": 4}))
        }
        async fn download_failure() -> (StatusCode, &'static str) {
            (StatusCode::BAD_GATEWAY, "upstream")
        }
        let base_url = serve(|base| {
            Router::new()
                .route("/v25.0/media", get(metadata))
                .route("/download", get(download_failure))
                .with_state(base)
        })
        .await;
        let client = WhatsAppClient::with_base_url(&client_config(true), base_url).unwrap();
        assert!(matches!(
            client.download_media("media", 10).await,
            Err(WhatsAppError::Api {
                status: StatusCode::BAD_GATEWAY,
                ..
            })
        ));

        async fn declared_too_large() -> Response<Body> {
            Response::builder()
                .header("content-length", "11")
                .body(Body::from("12345678901"))
                .unwrap()
        }
        let base_url = serve(|base| {
            Router::new()
                .route("/v25.0/media", get(metadata))
                .route("/download", get(declared_too_large))
                .with_state(base)
        })
        .await;
        let client = WhatsAppClient::with_base_url(&client_config(true), base_url).unwrap();
        assert!(matches!(
            client.download_media("media", 10).await,
            Err(WhatsAppError::Api {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                ..
            })
        ));
    }
}
