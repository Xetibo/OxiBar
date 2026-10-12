use oxibar_plugin_api::toml::Table;

const DEFAULT_MAX_ENTRIES: usize = 7;
const MAX_ENTRIES: usize = 50;
const DEFAULT_REFRESH_SECONDS: u64 = 300;

#[derive(Debug, Clone)]
pub(crate) struct Config {
    pub(crate) max_entries: usize,
    pub(crate) terminal: Vec<String>,
    pub(crate) refresh_seconds: u64,
    pub(crate) prefix: String,
    pub(crate) locale: String,
    pub(crate) icon_theme: String,
    pub(crate) favorites: Vec<String>,
}

pub(crate) fn read_config(global: &Table) -> Config {
    let table = global
        .get("applications")
        .and_then(|value| value.as_table());
    let positive = |key| {
        table
            .and_then(|table| table.get(key))
            .and_then(|value| value.as_integer())
            .and_then(|value| u64::try_from(value).ok())
            .filter(|value| *value > 0)
    };
    let terminal = table
        .and_then(|table| table.get("terminal"))
        .and_then(|value| value.as_str())
        .and_then(|value| shell_words::split(value).ok())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| vec!["kitty".to_owned()]);
    let text = |key: &str, fallback: &str| {
        table
            .and_then(|table| table.get(key))
            .and_then(|value| value.as_str())
            .unwrap_or(fallback)
            .to_owned()
    };
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|key| std::env::var(key).ok().filter(|value| !value.is_empty()))
        .unwrap_or_default();
    let requested_locale = text("locale", "");
    Config {
        max_entries: positive("max_entries")
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(DEFAULT_MAX_ENTRIES)
            .min(MAX_ENTRIES),
        terminal,
        refresh_seconds: positive("refresh_seconds").unwrap_or(DEFAULT_REFRESH_SECONDS),
        prefix: text("prefix", "a "),
        locale: if requested_locale.is_empty() {
            locale
        } else {
            requested_locale
        },
        icon_theme: text("icon_theme", "auto"),
        favorites: table
            .and_then(|table| table.get("favorites"))
            .and_then(|value| value.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
    }
}
