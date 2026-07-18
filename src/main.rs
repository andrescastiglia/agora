use std::{env, net::SocketAddr};

use anyhow::Context;
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::{get, post}, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{postgres::PgPoolOptions, PgPool};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::info;

#[derive(Clone)]
struct AppState {
    db: PgPool,
    verify_token: String,
}

#[derive(Debug, Deserialize)]
struct VerifyQuery {
    #[serde(rename = "hub.mode")]
    mode: String,
    #[serde(rename = "hub.verify_token")]
    verify_token: String,
    #[serde(rename = "hub.challenge")]
    challenge: String,
}

#[derive(Debug, Serialize)]
struct Health {
    status: &'static str,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let database_url = env::var("DATABASE_URL").context("DATABASE_URL is required")?;
    let verify_token = env::var("WHATSAPP_VERIFY_TOKEN").context("WHATSAPP_VERIFY_TOKEN is required")?;
    let bind = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());

    let db = PgPoolOptions::new().max_connections(10).connect(&database_url).await?;
    sqlx::migrate!().run(&db).await?;

    let state = AppState { db, verify_token };
    let app = Router::new()
        .route("/health", get(health))
        .route("/webhooks/whatsapp", get(verify_webhook).post(receive_webhook))
        .route("/api/search", post(search))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr: SocketAddr = bind.parse().context("invalid BIND_ADDR")?;
    info!(%addr, "Agora API listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<Health> {
    Json(Health { status: "ok" })
}

async fn verify_webhook(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<VerifyQuery>,
) -> impl IntoResponse {
    if query.mode == "subscribe" && query.verify_token == state.verify_token {
        (StatusCode::OK, query.challenge)
    } else {
        (StatusCode::FORBIDDEN, "invalid verification token".to_string())
    }
}

async fn receive_webhook(State(state): State<AppState>, Json(payload): Json<Value>) -> impl IntoResponse {
    let result = sqlx::query("INSERT INTO webhook_events (provider, payload) VALUES ($1, $2)")
        .bind("whatsapp")
        .bind(payload)
        .execute(&state.db)
        .await;

    match result {
        Ok(_) => (StatusCode::ACCEPTED, Json(json!({"accepted": true}))),
        Err(error) => {
            tracing::error!(%error, "failed to persist webhook");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"accepted": false})))
        }
    }
}

async fn search() -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, Json(json!({
        "error": "semantic search worker is not implemented yet"
    })))
}
