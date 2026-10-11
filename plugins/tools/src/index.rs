use crate::config::Config;
use oxibar_plugin_api::worker::CancelToken;
use std::{
    collections::{BTreeMap, BTreeSet},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
};
#[derive(Debug, Clone, Default)]
pub struct Index {
    pub commands: BTreeMap<String, PathBuf>,
    pub files: Vec<PathBuf>,
}
pub fn roots(config: &Config) -> Vec<PathBuf> {
    let mut roots = config.roots.clone();
    roots.extend(
        std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).take(128).collect::<Vec<_>>())
            .unwrap_or_default(),
    );
    roots
}
pub fn read(config: &Config, cancel: &CancelToken) -> Index {
    let mut result = Index::default();
    for directory in std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).take(128).collect::<Vec<_>>())
        .unwrap_or_default()
    {
        if cancel.is_cancelled() || result.commands.len() >= 20_000 {
            break;
        }
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten().take(20_000) {
            if cancel.is_cancelled() || result.commands.len() >= 20_000 {
                break;
            }
            let path = entry.path();
            let Ok(metadata) = path.metadata() else {
                continue;
            };
            if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
                let name = entry.file_name().to_string_lossy().into_owned();
                result.commands.entry(name).or_insert(path);
            }
        }
    }
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
