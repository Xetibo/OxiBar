use std::thread;

use iced::futures::channel::oneshot;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_runs_without_a_tokio_reactor() {
        assert_eq!(iced::futures::executor::block_on(run(|| 42)).unwrap(), 42);
    }
}
