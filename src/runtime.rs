//! Coordinate HTTP shutdown and the single durable queue consumer.
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, bail};
use tokio::{sync::watch, task::JoinHandle};

pub async fn supervise(
    mut worker: JoinHandle<()>,
    shutdown: watch::Sender<bool>,
    ready: Arc<AtomicBool>,
    stop: impl Future<Output = ()>,
    grace: Duration,
) -> anyhow::Result<()> {
    let completed = tokio::select! {
        result = &mut worker => Some(result),
        () = stop => None,
    };
    ready.store(false, Ordering::Release);
    let _ = shutdown.send(true);
    if let Some(result) = completed {
        result.context("worker terminated unexpectedly")?;
        bail!("worker exited before shutdown");
    }
    match tokio::time::timeout(grace, &mut worker).await {
        Ok(result) => result.context("worker failed while draining")?,
        Err(_) => {
            worker.abort();
            let _ = worker.await;
            // Claims remain durable. Never reset an ambiguous outgoing attempt.
            tracing::warn!("worker drain timed out; durable claims require recovery");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shutdown_waits_for_active_work() {
        let (tx, mut rx) = watch::channel(false);
        let ready = Arc::new(AtomicBool::new(true));
        let finished = Arc::new(AtomicBool::new(false));
        let done = finished.clone();
        let task = tokio::spawn(async move {
            rx.changed().await.unwrap();
            tokio::time::sleep(Duration::from_millis(5)).await;
            done.store(true, Ordering::Release);
        });
        supervise(task, tx, ready.clone(), async {}, Duration::from_secs(1))
            .await
            .unwrap();
        assert!(finished.load(Ordering::Acquire));
        assert!(!ready.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn unexpected_exit_closes_readiness_and_stops_http() {
        for panic in [false, true] {
            let (tx, rx) = watch::channel(false);
            let ready = Arc::new(AtomicBool::new(true));
            let task = tokio::spawn(async move {
                assert!(!panic, "simulated worker panic");
            });
            assert!(
                supervise(
                    task,
                    tx,
                    ready.clone(),
                    std::future::pending(),
                    Duration::from_secs(1)
                )
                .await
                .is_err()
            );
            assert!(*rx.borrow());
            assert!(!ready.load(Ordering::Acquire));
        }
    }

    #[tokio::test]
    async fn drain_timeout_cancels_task() {
        let (tx, _rx) = watch::channel(false);
        let ready = Arc::new(AtomicBool::new(true));
        let task = tokio::spawn(std::future::pending::<()>());
        let abort = task.abort_handle();
        supervise(task, tx, ready, async {}, Duration::from_millis(1))
            .await
            .unwrap();
        assert!(abort.is_finished());
    }
}
