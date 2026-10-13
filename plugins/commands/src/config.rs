use oxibar_plugin_api::toml::{Table, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub limit: usize,
    pub terminal: Vec<String>,
    pub command_terminal: bool,
    pub search_unprefixed: bool,
    pub favorites: Vec<String>,
    pub refresh_seconds: u64,
}

pub fn read(table: &Table) -> Result<Config, String> {
    let empty = Table::new();
    let values = table
        .get("commands")
        .and_then(Value::as_table)
        .unwrap_or(&empty);
    let number = |key: &str, default: i64, min: i64, max: i64| -> Result<i64, String> {
        match values.get(key) {
            Some(value) => value
                .as_integer()
                .filter(|value| (min..=max).contains(value))
                .ok_or_else(|| format!("commands.{key}: expected integer in {min}..{max}")),
            None => Ok(default),
        }
    };
    let boolean = |key: &str, default| -> Result<bool, String> {
        match values.get(key) {
            Some(value) => value
                .as_bool()
                .ok_or_else(|| format!("commands.{key}: expected boolean")),
            None => Ok(default),
        }
    };
    let terminal = match values.get("terminal") {
        Some(value) => value
            .as_str()
            .ok_or("commands.terminal: expected argument string")?,
        None => "kitty",
    };
    let terminal = shell_words::split(terminal).map_err(|error| error.to_string())?;
    if terminal.is_empty()
        || terminal.len() > 32
        || terminal
            .iter()
            .any(|word| word.is_empty() || word.len() > 4096 || word.contains('\0'))
    {
        return Err("commands.terminal: invalid argument list".into());
    }
    let favorites = match values.get("favorites") {
        Some(value) => value
            .as_array()
            .filter(|items| items.len() <= 4096)
            .ok_or("commands.favorites: expected bounded text array")?
            .iter()
            .map(|item| {
                item.as_str()
                    .filter(|text| !text.is_empty() && text.len() <= 4096 && !text.contains('\0'))
                    .map(str::to_owned)
                    .ok_or_else(|| "commands.favorites: invalid identity".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };
    Ok(Config {
        limit: number("max_entries", 7, 1, 50)? as usize,
        terminal,
        command_terminal: boolean("command_terminal", true)?,
        search_unprefixed: boolean("search_unprefixed", false)?,
        favorites,
        refresh_seconds: number("refresh_seconds", 300, 1, 3600)? as u64,
    })
}
