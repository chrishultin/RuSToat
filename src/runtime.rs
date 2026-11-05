//! Bridge between GPUI's executor and a Tokio runtime.
//!
//! GPUI runs its own executor, but our networking stack (`reqwest`,
//! `tokio-tungstenite`) needs Tokio. All IO runs on one shared Tokio runtime on
//! background threads. Results come back through `futures` oneshot channels,
//! which any executor can await, including GPUI's `cx.spawn`.

use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;
use std::task::{Context, Poll};

use anyhow::anyhow;
use futures::channel::oneshot;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// The process-wide IO runtime, started on first use.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("stoat-io")
            .enable_all()
            .build()
            .expect("failed to start the Tokio IO runtime")
    })
}

/// Run `fut` on the Tokio IO runtime and return a handle that any executor can
/// await.
///
/// Dropping the handle aborts the Tokio task. If the UI stops caring about a
/// result (for example, the user switches channels), the request is cancelled
/// instead of finishing in the background.
pub fn spawn_io<F>(fut: F) -> IoTask<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let (tx, rx) = oneshot::channel();
    let handle = runtime().spawn(async move {
        // The receiver may already be gone; that's fine.
        let _ = tx.send(fut.await);
    });
    IoTask {
        rx,
        abort: handle.abort_handle(),
    }
}

/// A pending result from [`spawn_io`].
///
/// Resolves to `Err` if the task panicked or was cancelled before it finished.
#[must_use = "dropping an IoTask cancels the underlying IO"]
pub struct IoTask<T> {
    rx: oneshot::Receiver<T>,
    abort: tokio::task::AbortHandle,
}

impl<T> Future for IoTask<T> {
    type Output = anyhow::Result<T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.rx)
            .poll(cx)
            .map(|res| res.map_err(|_| anyhow!("IO task ended without producing a result")))
    }
}

impl<T> Drop for IoTask<T> {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    #[test]
    fn delivers_tokio_result_to_a_non_tokio_executor() {
        // `tokio::time::sleep` panics outside a Tokio runtime, so this only
        // passes if the future really runs on our IO runtime.
        let task = spawn_io(async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            42
        });
        assert_eq!(futures::executor::block_on(task).unwrap(), 42);
    }

    #[test]
    fn panicking_task_resolves_to_error() {
        let task = spawn_io(async {
            panic!("boom");
        });
        assert!(futures::executor::block_on(task).is_err());
    }

    #[test]
    fn dropping_the_handle_aborts_the_task() {
        let finished = Arc::new(AtomicBool::new(false));
        let flag = finished.clone();
        let task = spawn_io(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            flag.store(true, Ordering::SeqCst);
        });
        drop(task);
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            !finished.load(Ordering::SeqCst),
            "task should have been aborted"
        );
    }
}
