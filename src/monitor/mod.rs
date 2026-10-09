#[cfg(test)]
use iced_layershell::reexport::OutputOption;

mod hyprland;
pub mod output;
mod wayland;
pub use output::{Output, OutputMode, OutputSnapshot};

pub fn read_inventory() -> Result<OutputSnapshot, String> {
    hyprland::inventory().or_else(|_| wayland::inventory())
}

pub(crate) fn watch_inventory(
    seconds: &u64,
) -> impl iced::futures::Stream<Item = crate::Message> + use<> {
    let interval = std::time::Duration::from_secs(*seconds);
    iced::stream::channel(1, async move |mut output| {
        use iced::futures::SinkExt;
        let mut previous = None;
        loop {
            let result = crate::background::run(read_inventory)
                .await
                .and_then(|result| result);
            if previous.as_ref() != Some(&result) {
                previous = Some(result.clone());
                if output
                    .send(crate::Message::OutputsChanged(result))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            tokio::time::sleep(interval).await;
        }
    })
}

type ActiveOutputBackend = fn() -> Option<String>;

const ACTIVE_OUTPUT_BACKENDS: &[ActiveOutputBackend] = &[hyprland::active_output_name];

pub(crate) fn active_output_name() -> Option<String> {
    ACTIVE_OUTPUT_BACKENDS
        .iter()
        .find_map(|backend| normalize_output_name(backend()))
}

fn normalize_output_name(output_name: Option<String>) -> Option<String> {
    output_name.filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toast_output_option(output_name: Option<String>) -> OutputOption {
        normalize_output_name(output_name)
            .map(OutputOption::OutputName)
            .unwrap_or(OutputOption::LastOutput)
    }

    #[test]
    fn toast_output_prefers_active_monitor_name() {
        assert_eq!(
            toast_output_option(Some("DP-1".to_owned())),
            OutputOption::OutputName("DP-1".to_owned())
        );
        assert_eq!(
            toast_output_option(Some(String::new())),
            OutputOption::LastOutput
        );
        assert_eq!(toast_output_option(None), OutputOption::LastOutput);
    }
}
