use iced::futures::channel::oneshot;

pub async fn run<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (send, receive) = oneshot::channel();
    std::thread::Builder::new()
        .name("oxibar-calendar-job".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                .unwrap_or_else(|_| Err("Calendar worker panicked".into()));
            let _ = send.send(result);
        })
        .map_err(|error| error.to_string())?;
    receive
        .await
        .map_err(|_| "Calendar worker stopped".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::futures::{Future, executor::block_on};
    #[test]
    fn blocking_calendar_job_does_not_block_ui_polling() {
        let (send, receive) = std::sync::mpsc::channel();
        let mut task = Box::pin(run(move || {
            receive
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap();
            Ok(7)
        }));
        let started = std::time::Instant::now();
        let mut context = std::task::Context::from_waker(iced::futures::task::noop_waker_ref());
        assert!(task.as_mut().poll(&mut context).is_pending());
        assert!(started.elapsed() < std::time::Duration::from_millis(50));
        send.send(()).unwrap();
        assert_eq!(block_on(task).unwrap(), 7);
    }
}
