pub mod config;
pub mod document;
pub mod http;
pub mod legal;
pub mod object_storage;
pub mod openai;
pub mod repository;
pub mod security;
pub mod text;
pub mod whatsapp;
pub mod worker;

use std::sync::Arc;

use axum::{Router, routing::get};
use sqlx::PgPool;
use tower_http::{limit::RequestBodyLimitLayer, trace::TraceLayer};

use crate::{
    config::Config,
    http::{health, ready, receive_whatsapp, verify_whatsapp},
    legal::{data_deletion, privacy, terms},
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: PgPool,
}

pub fn build_router(state: AppState) -> Router {
    let body_limit = state.config.webhook_max_body_bytes;

    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/privacy", get(privacy))
        .route("/terms", get(terms))
        .route("/data-deletion", get(data_deletion))
        .route(
            "/webhooks/whatsapp",
            get(verify_whatsapp).post(receive_whatsapp),
        )
        .layer(RequestBodyLimitLayer::new(body_limit))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
