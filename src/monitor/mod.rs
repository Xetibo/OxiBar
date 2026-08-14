use iced_layershell::reexport::OutputOption;

mod hyprland;

type ActiveOutputBackend = fn() -> Option<String>;

const ACTIVE_OUTPUT_BACKENDS: &[ActiveOutputBackend] = &[hyprland::active_output_name];

pub(crate) fn active_toast_output_option() -> OutputOption {
    active_output_name()
        .map(OutputOption::OutputName)
        .unwrap_or(OutputOption::LastOutput)
}

fn active_output_name() -> Option<String> {
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
