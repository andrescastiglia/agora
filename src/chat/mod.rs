use std::{fmt, str::FromStr};

use anyhow::Context;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::config::Config;

pub mod telegram;
pub mod whatsapp;

pub const TELEGRAM_MAX_DOCUMENT_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatProvider {
    Telegram,
    WhatsApp,
}

impl ChatProvider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::WhatsApp => "whatsapp",
        }
    }

    pub const fn message_limit(self) -> usize {
        match self {
            Self::Telegram => 4_096,
            Self::WhatsApp => 4_000,
        }
    }
}

impl fmt::Display for ChatProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ChatProvider {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "telegram" => Ok(Self::Telegram),
            "whatsapp" => Ok(Self::WhatsApp),
            _ => Err("must be telegram or whatsapp"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingDocument {
    pub provider_media_id: String,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub sha256: Option<String>,
    pub file_size: Option<u64>,
    pub caption: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IncomingMessage {
    pub provider: ChatProvider,
    pub external_message_id: String,
    pub conversation_id: String,
    pub space_id: String,
    pub sender_id: String,
    pub sender_name: Option<String>,
    pub kind: String,
    pub text: Option<String>,
    pub document: Option<IncomingDocument>,
    pub timestamp: DateTime<Utc>,
    pub reply_to_message_id: Option<String>,
    pub metadata: Value,
}

impl IncomingMessage {
    pub fn effective_text(&self) -> Option<&str> {
        self.text.as_deref().or_else(|| {
            self.document
                .as_ref()
                .and_then(|document| document.caption.as_deref())
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IncomingStatus {
    pub provider: ChatProvider,
    pub provider_message_id: String,
    pub status: String,
    pub timestamp: Option<DateTime<Utc>>,
    pub recipient_id: Option<String>,
    pub recipient_type: Option<String>,
    pub error: Option<Value>,
}

#[derive(Debug, Default)]
pub struct ParsedEvent {
    pub messages: Vec<IncomingMessage>,
    pub statuses: Vec<IncomingStatus>,
}

pub fn parse_event(
    provider: ChatProvider,
    payload: &Value,
    space_id: &str,
) -> anyhow::Result<ParsedEvent> {
    match provider {
        ChatProvider::Telegram => telegram::parse_event(payload, space_id),
        ChatProvider::WhatsApp => whatsapp::parse_event(payload, space_id),
    }
}

pub fn question_for_bot(provider: ChatProvider, text: &str, config: &Config) -> Option<String> {
    match provider {
        ChatProvider::Telegram => {
            telegram::question_for_bot(text, config.telegram_bot_username.as_deref()?)
        }
        ChatProvider::WhatsApp => whatsapp::question_for_bot(text, &config.bot_mention),
    }
}

pub fn supported_document(filename: Option<&str>, mime_type: Option<&str>) -> bool {
    whatsapp::supported_document(filename, mime_type)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentMessage {
    pub external_message_id: String,
}

#[derive(Debug)]
pub struct DownloadedDocument {
    pub bytes: Vec<u8>,
    pub mime_type: Option<String>,
    pub sha256: Option<String>,
}

#[async_trait]
pub trait ChatClient: Send + Sync {
    async fn download_document(
        &self,
        media_id: &str,
        maximum_bytes: u64,
    ) -> anyhow::Result<DownloadedDocument>;

    async fn send_text(
        &self,
        conversation_id: &str,
        body: &str,
        reply_to_message_id: Option<&str>,
    ) -> anyhow::Result<SentMessage>;
}

pub enum ProviderClient {
    Telegram(telegram::TelegramClient),
    WhatsApp(whatsapp::WhatsAppClient),
}

impl ProviderClient {
    pub fn from_config(provider: ChatProvider, config: &Config) -> anyhow::Result<Self> {
        match provider {
            ChatProvider::Telegram => Ok(Self::Telegram(
                telegram::TelegramClient::from_config(config)
                    .context("Telegram client is not configured")?,
            )),
            ChatProvider::WhatsApp => Ok(Self::WhatsApp(
                whatsapp::WhatsAppClient::from_config(config)
                    .context("WhatsApp client is not configured")?,
            )),
        }
    }
}

#[async_trait]
impl ChatClient for ProviderClient {
    async fn download_document(
        &self,
        media_id: &str,
        maximum_bytes: u64,
    ) -> anyhow::Result<DownloadedDocument> {
        match self {
            Self::Telegram(client) => client
                .download_document(media_id, maximum_bytes)
                .await
                .map_err(Into::into),
            Self::WhatsApp(client) => {
                let (bytes, mime_type, sha256) =
                    client.download_media(media_id, maximum_bytes).await?;
                Ok(DownloadedDocument {
                    bytes,
                    mime_type,
                    sha256,
                })
            }
        }
    }

    async fn send_text(
        &self,
        conversation_id: &str,
        body: &str,
        reply_to_message_id: Option<&str>,
    ) -> anyhow::Result<SentMessage> {
        match self {
            Self::Telegram(client) => client
                .send_text(conversation_id, body, reply_to_message_id)
                .await
                .map_err(Into::into),
            Self::WhatsApp(client) => {
                let sent = client.send_group_text(conversation_id, body).await?;
                Ok(SentMessage {
                    external_message_id: sent.id,
                })
            }
        }
    }
}
