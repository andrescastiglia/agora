use std::{
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use agora::{AppState, build_router, config::Config, worker};
use anyhow::Context;
use sqlx::postgres::PgPoolOptions;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Arc::new(Config::from_env()?);
    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(config.database_url.expose())
        .await
        .context("failed to connect to PostgreSQL")?;
    sqlx::migrate!()
        .run(&db)
        .await
        .context("failed to run database migrations")?;

    let worker_db = db.clone();
    let worker_config = config.clone();
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let worker_shutdown = shutdown_rx.clone();
    let worker_ready = Arc::new(AtomicBool::new(true));
    let worker = tokio::spawn(async move {
        worker::run(worker_db, worker_config, worker_shutdown).await;
    });

    let app = build_router(AppState {
        config: config.clone(),
        db,
        worker_ready: worker_ready.clone(),
    });
    let address = config.bind_addr;
    info!(%address, "Agora API listening");
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .context("failed to bind HTTP listener")?;
    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = shutdown_rx.changed().await;
    });
    let supervisor = agora::runtime::supervise(
        worker,
        shutdown_tx,
        worker_ready,
        shutdown_signal(),
        Duration::from_secs(300),
    );
    let (http_result, worker_result) = tokio::join!(async { server.await }, supervisor);
    http_result.context("HTTP server failed")?;
    worker_result?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
