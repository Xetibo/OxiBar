use iced::futures::{
    Future, FutureExt,
    future::{Either, select},
};
use std::time::Duration;

pub async fn bounded<T>(
    job: impl Future<Output = Result<T, String>>,
    duration: Duration,
) -> Result<T, String> {
    match select(
        job.boxed_local(),
        async_io::Timer::after(duration).boxed_local(),
    )
    .await
    {
        Either::Left((result, _)) => result,
        Either::Right(_) => Err("System operation timed out".to_owned()),
    }
}

pub async fn run<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    std::thread::Builder::new()
        .name("oxibar-system-command".to_owned())
        .spawn(move || {
            let _ = sender.send(job());
        })
        .map_err(|error| error.to_string())?;
    receiver
        .await
        .map_err(|_| "System command worker stopped".to_owned())?
}

pub fn launch(arguments: Vec<String>) -> Result<(), String> {
    oxibar_plugin_api::launch::launch(arguments, None).map(|_| ())
}
