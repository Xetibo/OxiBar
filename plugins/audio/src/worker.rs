use std::{future::Future, thread};

use iced::futures::{
    channel::oneshot,
    future::{Either, select},
};

pub(crate) async fn run<T: Send + 'static>(
    job: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let (sender, receiver) = oneshot::channel();
    thread::Builder::new()
        .name("oxibar-audio-compat".to_owned())
        .spawn(move || {
            let _ = sender.send(job());
        })
        .map_err(|e| e.to_string())?;
    receiver
        .await
        .map_err(|_| "audio: compatibility worker stopped".to_owned())
}

// async-io works across the Rust dylib boundary without the host's Tokio TLS.
pub(crate) async fn bounded<T>(job: impl Future<Output = Result<T, String>>) -> Result<T, String> {
    deadline(job, std::time::Duration::from_secs(3), "media request").await
}

pub(crate) async fn deadline<T>(
    job: impl Future<Output = Result<T, String>>,
    duration: std::time::Duration,
    label: &str,
) -> Result<T, String> {
    let timeout = async_io::Timer::after(duration);
    match select(Box::pin(job), Box::pin(timeout)).await {
        Either::Left((result, _)) => result,
        Either::Right(_) => Err(format!("audio: {label} timed out")),
    }
}
