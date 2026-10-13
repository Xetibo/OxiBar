use std::{collections::BTreeMap, os::unix::ffi::OsStrExt, path::PathBuf};

use oxibar_plugin_api::worker::CancelToken;

use crate::{calculator, clipboard, config::Config, index::Index};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    Execute {
        argv: Vec<String>,
        terminal: bool,
        directory: Option<PathBuf>,
    },
    Open(String),
    Copy {
        text: String,
        identity: String,
    },
}

#[derive(Debug, Clone)]
pub struct Found {
    pub title: String,
    pub description: String,
    pub key: Option<String>,
    pub intent: Intent,
    pub score: i64,
}

fn match_score(query: &str, text: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let text = text.to_lowercase();
    if text == query {
        return Some(1000);
    }
    if text.starts_with(query) {
        return Some(500 - (text.len() as i64).min(100));
    }
    if let Some(position) = text.find(query) {
        return Some(300 - (position as i64).min(100));
    }
    let mut chars = query.chars();
    let mut needed = chars.next()?;
    let mut gaps = 0;
    for character in text.chars() {
        if character == needed {
            if let Some(next) = chars.next() {
                needed = next;
            } else {
                return Some(100 - gaps.min(100));
            }
        } else {
            gaps += 1;
        }
    }
    None
}

pub fn search(
    config: &Config,
    index: &Index,
    clipboard: &clipboard::Snapshot,
    input: &str,
    ranking: &BTreeMap<String, i64>,
    cancel: &CancelToken,
) -> Result<Vec<Found>, String> {
    let input = input.trim_start();
    let prefix = input
        .chars()
        .next()
        .filter(|character| "=/@!:".contains(*character));
    let query = if prefix.is_some() { &input[1..] } else { input }.trim();
    let normalized = query.to_lowercase();
    let mut found = Vec::new();
    let mut add = |mut item: Found| {
        item.score += item
            .key
            .as_ref()
            .and_then(|key| ranking.get(key))
            .copied()
            .unwrap_or_default();
        let position = found
            .binary_search_by(|existing: &Found| {
                existing
                    .score
                    .cmp(&item.score)
                    .reverse()
                    .then_with(|| existing.title.cmp(&item.title))
            })
            .unwrap_or_else(|position| position);
        if position < config.limit {
            found.insert(position, item);
            found.truncate(config.limit);
        }
    };
    if prefix == Some('=') {
        if !query.is_empty() {
            let result = calculator::calculate(query)?;
            add(Found {
                title: format!("{query} = {result}"),
                description: if clipboard.available {
                    "Copy result to clipboard"
                } else {
                    "Clipboard unavailable"
                }
                .into(),
                key: None,
                intent: Intent::Copy {
                    identity: format!("calculator:{result}"),
                    text: result,
                },
                score: 2000,
            });
        }
        return Ok(found);
    }
    if prefix == Some(':') {
        if !config.clipboard {
            return Err(
                "Clipboard history is disabled; enable tools.clipboard_enabled in settings".into(),
            );
        }
        if !clipboard.available {
            return Err(clipboard
                .error
                .clone()
                .unwrap_or_else(|| "Clipboard unavailable".into()));
        }
        for (position, item) in clipboard.entries.iter().enumerate() {
            if cancel.is_cancelled() {
                return Ok(Vec::new());
            }
            if let Some(score) = match_score(&normalized, &item.text) {
                add(Found {
                    title: item
                        .text
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .chars()
                        .take(72)
                        .collect(),
                    description: format!(
                        "Clipboard history · {} characters",
                        item.text.chars().count()
                    ),
                    key: None,
                    intent: Intent::Copy {
                        identity: format!("clipboard:{}", item.id),
                        text: item.text.clone(),
                    },
                    score: score + 500 - position as i64,
                });
            }
        }
        return Ok(found);
    }
    if prefix.is_none() || prefix == Some('/') {
        for path in &index.files {
            if cancel.is_cancelled() {
                return Ok(Vec::new());
            }
            let Some(description) = path.to_str() else {
                continue;
            };
            if let Some(score) = match_score(&normalized, description) {
                let key = format!(
                    "file:{}",
                    path.as_os_str()
                        .as_bytes()
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                );
                if prefix.is_none() && query.len() < 2 && !config.favorites.contains(&key) {
                    continue;
                }
                add(Found {
                    title: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    description: description.to_owned(),
                    key: Some(key),
                    intent: Intent::Open(description.to_owned()),
                    score: score - 50,
                });
            }
        }
    }
    if prefix.is_none() || prefix == Some('@') {
        for bookmark in &config.bookmarks {
            if cancel.is_cancelled() {
                return Ok(Vec::new());
            }
            if let Some(score) =
                match_score(&normalized, &format!("{} {}", bookmark.name, bookmark.url))
            {
                add(Found {
                    title: bookmark.name.clone(),
                    description: bookmark.url.clone(),
                    key: Some(format!("bookmark:{}", bookmark.id)),
                    intent: Intent::Open(bookmark.url.clone()),
                    score,
                });
            }
        }
    }
    if prefix.is_none() || prefix == Some('!') {
        for action in &config.actions {
            if cancel.is_cancelled() {
                return Ok(Vec::new());
            }
            if let Some(score) = match_score(&normalized, &action.name) {
                add(Found {
                    title: action.name.clone(),
                    description: format!("Action · {}", shell_words::join(&action.argv)),
                    key: Some(format!("action:{}", action.id)),
                    intent: Intent::Execute {
                        argv: action.argv.clone(),
                        terminal: action.terminal,
                        directory: action.directory.clone(),
                    },
                    score,
                });
            }
        }
    }
    Ok(found)
}
