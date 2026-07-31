use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use crate::config::Config;

use super::{
    ChatProvider, DownloadedDocument, IncomingDocument, IncomingMessage, ParsedEvent, SentMessage,
    TELEGRAM_MAX_DOCUMENT_BYTES,
};

#[derive(Debug, Deserialize)]
struct Update {
    #[allow(dead_code)]
    update_id: i64,
    message: Option<Message>,
}

#[derive(Debug, Deserialize)]
struct Message {
    message_id: i64,
    date: i64,
    chat: Chat,
    #[serde(rename = "from")]
    sender: Option<User>,
    text: Option<String>,
    caption: Option<String>,
    document: Option<Document>,
    reply_to_message: Option<ReplyMessage>,
}

#[derive(Debug, Deserialize)]
struct Chat {
    id: i64,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct User {
    id: i64,
    first_name: String,
    last_name: Option<String>,
    username: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Document {
    file_id: String,
    file_name: Option<String>,
    mime_type: Option<String>,
    file_size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ReplyMessage {
    message_id: i64,
}

pub fn update_id(payload: &Value) -> Option<String> {
    payload.get("update_id")?.as_i64().map(|id| id.to_string())
}

pub fn parse_event(payload: &Value, space_id: &str) -> anyhow::Result<ParsedEvent> {
    let update: Update = serde_json::from_value(payload.clone())?;
    let Some(message) = update.message else {
        return Ok(ParsedEvent::default());
    };
    if !matches!(message.chat.kind.as_str(), "group" | "supergroup") {
        return Ok(ParsedEvent::default());
    }
    let Some(sender) = message.sender else {
        return Ok(ParsedEvent::default());
    };
    if message.text.is_none() && message.document.is_none() {
        return Ok(ParsedEvent::default());
    }
    let Some(timestamp) = chrono::DateTime::from_timestamp(message.date, 0) else {
        return Ok(ParsedEvent::default());
    };
    let sender_name = sender.username.or_else(|| {
        Some(match sender.last_name {
            Some(last_name) => format!("{} {last_name}", sender.first_name),
            None => sender.first_name,
        })
    });
    let document = message.document.map(|document| IncomingDocument {
        provider_media_id: document.file_id,
        filename: document.file_name,
        mime_type: document.mime_type,
        sha256: None,
        file_size: document.file_size,
        caption: message.caption,
    });
    let kind = if document.is_some() {
        "document"
    } else {
        "text"
    };
    Ok(ParsedEvent {
        messages: vec![IncomingMessage {
            provider: ChatProvider::Telegram,
            external_message_id: message.message_id.to_string(),
            conversation_id: message.chat.id.to_string(),
            space_id: space_id.to_owned(),
            sender_id: sender.id.to_string(),
            sender_name,
            kind: kind.into(),
            text: message.text,
            document,
            timestamp,
            reply_to_message_id: message
                .reply_to_message
                .map(|reply| reply.message_id.to_string()),
            metadata: serde_json::json!({}),
        }],
        statuses: Vec::new(),
    })
}

pub fn question_for_bot(text: &str, bot_username: &str) -> Option<String> {
    let trimmed = text.trim();
    let lower = trimmed.to_ascii_lowercase();
    let username = bot_username
        .trim()
        .trim_start_matches('@')
        .to_ascii_lowercase();
    let prefixes = [
        format!("/agora@{username}"),
        "/agora".into(),
        format!("@{username}"),
    ];

    prefixes.iter().find_map(|prefix| {
        let rest = lower.strip_prefix(prefix)?;
        if rest.chars().next().is_some_and(|character| {
            !character.is_whitespace() && character != ':' && character != ','
        }) {
            return None;
        }
        let question = trimmed
            .get(prefix.len()..)?
            .trim_start_matches([' ', '\t', ':', ','])
            .trim();
        (!question.is_empty()).then(|| question.to_owned())
    })
}

#[derive(Clone)]
pub struct TelegramClient {
    http: reqwest::Client,
    token: String,
    base_url: String,
}

#[derive(Debug, Error)]
pub enum TelegramError {
    #[error("Telegram API configuration is incomplete")]
    NotConfigured,
    #[error("Telegram request failed")]
    Transport,
    #[error("Telegram returned HTTP {0}")]
    Api(reqwest::StatusCode),
    #[error("Telegram response did not include the expected field")]
    InvalidResponse,
    #[error("Telegram document exceeds the 20 MiB download limit")]
    TooLarge,
}

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    ok: bool,
    result: Option<T>,
}

#[derive(Debug, Deserialize)]
struct TelegramFile {
    file_size: Option<u64>,
    file_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramSentMessage {
    message_id: i64,
}

impl TelegramClient {
    pub fn from_config(config: &Config) -> Result<Self, TelegramError> {
        let token = config
            .telegram_bot_token
            .as_ref()
            .ok_or(TelegramError::NotConfigured)?
            .expose()
            .to_owned();
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|_| TelegramError::Transport)?;
        Ok(Self {
            http,
            token,
            base_url: "https://api.telegram.org".into(),
        })
    }

    #[cfg(test)]
    pub fn with_base_url(config: &Config, base_url: String) -> Result<Self, TelegramError> {
        let mut client = Self::from_config(config)?;
        client.base_url = base_url;
        Ok(client)
    }

    pub async fn download_document(
        &self,
        file_id: &str,
        maximum_bytes: u64,
    ) -> Result<DownloadedDocument, TelegramError> {
        let maximum_bytes = maximum_bytes.min(TELEGRAM_MAX_DOCUMENT_BYTES);
        let response = self
            .http
            .post(format!("{}/bot{}/getFile", self.base_url, self.token))
            .json(&serde_json::json!({"file_id": file_id}))
            .send()
            .await
            .map_err(|_| TelegramError::Transport)?;
        let status = response.status();
        if !status.is_success() {
            return Err(TelegramError::Api(status));
        }
        let response = response
            .json::<ApiResponse<TelegramFile>>()
            .await
            .map_err(|_| TelegramError::InvalidResponse)?;
        let file = response
            .ok
            .then_some(response.result)
            .flatten()
            .ok_or(TelegramError::InvalidResponse)?;
        if file.file_size.is_some_and(|size| size > maximum_bytes) {
            return Err(TelegramError::TooLarge);
        }
        let file_path = file.file_path.ok_or(TelegramError::InvalidResponse)?;
        let mut response = self
            .http
            .get(format!(
                "{}/file/bot{}/{}",
                self.base_url,
                self.token,
                file_path.trim_start_matches('/')
            ))
            .send()
            .await
            .map_err(|_| TelegramError::Transport)?;
        let status = response.status();
        if !status.is_success() {
            return Err(TelegramError::Api(status));
        }
        if response
            .content_length()
            .is_some_and(|size| size > maximum_bytes)
        {
            return Err(TelegramError::TooLarge);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| TelegramError::Transport)?
        {
            if bytes.len().saturating_add(chunk.len()) as u64 > maximum_bytes {
                return Err(TelegramError::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(DownloadedDocument {
            bytes,
            mime_type: None,
            sha256: None,
        })
    }

    pub async fn send_text(
        &self,
        conversation_id: &str,
        body: &str,
        reply_to_message_id: Option<&str>,
    ) -> Result<SentMessage, TelegramError> {
        let reply_parameters = reply_to_message_id
            .and_then(|message_id| message_id.parse::<i64>().ok())
            .map(|message_id| {
                serde_json::json!({
                    "message_id": message_id,
                    "allow_sending_without_reply": true,
                })
            });
        let mut request = serde_json::json!({
            "chat_id": conversation_id,
            "text": body,
        });
        if let Some(reply_parameters) = reply_parameters {
            request["reply_parameters"] = reply_parameters;
        }
        let response = self
            .http
            .post(format!("{}/bot{}/sendMessage", self.base_url, self.token))
            .json(&request)
            .send()
            .await
            .map_err(|_| TelegramError::Transport)?;
        let status = response.status();
        if !status.is_success() {
            return Err(TelegramError::Api(status));
        }
        let response = response
            .json::<ApiResponse<TelegramSentMessage>>()
            .await
            .map_err(|_| TelegramError::InvalidResponse)?;
        let message = response
            .ok
            .then_some(response.result)
            .flatten()
            .ok_or(TelegramError::InvalidResponse)?;
        Ok(SentMessage {
            external_message_id: message.message_id.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use axum::{
        Json, Router,
        http::StatusCode,
        routing::{get, post},
    };
    use serde_json::json;

    use super::*;

    fn config() -> Config {
        Config::from_map(HashMap::from([
            ("DATABASE_URL".into(), "postgres://localhost/agora".into()),
            ("KNOWLEDGE_SPACE_ID".into(), "agora".into()),
            ("TELEGRAM_BOT_TOKEN".into(), "test-token".into()),
            ("TELEGRAM_WEBHOOK_SECRET".into(), "webhook-secret".into()),
            ("TELEGRAM_GROUP_ID".into(), "-1001".into()),
            ("TELEGRAM_ALLOWED_USER_IDS".into(), "42".into()),
            ("TELEGRAM_BOT_USERNAME".into(), "agora_bot".into()),
        ]))
        .unwrap()
    }

    async fn serve(build: impl FnOnce(String) -> Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        let router = build(base_url.clone());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        base_url
    }

    #[test]
    fn parses_group_text_captions_documents_and_invocations() {
        let text = json!({
            "update_id": 7,
            "message": {
                "message_id": 11,
                "date": 1700000000,
                "chat": {"id": -1001, "type": "supergroup"},
                "from": {"id": 42, "first_name": "Ana", "username": "ana"},
                "text": "@agora_bot ¿qué se decidió?"
            }
        });
        let parsed = parse_event(&text, "agora").unwrap();
        assert_eq!(parsed.messages[0].conversation_id, "-1001");
        assert_eq!(parsed.messages[0].sender_name.as_deref(), Some("ana"));
        assert_eq!(update_id(&text).as_deref(), Some("7"));
        assert_eq!(
            question_for_bot("@agora_bot: ¿qué se decidió?", "agora_bot"),
            Some("¿qué se decidió?".into())
        );
        assert_eq!(
            question_for_bot("/agora@agora_bot resumí", "agora_bot"),
            Some("resumí".into())
        );
        assert_eq!(question_for_bot("conversación normal", "agora_bot"), None);

        let document = json!({
            "update_id": 8,
            "message": {
                "message_id": 12,
                "date": 1700000001,
                "chat": {"id": -1001, "type": "group"},
                "from": {"id": 42, "first_name": "Ana"},
                "caption": "/agora resumí",
                "document": {
                    "file_id": "file-1",
                    "file_name": "informe.pdf",
                    "mime_type": "application/pdf",
                    "file_size": 1024
                }
            }
        });
        let parsed = parse_event(&document, "agora").unwrap();
        let document = parsed.messages[0].document.as_ref().unwrap();
        assert_eq!(document.caption.as_deref(), Some("/agora resumí"));
        assert_eq!(document.file_size, Some(1024));
    }

    #[test]
    fn rejects_private_chats_and_updates_without_messages() {
        let private = json!({
            "update_id": 1,
            "message": {
                "message_id": 2,
                "date": 1700000000,
                "chat": {"id": 42, "type": "private"},
                "from": {"id": 42, "first_name": "Ana"},
                "text": "hola"
            }
        });
        assert!(parse_event(&private, "agora").unwrap().messages.is_empty());
        assert!(
            parse_event(&json!({"update_id": 2}), "agora")
                .unwrap()
                .messages
                .is_empty()
        );

        let service_message = json!({
            "update_id": 3,
            "message": {
                "message_id": 10,
                "date": 1700000000,
                "chat": {"id": -1001, "type": "supergroup"},
                "from": {"id": 42, "first_name": "Ana"},
                "new_chat_members": [{"id": 99, "first_name": "Agora"}]
            }
        });
        assert!(
            parse_event(&service_message, "agora")
                .unwrap()
                .messages
                .is_empty()
        );

        let edited_message = json!({
            "update_id": 4,
            "edited_message": {
                "message_id": 11,
                "date": 1700000001,
                "chat": {"id": -1001, "type": "supergroup"},
                "from": {"id": 42, "first_name": "Ana"},
                "text": "/agora texto editado"
            }
        });
        assert!(
            parse_event(&edited_message, "agora")
                .unwrap()
                .messages
                .is_empty()
        );
    }

    #[tokio::test]
    async fn downloads_and_sends_through_the_bot_api() {
        async fn get_file() -> Json<Value> {
            Json(json!({
                "ok": true,
                "result": {"file_size": 4, "file_path": "docs/file.pdf"}
            }))
        }
        async fn download() -> &'static [u8] {
            b"data"
        }
        async fn send(Json(body): Json<Value>) -> Json<Value> {
            assert_eq!(body["chat_id"], "-1001");
            assert_eq!(body["reply_parameters"]["message_id"], 11);
            Json(json!({"ok": true, "result": {"message_id": 99}}))
        }
        let base_url = serve(|_| {
            Router::new()
                .route("/bottest-token/getFile", post(get_file))
                .route("/file/bottest-token/docs/file.pdf", get(download))
                .route("/bottest-token/sendMessage", post(send))
        })
        .await;
        let client = TelegramClient::with_base_url(&config(), base_url).unwrap();
        assert_eq!(
            client
                .download_document("file-1", TELEGRAM_MAX_DOCUMENT_BYTES)
                .await
                .unwrap()
                .bytes,
            b"data"
        );
        assert_eq!(
            client
                .send_text("-1001", "Respuesta", Some("11"))
                .await
                .unwrap()
                .external_message_id,
            "99"
        );
    }

    #[tokio::test]
    async fn enforces_the_telegram_limit_and_sanitizes_errors() {
        async fn oversized() -> Json<Value> {
            Json(json!({
                "ok": true,
                "result": {"file_size": TELEGRAM_MAX_DOCUMENT_BYTES + 1, "file_path": "x"}
            }))
        }
        let base_url =
            serve(|_| Router::new().route("/bottest-token/getFile", post(oversized))).await;
        let client = TelegramClient::with_base_url(&config(), base_url).unwrap();
        assert!(matches!(
            client
                .download_document("file", TELEGRAM_MAX_DOCUMENT_BYTES)
                .await,
            Err(TelegramError::TooLarge)
        ));

        async fn failure() -> StatusCode {
            StatusCode::UNAUTHORIZED
        }
        let base_url =
            serve(|_| Router::new().route("/bottest-token/sendMessage", post(failure))).await;
        let client = TelegramClient::with_base_url(&config(), base_url).unwrap();
        let error = client.send_text("-1001", "body", None).await.unwrap_err();
        assert!(!format!("{error:?}").contains("test-token"));
        assert!(!error.to_string().contains("test-token"));
    }
}
