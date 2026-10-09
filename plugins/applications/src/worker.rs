use std::{sync::mpsc, thread, time::Duration};

use iced::futures::channel::{mpsc::Sender, oneshot};
use oxibar_plugin_api::PluginMsg;

struct Worker(Option<thread::JoinHandle<()>>);

impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(worker) = self.0.take() {
            let _ = worker.join();
        }
    }
}

// A dylib has its own Tokio TLS, so it cannot use the host's runtime implicitly.
pub(crate) async fn run<T: Send + 'static>(
    job: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let (sender, receiver) = oneshot::channel();
    let _worker = Worker(Some(
        thread::Builder::new()
            .name("oxirun-worker".to_owned())
            .spawn(move || {
                let _ = sender.send(job());
            })
            .map_err(|error| error.to_string())?,
    ));
    receiver
        .await
        .map_err(|_| "application worker stopped without a result".to_owned())
}

pub(crate) struct RefreshTimer {
    cancel: mpsc::Sender<()>,
    _worker: Worker,
}

impl RefreshTimer {
    pub(crate) fn new(
        interval: Duration,
        mut output: Sender<PluginMsg>,
        message: impl Fn() -> PluginMsg + Send + 'static,
    ) -> Self {
        let (cancel, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            while matches!(
                receiver.recv_timeout(interval),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                if let Err(error) = output.try_send(message())
                    && error.is_disconnected()
                {
                    break;
                }
            }
        });
        Self {
            cancel,
            _worker: Worker(Some(worker)),
        }
    }
}

impl Drop for RefreshTimer {
    fn drop(&mut self) {
        let _ = self.cancel.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_runs_without_a_tokio_reactor() {
        assert_eq!(iced::futures::executor::block_on(run(|| 42)).unwrap(), 42);
    }

    #[test]
    fn dropping_timer_cancels_long_wait_and_joins_worker() {
        let (sender, _receiver) = iced::futures::channel::mpsc::channel(1);
        let timer = RefreshTimer::new(Duration::from_secs(60), sender, || {
            std::sync::Arc::new("refresh")
        });
        drop(timer);
    }
}
