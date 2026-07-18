use std::{collections::HashMap, env, fmt, net::SocketAddr, str::FromStr, time::Duration};

use thiserror::Error;

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
    pub whatsapp_verify_token: Secret,
    pub whatsapp_app_secret: Secret,
    pub whatsapp_access_token: Option<Secret>,
    pub whatsapp_phone_number_id: Option<String>,
    pub whatsapp_waba_id: Option<String>,
    pub whatsapp_group_id: Option<String>,
    pub meta_graph_api_version: String,
    pub openai_api_key: Option<Secret>,
    pub openai_response_model: String,
    pub openai_embedding_model: String,
    pub openai_embedding_dimensions: usize,
    pub bot_mention: String,
    pub allowed_whatsapp_ids: Vec<String>,
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
        let required = |name: &'static str| {
            values
                .get(name)
                .filter(|value| !value.trim().is_empty())
                .cloned()
                .ok_or(ConfigError::Missing(name))
        };
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

        Ok(Self {
            database_url: Secret(required("DATABASE_URL")?),
            bind_addr,
            whatsapp_verify_token: Secret(required("WHATSAPP_VERIFY_TOKEN")?),
            whatsapp_app_secret: Secret(required("WHATSAPP_APP_SECRET")?),
            whatsapp_access_token: optional_secret(&values, "WHATSAPP_ACCESS_TOKEN"),
            whatsapp_phone_number_id: optional(&values, "WHATSAPP_PHONE_NUMBER_ID"),
            whatsapp_waba_id: optional(&values, "WHATSAPP_WABA_ID"),
            whatsapp_group_id: optional(&values, "WHATSAPP_GROUP_ID"),
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
            allowed_whatsapp_ids: values
                .get("ALLOWED_WHATSAPP_IDS")
                .map(|value| {
                    value
                        .split(',')
                        .map(str::trim)
                        .filter(|id| !id.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            webhook_max_body_bytes: parse("WEBHOOK_MAX_BODY_BYTES", "1048576")?,
            document_max_bytes,
            worker_poll_interval: Duration::from_millis(poll_ms as u64),
        })
    }

    pub fn whatsapp_ready(&self) -> bool {
        self.whatsapp_access_token.is_some()
            && self.whatsapp_phone_number_id.is_some()
            && self.whatsapp_waba_id.is_some()
    }

    pub fn openai_ready(&self) -> bool {
        self.openai_api_key.is_some()
    }
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

    fn minimum() -> HashMap<String, String> {
        HashMap::from([
            ("DATABASE_URL".into(), "postgres://localhost/agora".into()),
            ("WHATSAPP_VERIFY_TOKEN".into(), "verify".into()),
            ("WHATSAPP_APP_SECRET".into(), "secret".into()),
        ])
    }

    #[test]
    fn loads_defaults_without_optional_providers() {
        let config = Config::from_map(minimum()).unwrap();

        assert_eq!(config.bind_addr, "0.0.0.0:8080".parse().unwrap());
        assert_eq!(config.meta_graph_api_version, "v25.0");
        assert_eq!(config.openai_response_model, "gpt-5.6-sol");
        assert_eq!(config.openai_embedding_dimensions, 1536);
        assert!(!config.whatsapp_ready());
        assert!(!config.openai_ready());
    }

    #[test]
    fn rejects_missing_required_values() {
        let error = Config::from_map(HashMap::new()).unwrap_err();
        assert_eq!(error, ConfigError::Missing("DATABASE_URL"));
    }

    #[test]
    fn parses_allowlist_and_overrides() {
        let mut values = minimum();
        values.insert("ALLOWED_WHATSAPP_IDS".into(), " 5491,5492, ".into());
        values.insert("BIND_ADDR".into(), "127.0.0.1:9000".into());
        values.insert("WORKER_POLL_INTERVAL_MS".into(), "250".into());

        let config = Config::from_map(values).unwrap();

        assert_eq!(config.allowed_whatsapp_ids, ["5491", "5492"]);
        assert_eq!(config.bind_addr.port(), 9000);
        assert_eq!(config.worker_poll_interval, Duration::from_millis(250));
    }

    #[test]
    fn secrets_are_redacted_in_debug_output() {
        let config = Config::from_map(minimum()).unwrap();
        let debug = format!("{config:?}");

        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("postgres://localhost/agora"));
    }

    #[test]
    fn rejects_zero_worker_interval() {
        let mut values = minimum();
        values.insert("WORKER_POLL_INTERVAL_MS".into(), "0".into());

        assert!(matches!(
            Config::from_map(values),
            Err(ConfigError::Invalid {
                name: "WORKER_POLL_INTERVAL_MS",
                ..
            })
        ));
    }

    #[test]
    fn rejects_embedding_dimensions_that_do_not_match_the_schema() {
        let mut values = minimum();
        values.insert("OPENAI_EMBEDDING_DIMENSIONS".into(), "3072".into());

        assert_eq!(
            Config::from_map(values).unwrap_err(),
            ConfigError::Invalid {
                name: "OPENAI_EMBEDDING_DIMENSIONS",
                message: "must be 1536 to match the database vector column".into(),
            }
        );
    }

    #[test]
    fn rejects_document_limits_that_do_not_fit_in_postgres() {
        for invalid in ["0", "26214401"] {
            let mut values = minimum();
            values.insert("DOCUMENT_MAX_BYTES".into(), invalid.into());

            assert_eq!(
                Config::from_map(values).unwrap_err(),
                ConfigError::Invalid {
                    name: "DOCUMENT_MAX_BYTES",
                    message: "must be between 1 and 26214400".into(),
                }
            );
        }
    }
}
