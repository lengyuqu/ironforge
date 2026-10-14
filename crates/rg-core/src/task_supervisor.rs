//! Supervision for long-lived background tasks.
//!
//! With `panic = "unwind"` a panic in a background task no longer takes the
//! process down — but it does kill the task. A dead log-write consumer or a
//! dead watchdog leaves the server *looking* healthy while silently losing
//! data, which is worse than crashing. These helpers keep such tasks alive:
//! a panic is logged at `error!` level and the task is restarted.

use std::future::Future;
use std::time::Duration;

use futures::FutureExt;

/// Delay before restarting a task that panicked, to avoid a hot panic loop.
const RESTART_BACKOFF: Duration = Duration::from_secs(1);

/// Render a caught panic payload as text.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

/// Run `make()` forever, restarting it if it returns or panics.
///
/// `make` is called once per attempt, so each restart gets a fresh future and
/// any state captured by the closure is re-created. A panic is caught, logged
/// and retried after [`RESTART_BACKOFF`].
///
/// A task that returns normally is also restarted: these are meant to be
/// infinite loops, so returning means something went wrong.
pub async fn supervise<F, Fut>(name: &'static str, make: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    supervise_with_backoff(name, make, RESTART_BACKOFF).await
}

/// [`supervise`] with a caller-supplied restart delay.
///
/// Tests use a short delay; production uses [`RESTART_BACKOFF`].
pub async fn supervise_with_backoff<F, Fut>(name: &'static str, mut make: F, backoff: Duration)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    loop {
        let outcome = std::panic::AssertUnwindSafe(make()).catch_unwind().await;
        match outcome {
            Ok(()) => tracing::error!(
                task = name,
                "supervised background task returned; restarting"
            ),
            Err(payload) => tracing::error!(
                task = name,
                panic = %panic_message(&payload),
                "supervised background task panicked; restarting"
            ),
        }
        tokio::time::sleep(backoff).await;
    }
}

/// Run one step, catching a panic so the surrounding loop survives it.
///
/// Use this when the loop owns a resource that cannot be recreated (for
/// example an `mpsc::Receiver`): the loop keeps its state, and only the
/// failing step is restarted. Returns `true` if the step completed, `false`
/// if it panicked.
pub async fn step<Fut>(name: &'static str, future: Fut) -> bool
where
    Fut: Future<Output = ()>,
{
    match std::panic::AssertUnwindSafe(future).catch_unwind().await {
        Ok(()) => true,
        Err(payload) => {
            tracing::error!(
                task = name,
                panic = %panic_message(&payload),
                "background task step panicked; continuing"
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A task that panics must be restarted, not abandoned. Without this the
    /// process would keep running while the task silently stayed dead.
    #[tokio::test]
    async fn supervise_restarts_after_panics() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_task = attempts.clone();
        let task = tokio::spawn(supervise_with_backoff(
            "test.panicky",
            move || {
                let attempts = attempts_task.clone();
                async move {
                    let n = attempts.fetch_add(1, Ordering::SeqCst);
                    if n < 3 {
                        panic!("boom #{n}");
                    }
                    // Fourth attempt blocks, so the count stabilises at 4.
                    std::future::pending::<()>().await;
                }
            },
            Duration::from_millis(5),
        ));
        tokio::time::timeout(Duration::from_secs(5), async {
            while attempts.load(Ordering::SeqCst) < 4 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("supervisor must have retried past the panics");
        assert!(attempts.load(Ordering::SeqCst) >= 4);
        task.abort();
    }

    #[tokio::test]
    async fn step_reports_panics_without_propagating() {
        assert!(step("test.ok", async {}).await);
        let panicked = step("test.panic", async { panic!("boom") }).await;
        assert!(!panicked, "step must report the panic instead of unwinding");
    }

    #[test]
    fn panic_payload_is_rendered() {
        let payload = std::panic::catch_unwind(|| panic!("literal payload")).unwrap_err();
        assert_eq!(panic_message(&*payload), "literal payload");
    }
}