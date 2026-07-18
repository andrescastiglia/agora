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
    repository::{persist_webhook_event, ping},
    security::{sha256_hex, verify_meta_signature},
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
        && query.verify_token == state.config.whatsapp_verify_token.expose()
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
    if let Err(error) =
        verify_meta_signature(signature, &body, state.config.whatsapp_app_secret.expose())
    {
        tracing::warn!(%error, "rejected WhatsApp webhook signature");
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"accepted": false, "error": "invalid signature"})),
        )
            .into_response();
    }

    let payload: Value = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(%error, "rejected malformed WhatsApp webhook");
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"accepted": false, "error": "invalid JSON"})),
            )
                .into_response();
        }
    };
    let content_sha256 = sha256_hex(&body);

    match persist_webhook_event(&state.db, &payload, &content_sha256).await {
        Ok(inserted) => (
            StatusCode::OK,
            Json(json!({"accepted": true, "duplicate": !inserted})),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(%error, "failed to persist WhatsApp webhook");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"accepted": false})),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use hmac::{Hmac, Mac};
    use http_body_util::BodyExt;
    use sha2::Sha256;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use crate::{AppState, build_router, config::Config};

    fn app() -> axum::Router {
        let config = Config::from_map(HashMap::from([
            ("DATABASE_URL".into(), "postgres://localhost/agora".into()),
            ("WHATSAPP_VERIFY_TOKEN".into(), "verify-me".into()),
            ("WHATSAPP_APP_SECRET".into(), "sign-me".into()),
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
        let response = app()
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body, r#"{"status":"ok"}"#);
    }

    #[tokio::test]
    async fn verifies_matching_challenge() {
        let response = app()
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
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body, "1234");
    }

    #[tokio::test]
    async fn rejects_wrong_verification_token() {
        let response = app()
            .oneshot(
                Request::get(
                    "/webhooks/whatsapp?hub.mode=subscribe&hub.verify_token=wrong&hub.challenge=1234",
                )
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn rejects_unsigned_webhook_before_database_access() {
        let response = app()
            .oneshot(
                Request::post("/webhooks/whatsapp")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn enforces_webhook_body_limit() {
        let response = app()
            .oneshot(
                Request::post("/webhooks/whatsapp")
                    .body(Body::from(vec![b'x'; 1_048_577]))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn readiness_reports_an_unavailable_database() {
        let response = app()
            .oneshot(Request::get("/ready").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn rejects_signed_invalid_json() {
        let body = b"not-json";
        let response = app()
            .oneshot(
                Request::post("/webhooks/whatsapp")
                    .header("x-hub-signature-256", signature(body))
                    .body(Body::from(body.as_slice()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn reports_persistence_failure_without_exposing_details() {
        let body = br#"{"object":"whatsapp_business_account","entry":[]}"#;
        let response = app()
            .oneshot(
                Request::post("/webhooks/whatsapp")
                    .header("x-hub-signature-256", signature(body))
                    .body(Body::from(body.as_slice()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body, r#"{"accepted":false}"#);
    }
}
