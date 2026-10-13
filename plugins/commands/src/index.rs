use oxibar_plugin_api::worker::CancelToken;
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt, path::PathBuf};

pub type Index = BTreeMap<String, PathBuf>;

pub fn roots() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).take(128).collect())
        .unwrap_or_default()
}

pub fn read(roots: &[PathBuf], cancel: &CancelToken) -> Index {
    let mut commands = Index::new();
    for directory in roots {
        if cancel.is_cancelled() || commands.len() >= 20_000 {
            break;
        }
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten().take(20_000) {
            if cancel.is_cancelled() || commands.len() >= 20_000 {
                break;
            }
            let path = entry.path();
            if path.metadata().is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            }) && path.to_str().is_some()
                && let Some(name) = entry.file_name().to_str()
            {
                commands.entry(name.to_owned()).or_insert(path);
            }
        }
    }
    commands
}
