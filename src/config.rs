use std::{collections::HashMap, env, fmt, net::SocketAddr, str::FromStr, time::Duration};

use thiserror::Error;

use crate::chat::ChatProvider;

pub const MAX_DOCUMENT_BYTES: u64 = 26_214_400;

#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: Secret,
    pub bind_addr: SocketAddr,
    pub chat_provider: ChatProvider,
    pub knowledge_space_id: String,
    pub telegram_bot_token: Option<Secret>,
    pub telegram_webhook_secret: Option<Secret>,
    pub telegram_group_id: Option<String>,
    pub telegram_allowed_user_ids: Vec<String>,
    pub telegram_bot_username: Option<String>,
    pub whatsapp_verify_token: Option<Secret>,
    pub whatsapp_app_secret: Option<Secret>,
    pub whatsapp_access_token: Option<Secret>,
    pub whatsapp_phone_number_id: Option<String>,
    pub whatsapp_waba_id: Option<String>,
    pub whatsapp_group_id: Option<String>,
    pub whatsapp_allowed_user_ids: Vec<String>,
    pub meta_graph_api_version: String,
    pub openai_api_key: Option<Secret>,
    pub openai_response_model: String,
    pub openai_embedding_model: String,
    pub openai_embedding_dimensions: usize,
    pub bot_mention: String,
    pub webhook_max_body_bytes: usize,
    pub document_max_bytes: u64,
    pub worker_poll_interval: Duration,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("{0} is required")]
    Missing(&'static str),
    #[error("{name} is invalid: {message}")]
    Invalid { name: &'static str, message: String },
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_map(env::vars().collect())
    }

    pub fn from_map(values: HashMap<String, String>) -> Result<Self, ConfigError> {
        let required =
            |name: &'static str| optional(&values, name).ok_or(ConfigError::Missing(name));
        let parse = |name: &'static str, default: &str| {
            values
                .get(name)
                .map(String::as_str)
                .unwrap_or(default)
                .parse::<usize>()
                .map_err(|error| ConfigError::Invalid {
                    name,
                    message: error.to_string(),
                })
        };

        let chat_provider = values
            .get("CHAT_PROVIDER")
            .map(String::as_str)
            .unwrap_or("telegram")
            .parse::<ChatProvider>()
            .map_err(|message| ConfigError::Invalid {
                name: "CHAT_PROVIDER",
                message: message.into(),
            })?;
        let bind_addr = values
            .get("BIND_ADDR")
            .map(String::as_str)
            .unwrap_or("0.0.0.0:8080")
            .parse()
            .map_err(|error: std::net::AddrParseError| ConfigError::Invalid {
                name: "BIND_ADDR",
                message: error.to_string(),
            })?;
        let poll_ms = parse("WORKER_POLL_INTERVAL_MS", "1000")?;
        if poll_ms == 0 {
            return Err(ConfigError::Invalid {
                name: "WORKER_POLL_INTERVAL_MS",
                message: "must be greater than zero".into(),
            });
        }
        let openai_embedding_dimensions = parse("OPENAI_EMBEDDING_DIMENSIONS", "1536")?;
        if openai_embedding_dimensions != 1536 {
            return Err(ConfigError::Invalid {
                name: "OPENAI_EMBEDDING_DIMENSIONS",
                message: "must be 1536 to match the database vector column".into(),
            });
        }
        let document_max_bytes = parse("DOCUMENT_MAX_BYTES", "26214400")? as u64;
        if !(1..=MAX_DOCUMENT_BYTES).contains(&document_max_bytes) {
            return Err(ConfigError::Invalid {
                name: "DOCUMENT_MAX_BYTES",
                message: format!("must be between 1 and {MAX_DOCUMENT_BYTES}"),
            });
        }

        let whatsapp_allowed_user_ids = optional(&values, "WHATSAPP_ALLOWED_USER_IDS")
            .or_else(|| optional(&values, "ALLOWED_WHATSAPP_IDS"))
            .map(|value| parse_list(&value))
            .unwrap_or_default();
        let config = Self {
            database_url: Secret(required("DATABASE_URL")?),
            bind_addr,
            chat_provider,
            knowledge_space_id: required("KNOWLEDGE_SPACE_ID")?,
            telegram_bot_token: optional_secret(&values, "TELEGRAM_BOT_TOKEN"),
            telegram_webhook_secret: optional_secret(&values, "TELEGRAM_WEBHOOK_SECRET"),
            telegram_group_id: optional(&values, "TELEGRAM_GROUP_ID"),
            telegram_allowed_user_ids: optional(&values, "TELEGRAM_ALLOWED_USER_IDS")
                .map(|value| parse_list(&value))
                .unwrap_or_default(),
            telegram_bot_username: optional(&values, "TELEGRAM_BOT_USERNAME")
                .map(|value| value.trim_start_matches('@').to_owned()),
            whatsapp_verify_token: optional_secret(&values, "WHATSAPP_VERIFY_TOKEN"),
            whatsapp_app_secret: optional_secret(&values, "WHATSAPP_APP_SECRET"),
            whatsapp_access_token: optional_secret(&values, "WHATSAPP_ACCESS_TOKEN"),
            whatsapp_phone_number_id: optional(&values, "WHATSAPP_PHONE_NUMBER_ID"),
            whatsapp_waba_id: optional(&values, "WHATSAPP_WABA_ID"),
            whatsapp_group_id: optional(&values, "WHATSAPP_GROUP_ID"),
            whatsapp_allowed_user_ids,
            meta_graph_api_version: values
                .get("META_GRAPH_API_VERSION")
                .cloned()
                .unwrap_or_else(|| "v25.0".into()),
            openai_api_key: optional_secret(&values, "OPENAI_API_KEY"),
            openai_response_model: values
                .get("OPENAI_RESPONSE_MODEL")
                .cloned()
                .unwrap_or_else(|| "gpt-5.6-sol".into()),
            openai_embedding_model: values
                .get("OPENAI_EMBEDDING_MODEL")
                .cloned()
                .unwrap_or_else(|| "text-embedding-3-small".into()),
            openai_embedding_dimensions,
            bot_mention: values
                .get("BOT_MENTION")
                .cloned()
                .unwrap_or_else(|| "@agora".into()),
            webhook_max_body_bytes: parse("WEBHOOK_MAX_BODY_BYTES", "1048576")?,
            document_max_bytes,
            worker_poll_interval: Duration::from_millis(poll_ms as u64),
        };
        config.validate_active_provider()?;
        Ok(config)
    }

    pub fn active_provider_ready(&self) -> bool {
        match self.chat_provider {
            ChatProvider::Telegram => {
                self.telegram_bot_token.is_some()
                    && self.telegram_webhook_secret.is_some()
                    && self.telegram_group_id.is_some()
                    && !self.telegram_allowed_user_ids.is_empty()
                    && self.telegram_bot_username.is_some()
            }
            ChatProvider::WhatsApp => {
                self.whatsapp_verify_token.is_some()
                    && self.whatsapp_app_secret.is_some()
                    && self.whatsapp_access_token.is_some()
                    && self.whatsapp_phone_number_id.is_some()
                    && self.whatsapp_waba_id.is_some()
                    && self.whatsapp_group_id.is_some()
                    && !self.whatsapp_allowed_user_ids.is_empty()
            }
        }
    }

    pub fn openai_ready(&self) -> bool {
        self.openai_api_key.is_some()
    }

    pub fn safety_identifier_secret(&self) -> &Secret {
        match self.chat_provider {
            ChatProvider::Telegram => self
                .telegram_webhook_secret
                .as_ref()
                .expect("validated Telegram configuration"),
            ChatProvider::WhatsApp => self
                .whatsapp_app_secret
                .as_ref()
                .expect("validated WhatsApp configuration"),
        }
    }

    fn validate_active_provider(&self) -> Result<(), ConfigError> {
        let missing = match self.chat_provider {
            ChatProvider::Telegram => [
                (self.telegram_bot_token.is_none(), "TELEGRAM_BOT_TOKEN"),
                (
                    self.telegram_webhook_secret.is_none(),
                    "TELEGRAM_WEBHOOK_SECRET",
                ),
                (self.telegram_group_id.is_none(), "TELEGRAM_GROUP_ID"),
                (
                    self.telegram_allowed_user_ids.is_empty(),
                    "TELEGRAM_ALLOWED_USER_IDS",
                ),
                (
                    self.telegram_bot_username.is_none(),
                    "TELEGRAM_BOT_USERNAME",
                ),
            ]
            .into_iter()
            .find_map(|(missing, name)| missing.then_some(name)),
            ChatProvider::WhatsApp => [
                (
                    self.whatsapp_verify_token.is_none(),
                    "WHATSAPP_VERIFY_TOKEN",
                ),
                (self.whatsapp_app_secret.is_none(), "WHATSAPP_APP_SECRET"),
                (
                    self.whatsapp_access_token.is_none(),
                    "WHATSAPP_ACCESS_TOKEN",
                ),
                (
                    self.whatsapp_phone_number_id.is_none(),
                    "WHATSAPP_PHONE_NUMBER_ID",
                ),
                (self.whatsapp_waba_id.is_none(), "WHATSAPP_WABA_ID"),
                (self.whatsapp_group_id.is_none(), "WHATSAPP_GROUP_ID"),
                (
                    self.whatsapp_allowed_user_ids.is_empty(),
                    "WHATSAPP_ALLOWED_USER_IDS",
                ),
            ]
            .into_iter()
            .find_map(|(missing, name)| missing.then_some(name)),
        };
        if let Some(name) = missing {
            return Err(ConfigError::Missing(name));
        }
        if self.chat_provider == ChatProvider::Telegram {
            let secret = self
                .telegram_webhook_secret
                .as_ref()
                .expect("checked above")
                .expose();
            if secret.len() > 256
                || !secret
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                return Err(ConfigError::Invalid {
                    name: "TELEGRAM_WEBHOOK_SECRET",
                    message: "must contain 1-256 ASCII letters, digits, underscores or hyphens"
                        .into(),
                });
            }
            let valid_group = self
                .telegram_group_id
                .as_deref()
                .and_then(|value| value.parse::<i64>().ok())
                .is_some_and(|value| value < 0);
            if !valid_group {
                return Err(ConfigError::Invalid {
                    name: "TELEGRAM_GROUP_ID",
                    message: "must be a negative Telegram group identifier".into(),
                });
            }
        }
        Ok(())
    }
}

fn parse_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect()
}

fn optional(values: &HashMap<String, String>, name: &str) -> Option<String> {
    values
        .get(name)
        .filter(|value| !value.trim().is_empty())
        .cloned()
}

fn optional_secret(values: &HashMap<String, String>, name: &str) -> Option<Secret> {
    optional(values, name).map(Secret)
}

impl FromStr for Secret {
    type Err = std::convert::Infallible;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self(value.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn telegram() -> HashMap<String, String> {
        HashMap::from([
            ("DATABASE_URL".into(), "postgres://localhost/agora".into()),
            ("KNOWLEDGE_SPACE_ID".into(), "agora".into()),
            ("TELEGRAM_BOT_TOKEN".into(), "telegram-token".into()),
            ("TELEGRAM_WEBHOOK_SECRET".into(), "telegram-secret".into()),
            ("TELEGRAM_GROUP_ID".into(), "-1001".into()),
            ("TELEGRAM_ALLOWED_USER_IDS".into(), "42,43".into()),
            ("TELEGRAM_BOT_USERNAME".into(), "agora_bot".into()),
        ])
    }

    fn both() -> HashMap<String, String> {
        let mut values = telegram();
        values.extend([
            (
                "WHATSAPP_VERIFY_TOKEN".into(),
                "whatsapp-verify-secret".into(),
            ),
            ("WHATSAPP_APP_SECRET".into(), "meta-secret".into()),
            ("WHATSAPP_ACCESS_TOKEN".into(), "meta-token".into()),
            ("WHATSAPP_PHONE_NUMBER_ID".into(), "phone".into()),
            ("WHATSAPP_WABA_ID".into(), "waba".into()),
            ("WHATSAPP_GROUP_ID".into(), "group".into()),
            ("WHATSAPP_ALLOWED_USER_IDS".into(), "5491,5492".into()),
        ]);
        values
    }

    #[test]
    fn defaults_to_telegram_and_loads_provider_configuration() {
        let config = Config::from_map(telegram()).unwrap();

        assert_eq!(config.chat_provider, ChatProvider::Telegram);
        assert_eq!(config.knowledge_space_id, "agora");
        assert_eq!(config.telegram_allowed_user_ids, ["42", "43"]);
        assert!(config.active_provider_ready());
        assert!(!config.openai_ready());
    }

    #[test]
    fn validates_both_provider_selections_and_rejects_unknown_values() {
        let mut values = both();
        values.insert("CHAT_PROVIDER".into(), "whatsapp".into());
        assert_eq!(
            Config::from_map(values).unwrap().chat_provider,
            ChatProvider::WhatsApp
        );

        let mut missing = telegram();
        missing.remove("TELEGRAM_BOT_TOKEN");
        assert_eq!(
            Config::from_map(missing).unwrap_err(),
            ConfigError::Missing("TELEGRAM_BOT_TOKEN")
        );

        let mut invalid = telegram();
        invalid.insert("CHAT_PROVIDER".into(), "signal".into());
        assert!(matches!(
            Config::from_map(invalid),
            Err(ConfigError::Invalid {
                name: "CHAT_PROVIDER",
                ..
            })
        ));

        let mut invalid_secret = telegram();
        invalid_secret.insert("TELEGRAM_WEBHOOK_SECRET".into(), "contains spaces".into());
        assert!(matches!(
            Config::from_map(invalid_secret),
            Err(ConfigError::Invalid {
                name: "TELEGRAM_WEBHOOK_SECRET",
                ..
            })
        ));
    }

    #[test]
    fn switching_only_chat_provider_selects_the_other_complete_block() {
        let telegram = Config::from_map(both()).unwrap();
        let mut values = both();
        values.insert("CHAT_PROVIDER".into(), "whatsapp".into());
        let whatsapp = Config::from_map(values).unwrap();

        assert_eq!(telegram.chat_provider, ChatProvider::Telegram);
        assert_eq!(whatsapp.chat_provider, ChatProvider::WhatsApp);
        assert_eq!(telegram.knowledge_space_id, whatsapp.knowledge_space_id);
    }

    #[test]
    fn supports_the_deprecated_whatsapp_allowlist_alias() {
        let mut values = both();
        values.insert("CHAT_PROVIDER".into(), "whatsapp".into());
        values.remove("WHATSAPP_ALLOWED_USER_IDS");
        values.insert("ALLOWED_WHATSAPP_IDS".into(), " 5491,5492, ".into());

        assert_eq!(
            Config::from_map(values).unwrap().whatsapp_allowed_user_ids,
            ["5491", "5492"]
        );
    }

    #[test]
    fn secrets_are_redacted_in_debug_output() {
        let config = Config::from_map(both()).unwrap();
        let debug = format!("{config:?}");

        for secret in [
            "postgres://localhost/agora",
            "telegram-token",
            "telegram-secret",
            "meta-token",
            "meta-secret",
            "whatsapp-verify-secret",
        ] {
            assert!(!debug.contains(secret));
        }
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn rejects_missing_database_and_space() {
        assert_eq!(
            Config::from_map(HashMap::new()).unwrap_err(),
            ConfigError::Missing("DATABASE_URL")
        );
        let mut values = telegram();
        values.remove("KNOWLEDGE_SPACE_ID");
        assert_eq!(
            Config::from_map(values).unwrap_err(),
            ConfigError::Missing("KNOWLEDGE_SPACE_ID")
        );
    }

    #[test]
    fn rejects_invalid_limits() {
        for (name, invalid) in [
            ("WORKER_POLL_INTERVAL_MS", "0"),
            ("OPENAI_EMBEDDING_DIMENSIONS", "3072"),
            ("DOCUMENT_MAX_BYTES", "26214401"),
        ] {
            let mut values = telegram();
            values.insert(name.into(), invalid.into());
            assert!(matches!(
                Config::from_map(values),
                Err(ConfigError::Invalid { name: field, .. }) if field == name
            ));
        }
    }
}
