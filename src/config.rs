use std::{fs, io::Read, path::PathBuf};

use iced_layershell::reexport::Anchor;
use toml::Table;

pub fn get_allowed_plugins(config: &Table) -> Vec<&str> {
    match config.get("plugins") {
        Some(toml::Value::Array(values)) => values
            .iter()
            .filter_map(|value| match value {
                toml::Value::String(name) => Some(name.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    }
}

fn read_config(oxirun_config: &PathBuf) -> Table {
    let mut read_config = String::new();
    let mut file = fs::File::open(oxirun_config).expect("Could not open config file");
    let _ = file
        .read_to_string(&mut read_config)
        .expect("Could not read config file");

    toml::from_str(&read_config).expect("Could not deserialize config")
}

pub fn get_oxirun_dir() -> PathBuf {
    let base_dirs = xdg::BaseDirectories::new();
    let config_home = base_dirs.get_config_home();
    let oxirun_dir = config_home
        .expect("Could not get config home")
        .join("oxibar");
    if !oxirun_dir.is_dir() {
        std::fs::create_dir(&oxirun_dir).expect("Could not create config dir");
    }
    oxirun_dir
}

pub fn get_config() -> Table {
    let oxirun_dir = get_oxirun_dir();
    let oxirun_config = oxirun_dir.join("config.toml");
    if !oxirun_config.is_file() {
        Table::new()
    } else {
        read_config(&oxirun_config)
    }
}

fn anchor_from_string(anchor_str: &str) -> Anchor {
    match anchor_str.to_lowercase().as_str() {
        "top" => Anchor::Top,
        "bottom" => Anchor::Bottom,
        "right" => Anchor::Right,
        "left" => Anchor::Left,
        _ => Anchor::Top,
    }
}

#[derive(Debug, Clone)]
pub struct StartupRetryPolicy {
    pub enabled: bool,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub max_attempts: Option<u32>,
    pub catch_panics: bool,
}

impl Default for StartupRetryPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            initial_delay_ms: 500,
            max_delay_ms: 8000,
            max_attempts: None,
            catch_panics: true,
        }
    }
}

impl StartupRetryPolicy {
    /// Whether a further attempt is allowed after `attempts` have already
    /// been consumed. Returns `true` when `max_attempts` is unset (unlimited).
    pub fn can_attempt_more(&self, attempts: u32) -> bool {
        self.max_attempts
            .is_none_or(|max| attempts + 1 < max)
    }
}

fn non_negative_u64(value: i64) -> Option<u64> {
    u64::try_from(value).ok()
}

pub fn startup_retry_policy(config: &Table) -> StartupRetryPolicy {
    let defaults = StartupRetryPolicy::default();
    let Some(table) = config.get("startup").and_then(|v| v.as_table()) else {
        return defaults;
    };
    StartupRetryPolicy {
        enabled: table
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.enabled),
        initial_delay_ms: table
            .get("initial_delay_ms")
            .and_then(|v| v.as_integer())
            .and_then(non_negative_u64)
            .unwrap_or(defaults.initial_delay_ms),
        max_delay_ms: table
            .get("max_delay_ms")
            .and_then(|v| v.as_integer())
            .and_then(non_negative_u64)
            .unwrap_or(defaults.max_delay_ms),
        max_attempts: table
            .get("max_attempts")
            .and_then(|v| v.as_integer())
            .and_then(|v| u32::try_from(v).ok()),
        catch_panics: table
            .get("catch_panics")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.catch_panics),
    }
}

pub fn anchor_from_strings(anchor_strs: Vec<&str>) -> Anchor {
    let mut anchor = Anchor::empty();
    for anchor_str in anchor_strs {
        anchor |= anchor_from_string(anchor_str);
    }
    anchor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_plugins_reads_only_string_entries() {
        let mut config = Table::new();
        config.insert(
            "plugins".to_owned(),
            toml::Value::Array(vec![
                toml::Value::String("libclock.so".to_owned()),
                toml::Value::Integer(7),
                toml::Value::String("libnetwork.so".to_owned()),
            ]),
        );

        assert_eq!(
            get_allowed_plugins(&config),
            vec!["libclock.so", "libnetwork.so"]
        );
    }

    #[test]
    fn allowed_plugins_defaults_empty() {
        assert!(get_allowed_plugins(&Table::new()).is_empty());
    }

    #[test]
    fn anchors_combine_known_values_and_default_unknown_to_top() {
        assert_eq!(
            anchor_from_strings(vec!["left", "right"]),
            Anchor::Left | Anchor::Right
        );
        assert_eq!(anchor_from_strings(vec!["wat"]), Anchor::Top);
    }

    #[test]
    fn startup_retry_policy_defaults_when_table_absent() {
        let policy = startup_retry_policy(&Table::new());
        assert!(policy.enabled);
        assert_eq!(policy.initial_delay_ms, 500);
        assert_eq!(policy.max_delay_ms, 8000);
        assert_eq!(policy.max_attempts, None);
        assert!(policy.catch_panics);
        assert!(policy.can_attempt_more(0));
    }

    #[test]
    fn startup_retry_policy_reads_configured_values() {
        let mut table = Table::new();
        table.insert(
            "startup".to_owned(),
            toml::Value::Table(
                [
                    ("enabled", toml::Value::Boolean(false)),
                    ("initial_delay_ms", toml::Value::Integer(250)),
                    ("max_delay_ms", toml::Value::Integer(4000)),
                    ("max_attempts", toml::Value::Integer(3)),
                    ("catch_panics", toml::Value::Boolean(false)),
                ]
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect(),
            ),
        );

        let policy = startup_retry_policy(&table);
        assert!(!policy.enabled);
        assert_eq!(policy.initial_delay_ms, 250);
        assert_eq!(policy.max_delay_ms, 4000);
        assert_eq!(policy.max_attempts, Some(3));
        assert!(!policy.catch_panics);
    }

    #[test]
    fn startup_retry_policy_respects_max_attempts_budget() {
        let mut table = Table::new();
        table.insert(
            "startup".to_owned(),
            toml::Value::Table(
                [("max_attempts", toml::Value::Integer(3))]
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), v))
                    .collect(),
            ),
        );

        let policy = startup_retry_policy(&table);
        assert!(policy.can_attempt_more(0));
        assert!(policy.can_attempt_more(1));
        assert!(!policy.can_attempt_more(2));
    }
}
