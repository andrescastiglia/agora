use axum::{
    Json,
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    AppState,
    chat::{ChatProvider, telegram},
    repository::{persist_webhook_event, ping},
    security::{sha256_hex, verify_meta_signature, verify_telegram_secret},
};

#[derive(Debug, Deserialize)]
pub struct VerifyQuery {
    #[serde(rename = "hub.mode")]
    mode: String,
    #[serde(rename = "hub.verify_token")]
    verify_token: String,
    #[serde(rename = "hub.challenge")]
    challenge: String,
}

#[derive(Debug, Serialize)]
pub struct Health {
    status: &'static str,
}

pub async fn health() -> Json<Health> {
    Json(Health { status: "ok" })
}

pub async fn ready(State(state): State<AppState>) -> Response {
    if !state.config.active_provider_ready() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "unavailable"})),
        )
            .into_response();
    }
    match ping(&state.db).await {
        Ok(()) => (StatusCode::OK, Json(json!({"status": "ready"}))).into_response(),
        Err(error) => {
            tracing::error!(%error, "readiness database check failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"status": "unavailable"})),
            )
                .into_response()
        }
    }
}

pub async fn verify_whatsapp(
    State(state): State<AppState>,
    Query(query): Query<VerifyQuery>,
) -> Response {
    if query.mode == "subscribe"
        && state
            .config
            .whatsapp_verify_token
            .as_ref()
            .is_some_and(|secret| query.verify_token == secret.expose())
    {
        (StatusCode::OK, query.challenge).into_response()
    } else {
        (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "invalid verification request"})),
        )
            .into_response()
    }
}

pub async fn receive_whatsapp(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let signature = headers
        .get("x-hub-signature-256")
        .and_then(|value| value.to_str().ok());
    let Some(secret) = state.config.whatsapp_app_secret.as_ref() else {
        return provider_not_configured();
    };
    if let Err(error) = verify_meta_signature(signature, &body, secret.expose()) {
        tracing::warn!(%error, "rejected WhatsApp webhook signature");
        return unauthorized();
    }
    if state.config.chat_provider != ChatProvider::WhatsApp {
        return ignored_inactive();
    }

    let payload: Value = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(%error, "rejected malformed WhatsApp webhook");
            return invalid_json();
        }
    };
    let content_sha256 = sha256_hex(&body);
    persist(
        &state,
        ChatProvider::WhatsApp,
        &content_sha256,
        &payload,
        &content_sha256,
    )
    .await
}

pub async fn receive_telegram(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let header = headers
        .get("x-telegram-bot-api-secret-token")
        .and_then(|value| value.to_str().ok());
    let Some(secret) = state.config.telegram_webhook_secret.as_ref() else {
        return provider_not_configured();
    };
    if let Err(error) = verify_telegram_secret(header, secret.expose()) {
        tracing::warn!(%error, "rejected Telegram webhook secret");
        return unauthorized();
    }
    if state.config.chat_provider != ChatProvider::Telegram {
        return ignored_inactive();
    }

    let payload: Value = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(%error, "rejected malformed Telegram webhook");
            return invalid_json();
        }
    };
    let Some(provider_event_id) = telegram::update_id(&payload) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"accepted": false, "error": "missing update_id"})),
        )
            .into_response();
    };
    let content_sha256 = sha256_hex(&body);
    persist(
        &state,
        ChatProvider::Telegram,
        &provider_event_id,
        &payload,
        &content_sha256,
    )
    .await
}

async fn persist(
    state: &AppState,
    provider: ChatProvider,
    provider_event_id: &str,
    payload: &Value,
    content_sha256: &str,
) -> Response {
    match persist_webhook_event(
        &state.db,
        provider,
        provider_event_id,
        payload,
        content_sha256,
    )
    .await
    {
        Ok(inserted) => (
            StatusCode::OK,
            Json(json!({"accepted": true, "duplicate": !inserted})),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(%error, provider = %provider, "failed to persist chat webhook");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"accepted": false})),
            )
                .into_response()
        }
    }
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"accepted": false, "error": "invalid signature"})),
    )
        .into_response()
}

fn provider_not_configured() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"accepted": false})),
    )
        .into_response()
}

fn ignored_inactive() -> Response {
    (
        StatusCode::OK,
        Json(json!({"accepted": true, "ignored": true})),
    )
        .into_response()
}

fn invalid_json() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"accepted": false, "error": "invalid JSON"})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use hmac::{Hmac, KeyInit, Mac};
    use http_body_util::BodyExt;
    use sha2::Sha256;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use crate::{AppState, build_router, config::Config};

    fn app(provider: &str) -> axum::Router {
        let config = Config::from_map(HashMap::from([
            ("DATABASE_URL".into(), "postgres://localhost/agora".into()),
            ("KNOWLEDGE_SPACE_ID".into(), "agora".into()),
            ("CHAT_PROVIDER".into(), provider.into()),
            ("TELEGRAM_BOT_TOKEN".into(), "test-token".into()),
            ("TELEGRAM_WEBHOOK_SECRET".into(), "telegram-secret".into()),
            ("TELEGRAM_GROUP_ID".into(), "-1001".into()),
            ("TELEGRAM_ALLOWED_USER_IDS".into(), "42".into()),
            ("TELEGRAM_BOT_USERNAME".into(), "agora_bot".into()),
            ("WHATSAPP_VERIFY_TOKEN".into(), "verify-me".into()),
            ("WHATSAPP_APP_SECRET".into(), "sign-me".into()),
            ("WHATSAPP_ACCESS_TOKEN".into(), "wa-token".into()),
            ("WHATSAPP_PHONE_NUMBER_ID".into(), "phone".into()),
            ("WHATSAPP_WABA_ID".into(), "waba".into()),
            ("WHATSAPP_GROUP_ID".into(), "group".into()),
            ("WHATSAPP_ALLOWED_USER_IDS".into(), "sender".into()),
        ]))
        .unwrap();
        let db = PgPoolOptions::new()
            .connect_lazy(config.database_url.expose())
            .unwrap();
        build_router(AppState {
            config: Arc::new(config),
            db,
        })
    }

    fn signature(body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(b"sign-me").unwrap();
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    #[tokio::test]
    async fn health_is_public_and_stable() {
        let response = app("telegram")
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body, r#"{"status":"ok"}"#);
    }

    #[tokio::test]
    async fn verifies_matching_whatsapp_challenge() {
        let response = app("telegram")
            .oneshot(
                Request::get(
                    "/webhooks/whatsapp?hub.mode=subscribe&hub.verify_token=verify-me&hub.challenge=1234",
                )
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn rejects_invalid_provider_secrets_before_database_access() {
        let telegram = app("telegram")
            .oneshot(
                Request::post("/webhooks/telegram")
                    .header("x-telegram-bot-api-secret-token", "wrong")
                    .body(Body::from("not-json"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(telegram.status(), StatusCode::UNAUTHORIZED);

        let whatsapp = app("whatsapp")
            .oneshot(
                Request::post("/webhooks/whatsapp")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(whatsapp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn authenticates_and_ignores_the_inactive_provider_before_parsing() {
        let body = b"not-json";
        let whatsapp = app("telegram")
            .oneshot(
                Request::post("/webhooks/whatsapp")
                    .header("x-hub-signature-256", signature(body))
                    .body(Body::from(body.as_slice()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(whatsapp.status(), StatusCode::OK);

        let telegram = app("whatsapp")
            .oneshot(
                Request::post("/webhooks/telegram")
                    .header("x-telegram-bot-api-secret-token", "telegram-secret")
                    .body(Body::from("not-json"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(telegram.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn validates_active_payloads_and_body_limit() {
        let malformed = app("telegram")
            .oneshot(
                Request::post("/webhooks/telegram")
                    .header("x-telegram-bot-api-secret-token", "telegram-secret")
                    .body(Body::from("not-json"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);

        let missing_id = app("telegram")
            .oneshot(
                Request::post("/webhooks/telegram")
                    .header("x-telegram-bot-api-secret-token", "telegram-secret")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_id.status(), StatusCode::BAD_REQUEST);

        let oversized = app("telegram")
            .oneshot(
                Request::post("/webhooks/telegram")
                    .body(Body::from(vec![b'x'; 1_048_577]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn readiness_and_persistence_fail_closed() {
        let readiness = app("telegram")
            .oneshot(Request::get("/ready").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(readiness.status(), StatusCode::SERVICE_UNAVAILABLE);

        let response = app("telegram")
            .oneshot(
                Request::post("/webhooks/telegram")
                    .header("x-telegram-bot-api-secret-token", "telegram-secret")
                    .body(Body::from(r#"{"update_id":1}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body, r#"{"accepted":false}"#);
    }
}
