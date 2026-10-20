//! One cancellable owner for latest-only provider work, independent of a host reactor.

use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};

use iced::futures::{FutureExt, StreamExt, channel::oneshot, future::BoxFuture};

/// A subscription-owned timer: dropping its stream cancels the sleep and keeps
/// repeated subscriptions from accumulating detached poll threads.
pub fn interval(duration: std::time::Duration) -> iced::futures::stream::BoxStream<'static, ()> {
    iced::futures::stream::unfold((), move |()| async move {
        async_io::Timer::after(duration).await;
        Some(((), ()))
    })
    .boxed()
}

/// Run bounded blocking work without relying on a reactor shared across dylibs.
pub async fn run<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (send, receive) = oneshot::channel();
    thread::Builder::new()
        .name("oxibar-provider-job".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                .unwrap_or_else(|_| Err("Provider worker panicked".into()));
            let _ = send.send(result);
        })
        .map_err(|error| error.to_string())?;
    receive
        .await
        .map_err(|_| "Provider worker stopped".to_owned())?
}

#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

struct Job<T> {
    cancel: CancelToken,
    run: Box<dyn FnOnce(CancelToken) -> T + Send>,
    reply: oneshot::Sender<Result<T, String>>,
}

struct State<T> {
    pending: Option<Job<T>>,
    running: Option<CancelToken>,
    stopped: bool,
}

struct Shared<T> {
    state: Mutex<State<T>>,
    wake: Condvar,
}

/// Jobs must check their token during scans and bound any blocking I/O.
/// Replacing work cancels its predecessor; dropping the owner cancels and joins it.
pub struct LatestWorker<T: Send + 'static> {
    shared: Arc<Shared<T>>,
    thread: Option<JoinHandle<()>>,
}

impl<T: Send + 'static> std::fmt::Debug for LatestWorker<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LatestWorker").finish_non_exhaustive()
    }
}

impl<T: Send + 'static> LatestWorker<T> {
    pub fn new(name: &str) -> Result<Self, String> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                pending: None::<Job<T>>,
                running: None,
                stopped: false,
            }),
            wake: Condvar::new(),
        });
        let owner = shared.clone();
        let thread = thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                loop {
                    let job = {
                        let mut state = owner
                            .state
                            .lock()
                            .unwrap_or_else(|error| error.into_inner());
                        while state.pending.is_none() && !state.stopped {
                            state = owner
                                .wake
                                .wait(state)
                                .unwrap_or_else(|error| error.into_inner());
                        }
                        if state.stopped {
                            break;
                        }
                        let job = state.pending.take().expect("pending worker job");
                        state.running = Some(job.cancel.clone());
                        job
                    };
                    let token = job.cancel.clone();
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        (job.run)(token.clone())
                    }))
                    .map_err(|_| "provider worker job panicked".to_owned());
                    let result = if token.is_cancelled() {
                        Err("provider work superseded".to_owned())
                    } else {
                        result
                    };
                    let _ = job.reply.send(result);
                    owner
                        .state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .running = None;
                }
            })
            .map_err(|error| format!("cannot start provider worker: {error}"))?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    pub fn cancel(&self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(token) = &state.running {
            token.cancel();
        }
        if let Some(job) = state.pending.take() {
            job.cancel.cancel();
            let _ = job.reply.send(Err("provider work superseded".to_owned()));
        }
    }

    pub fn submit(
        &self,
        job: impl FnOnce(CancelToken) -> T + Send + 'static,
    ) -> BoxFuture<'static, Result<T, String>> {
        let (reply, result) = oneshot::channel();
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.stopped {
            let _ = reply.send(Err("provider worker stopped".to_owned()));
        } else {
            if let Some(token) = &state.running {
                token.cancel();
            }
            if let Some(previous) = state.pending.take() {
                previous.cancel.cancel();
                let _ = previous
                    .reply
                    .send(Err("provider work superseded".to_owned()));
            }
            state.pending = Some(Job {
                cancel: CancelToken::default(),
                run: Box::new(job),
                reply,
            });
            self.shared.wake.notify_one();
        }
        async move {
            result
                .await
                .unwrap_or_else(|_| Err("provider worker stopped without a result".to_owned()))
        }
        .boxed()
    }
}

impl<T: Send + 'static> Drop for LatestWorker<T> {
    fn drop(&mut self) {
        self.cancel();
        self.shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .stopped = true;
        self.shared.wake.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn interval_ownership_can_be_dropped_and_resumed_without_a_host_reactor() {
        for _ in 0..25 {
            let mut ticks = interval(Duration::from_millis(1));
            assert_eq!(iced::futures::executor::block_on(ticks.next()), Some(()));
            assert!(ticks.next().now_or_never().is_none());
            let started = Instant::now();
            drop(ticks);
            assert!(started.elapsed() < Duration::from_millis(100));
        }
    }

    #[test]
    fn coalesces_pending_work_and_cancels_running_work_without_a_reactor() {
        let worker = LatestWorker::new("provider-queue-test").unwrap();
        let (started, started_rx) = std::sync::mpsc::channel();
        let old = worker.submit(move |cancel| {
            started.send(()).unwrap();
            while !cancel.is_cancelled() {
                thread::sleep(Duration::from_millis(1));
            }
            1
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let newest = worker.submit(|_| 3);
        assert!(iced::futures::executor::block_on(old).is_err());
        assert_eq!(iced::futures::executor::block_on(newest).unwrap(), 3);
        let (started, started_rx) = std::sync::mpsc::channel();
        let (release, release_rx) = std::sync::mpsc::channel();
        let running = worker.submit(move |_| {
            started.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(1)).unwrap();
            5
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let superseded = worker.submit(|_| 6);
        let newest = worker.submit(|_| 7);
        assert!(iced::futures::executor::block_on(superseded).is_err());
        release.send(()).unwrap();
        assert!(iced::futures::executor::block_on(running).is_err());
        assert_eq!(iced::futures::executor::block_on(newest).unwrap(), 7);
        let (started, started_rx) = std::sync::mpsc::channel();
        let pending = worker.submit(move |cancel| {
            started.send(()).unwrap();
            while !cancel.is_cancelled() {
                thread::sleep(Duration::from_millis(1));
            }
            4
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let now = Instant::now();
        drop(worker);
        assert!(now.elapsed() < Duration::from_secs(1));
        assert!(iced::futures::executor::block_on(pending).is_err());
    }
}
