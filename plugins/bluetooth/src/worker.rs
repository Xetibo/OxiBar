use std::{future::Future, time::Duration};

use iced::futures::future::{Either, select};

pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
pub(crate) const RECONNECT_DELAY: Duration = Duration::from_secs(2);

pub(crate) async fn deadline<T>(
    job: impl Future<Output = Result<T, String>>,
    duration: Duration,
    label: &str,
) -> Result<T, String> {
    match select(Box::pin(job), Box::pin(async_io::Timer::after(duration))).await {
        Either::Left((result, _)) => result,
        Either::Right(_) => Err(format!("bluetooth: {label} timed out")),
    }
}
