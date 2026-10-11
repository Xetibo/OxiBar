use oxibar_plugin_api::toml::{Table, Value};
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub id: String,
    pub name: String,
    pub url: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedAction {
    pub id: String,
    pub name: String,
    pub argv: Vec<String>,
    pub terminal: bool,
    pub directory: Option<PathBuf>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub limit: usize,
    pub terminal: Vec<String>,
    pub opener: Vec<String>,
    pub command_terminal: bool,
    pub roots: Vec<PathBuf>,
    pub file_limit: usize,
    pub bookmarks: Vec<Bookmark>,
    pub actions: Vec<NamedAction>,
    pub favorites: Vec<String>,
    pub clipboard: bool,
    pub clipboard_limit: usize,
    pub refresh_seconds: u64,
}

pub fn expand_path(text: &str) -> PathBuf {
    if text == "~" || text.starts_with("~/") {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        home.join(text.strip_prefix("~/").unwrap_or(""))
    } else {
        PathBuf::from(text)
    }
}
fn valid_text(text: &str, maximum: usize) -> bool {
    !text.is_empty() && text.len() <= maximum && !text.contains('\0')
}
fn valid_id(text: &str) -> bool {
    valid_text(text, 128)
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}
fn words(value: Option<&Value>, default: &str) -> Result<Vec<String>, String> {
    let text = value.and_then(Value::as_str).unwrap_or(default);
    let words = shell_words::split(text).map_err(|error| error.to_string())?;
    if words.is_empty() || words.len() > 32 || words.iter().any(|word| !valid_text(word, 4096)) {
        return Err("tools: invalid argument list".into());
    }
    Ok(words)
}
pub fn read(table: &Table) -> Result<Config, String> {
    let empty = Table::new();
    let values = table
        .get("tools")
        .and_then(Value::as_table)
        .unwrap_or(&empty);
    let list = |key: &str, defaults: &[&str]| -> Result<Vec<String>, String> {
        let result = match values.get(key) {
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| {
                    item.as_str()
                        .filter(|text| valid_text(text, 4096))
                        .map(str::to_owned)
                        .ok_or_else(|| format!("tools.{key}: expected nonempty text"))
                })
                .collect::<Result<Vec<_>, _>>()?,
            None => defaults.iter().map(|text| (*text).to_owned()).collect(),
            _ => return Err(format!("tools.{key}: expected text array")),
        };
        if result.len() > 4096 {
            return Err(format!("tools.{key}: too many items"));
        }
        Ok(result)
    };
    let records = |key: &str, maximum: usize| -> Result<Vec<&Table>, String> {
        let Some(value) = values.get(key) else {
            return Ok(Vec::new());
        };
        let items = value
            .as_array()
            .ok_or_else(|| format!("tools.{key}: expected table array"))?;
        if items.len() > maximum {
            return Err(format!("tools.{key}: too many items"));
        }
        let mut ids = BTreeSet::new();
        items
            .iter()
            .map(|value| {
                let item = value
                    .as_table()
                    .ok_or_else(|| format!("tools.{key}: expected table"))?;
                let id = item
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| valid_id(id))
                    .ok_or_else(|| format!("tools.{key}: invalid id"))?;
                if !ids.insert(id) {
                    return Err(format!("tools.{key}: duplicate id {id}"));
                }
                if !item
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| valid_text(name, 256))
                {
                    return Err(format!("tools.{key}: missing name"));
                }
                Ok(item)
            })
            .collect()
    };
    let bookmarks = records("bookmarks", 256)?
        .into_iter()
        .map(|item| {
            let url = item
                .get("url")
                .and_then(Value::as_str)
                .filter(|url| {
                    valid_text(url, 8192)
                        && ["https://", "http://", "file://", "mailto:"]
                            .iter()
                            .any(|scheme| url.starts_with(scheme))
                })
                .ok_or_else(|| "tools.bookmarks: unsupported URL".to_owned())?;
            Ok(Bookmark {
                id: item["id"].as_str().unwrap().into(),
                name: item["name"].as_str().unwrap().into(),
                url: url.into(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let actions = records("actions", 128)?
        .into_iter()
        .map(|item| {
            let argv = words(item.get("command"), "")?;
            let terminal = item
                .get("terminal")
                .map(|value| {
                    value
                        .as_bool()
                        .ok_or_else(|| "tools.actions: terminal must be boolean".to_owned())
                })
                .transpose()?
                .unwrap_or(false);
            let directory = item
                .get("directory")
                .map(|value| {
                    value
                        .as_str()
                        .filter(|text| valid_text(text, 4096))
                        .map(expand_path)
                        .ok_or_else(|| "tools.actions: invalid directory".to_owned())
                })
                .transpose()?;
            Ok(NamedAction {
                id: item["id"].as_str().unwrap().into(),
                name: item["name"].as_str().unwrap().into(),
                argv,
                terminal,
                directory,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let number = |key: &str, default: i64, min: i64, max: i64| {
        values
            .get(key)
            .and_then(Value::as_integer)
            .unwrap_or(default)
            .clamp(min, max)
    };
    Ok(Config {
        limit: number("max_entries", 7, 1, 50) as usize,
        terminal: words(values.get("terminal"), "kitty")?,
        opener: words(values.get("opener"), "xdg-open")?,
        command_terminal: values
            .get("command_terminal")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        roots: list("file_roots", &["~/Documents", "~/Downloads"])?
            .iter()
            .map(|text| expand_path(text))
            .collect(),
        file_limit: number("file_limit", 20_000, 1, 100_000) as usize,
        bookmarks,
        actions,
        favorites: list("favorites", &[])?,
        clipboard: values
            .get("clipboard_enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        clipboard_limit: number("clipboard_limit", 50, 1, 100) as usize,
        refresh_seconds: number("refresh_seconds", 300, 1, 3600) as u64,
    })
}
