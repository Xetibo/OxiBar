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
    let executable = arguments
        .first()
        .filter(|value| !value.is_empty())
        .ok_or("Locker command is empty")?;
    let child = std::process::Command::new(executable)
        .args(&arguments[1..])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let child = std::sync::Arc::new(std::sync::Mutex::new(Some(child)));
    let reaper = child.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("oxibar-locker-reaper".to_owned())
        .spawn(move || {
            if let Some(mut child) = reaper
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
            {
                let _ = child.wait();
            }
        })
    {
        if let Some(mut child) = child
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
        return Err(error.to_string());
    }
    Ok(())
}
