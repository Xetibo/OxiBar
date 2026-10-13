use crate::config::Config;
use oxibar_plugin_api::worker::CancelToken;
use std::{collections::BTreeSet, path::PathBuf};
#[derive(Debug, Clone, Default)]
pub struct Index {
    pub files: Vec<PathBuf>,
}
pub fn roots(config: &Config) -> Vec<PathBuf> {
    config.roots.clone()
}
pub fn read(config: &Config, cancel: &CancelToken) -> Index {
    let mut result = Index::default();
    let mut visited = BTreeSet::new();
    let mut pending = config.roots.clone();
    while let Some(directory) = pending.pop() {
        if cancel.is_cancelled() || visited.len() >= 4096 || result.files.len() >= config.file_limit
        {
            break;
        }
        let Ok(canonical) = directory.canonicalize() else {
            continue;
        };
        if !visited.insert(canonical) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            if cancel.is_cancelled() || result.files.len() >= config.file_limit {
                break;
            }
            if entry.file_name().as_encoded_bytes().starts_with(b".") {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                result.files.push(entry.path());
            }
        }
    }
    result.files.sort();
    result
}
