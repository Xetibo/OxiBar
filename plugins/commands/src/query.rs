use crate::{config::Config, index::Index};
use oxibar_plugin_api::worker::CancelToken;
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt, path::Path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub title: String,
    pub key: String,
    pub argv: Vec<String>,
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
    input: &str,
    ranking: &BTreeMap<String, i64>,
    cancel: &CancelToken,
) -> Result<Vec<Found>, String> {
    let input = input.trim_start();
    let prefixed = input.starts_with('>');
    if cancel.is_cancelled() || (!prefixed && !config.search_unprefixed) {
        return Ok(Vec::new());
    }
    let query = input.strip_prefix('>').unwrap_or(input).trim();
    let normalized = query.to_lowercase();
    let mut found = Vec::new();
    let mut add = |mut item: Found| {
        item.score = item
            .score
            .saturating_add(ranking.get(&item.key).copied().unwrap_or_default());
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
    let mut exact = None;
    if prefixed && !query.is_empty() {
        let mut argv = shell_words::split(query).map_err(|error| error.to_string())?;
        if argv.len() > 32
            || argv
                .iter()
                .any(|arg| arg.len() > 4096 || arg.contains('\0'))
        {
            return Err("Command is too large".into());
        }
        if let Some(name) = argv.first() {
            let path = Path::new(name);
            let executable = index.get(name).cloned().or_else(|| {
                (path.is_absolute()
                    && path.metadata().is_ok_and(|metadata| {
                        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
                    }))
                .then(|| path.to_path_buf())
            });
            if let Some(executable) = executable {
                exact = Some(name.clone());
                let key = format!("command:{name}");
                argv[0] = executable
                    .to_str()
                    .ok_or("Executable path is not UTF-8")?
                    .to_owned();
                add(Found {
                    title: format!("Run {query}"),
                    key,
                    argv,
                    score: 1500,
                });
            } else if argv.len() > 1 {
                return Err("Executable was not found in PATH".into());
            }
        }
    }
    for (name, executable) in index {
        if cancel.is_cancelled() {
            return Ok(Vec::new());
        }
        let key = format!("command:{name}");
        if exact.as_ref() == Some(name)
            || (!prefixed && query.len() < 2 && !config.favorites.contains(&key))
        {
            continue;
        }
        if let Some(score) = match_score(&normalized, name)
            && let Some(executable) = executable.to_str()
        {
            add(Found {
                title: name.clone(),
                key,
                argv: vec![executable.to_owned()],
                score: score - 100,
            });
        }
    }
    Ok(found)
}
