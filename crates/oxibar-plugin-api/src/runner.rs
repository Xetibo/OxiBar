//! Bounded usage metadata, separate from immutable base configuration and GUI overrides.

use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use toml::{Table, Value};

#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub items: BTreeMap<String, (u64, u64)>,
}

impl Usage {
    pub fn record(&mut self, key: &str) {
        let now = unix_time();
        let entry = self.items.entry(key.to_owned()).or_default();
        entry.0 = entry.0.saturating_add(1);
        entry.1 = now;
        while self.items.len() > 4096 {
            let oldest = self
                .items
                .iter()
                .min_by_key(|(_, (_, time))| *time)
                .map(|(key, _)| key.clone());
            if let Some(key) = oldest {
                self.items.remove(&key);
            }
        }
    }
    pub fn merge_local(&mut self, local: &Self) {
        for (key, (count, time)) in &local.items {
            let entry = self.items.entry(key.clone()).or_default();
            entry.0 = entry.0.saturating_add(*count);
            entry.1 = entry.1.max(*time);
        }
        while self.items.len() > 4096 {
            if let Some(oldest) = self
                .items
                .iter()
                .min_by_key(|(_, (_, time))| *time)
                .map(|(key, _)| key.clone())
            {
                self.items.remove(&oldest);
            }
        }
    }
    pub fn ranking(&self, favorites: &[String]) -> BTreeMap<String, i64> {
        let now = unix_time();
        let mut ranking = self
            .items
            .iter()
            .map(|(key, (count, time))| {
                let recency = match now.saturating_sub(*time) {
                    0..=86400 => 200,
                    86401..=604800 => 50,
                    _ => 0,
                };
                (key.clone(), ((*count).min(1000) as i64) * 10 + recency)
            })
            .collect::<BTreeMap<_, _>>();
        for key in favorites {
            *ranking.entry(key.clone()).or_default() += 1_000_000;
        }
        ranking
    }
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug)]
pub struct UsageStore {
    path: PathBuf,
    saved: Mutex<u64>,
}

impl UsageStore {
    pub fn new(provider: &str) -> Self {
        let root = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
            })
            .unwrap_or_else(std::env::temp_dir);
        assert!(
            !provider.is_empty()
                && provider
                    .chars()
                    .all(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_'))
        );
        Self::at(root.join("oxibar").join(format!("{provider}-usage.toml")))
    }
    pub fn at(path: PathBuf) -> Self {
        Self {
            path,
            saved: Mutex::new(0),
        }
    }
    pub fn load(&self) -> Result<Usage, String> {
        let mut file = match OpenOptions::new()
            .read(true)
            .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
            .open(&self.path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Usage::default());
            }
            Err(error) => return Err(format!("runner metadata: {error}")),
        };
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        if !metadata.is_file() || metadata.len() > 1_048_576 {
            return Err("runner metadata must be a bounded regular file".to_owned());
        }
        let mut text = String::new();
        Read::by_ref(&mut file)
            .take(1_048_577)
            .read_to_string(&mut text)
            .map_err(|error| error.to_string())?;
        if text.len() > 1_048_576 {
            return Err("runner metadata exceeds its limit".to_owned());
        }
        let table: Table = text
            .parse()
            .map_err(|error: toml::de::Error| error.to_string())?;
        if table.get("version").and_then(Value::as_integer) != Some(1) {
            return Err("unsupported usage metadata version".into());
        }
        let rows = table
            .get("items")
            .and_then(Value::as_table)
            .ok_or("invalid usage metadata items")?;
        if rows.len() > 4096 {
            return Err("too many usage metadata items".into());
        }
        let mut items = BTreeMap::new();
        for (key, value) in rows {
            if key.is_empty() || key.len() > 4096 || key.contains('\0') {
                return Err("invalid usage metadata key".into());
            }
            let row = value.as_table().ok_or("invalid usage metadata row")?;
            let count = row
                .get("count")
                .and_then(Value::as_integer)
                .filter(|value| *value >= 0)
                .ok_or("invalid usage count")? as u64;
            let time = row
                .get("time")
                .and_then(Value::as_integer)
                .filter(|value| *value >= 0)
                .ok_or("invalid usage timestamp")? as u64;
            items.insert(key.clone(), (count, time));
        }
        Ok(Usage { items })
    }
    pub fn save(&self, usage: &Usage, revision: u64) -> Result<(), String> {
        let mut saved = self
            .saved
            .lock()
            .map_err(|_| "runner metadata writer unavailable")?;
        if revision < *saved {
            return Ok(());
        }
        if self
            .path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err("runner metadata cannot overwrite a managed symlink".to_owned());
        }
        let parent = self.path.parent().ok_or("runner metadata has no parent")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let items = usage
            .items
            .iter()
            .take(4096)
            .map(|(key, (count, time))| {
                (
                    key.clone(),
                    Value::Table(Table::from_iter([
                        (
                            "count".to_owned(),
                            Value::Integer((*count).min(i64::MAX as u64) as i64),
                        ),
                        (
                            "time".to_owned(),
                            Value::Integer((*time).min(i64::MAX as u64) as i64),
                        ),
                    ])),
                )
            })
            .collect::<Table>();
        let text = toml::to_string_pretty(&Table::from_iter([
            ("version".to_owned(), Value::Integer(1)),
            ("items".to_owned(), Value::Table(items)),
        ]))
        .map_err(|error| error.to_string())?;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let temporary = parent.join(format!(
            ".runner-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temporary, &self.path)?;
            fs::File::open(parent)?.sync_all()
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.map_err(|error: std::io::Error| error.to_string())?;
        *saved = revision;
        Ok(())
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn atomic_state_is_sparse_private_revisioned_and_rejects_unknown_formats() {
        let directory = std::env::temp_dir().join(format!(
            "oxibar-usage-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let store = UsageStore::at(directory.join("usage.toml"));
        let mut first = Usage::default();
        first.record("editor.desktop");
        let mut second = first.clone();
        second.record("editor.desktop");
        store.save(&second, 2).unwrap();
        store.save(&first, 1).unwrap();
        assert_eq!(store.load().unwrap().items["editor.desktop"].0, 2);
        assert_eq!(
            store.path().metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::write(store.path(), "version = 2\n[items]\n").unwrap();
        assert!(store.load().is_err());
        fs::write(
            store.path(),
            "version = 1\n[items.invalid]\ncount = -1\ntime = 0\n",
        )
        .unwrap();
        assert!(store.load().is_err());
        fs::remove_file(store.path()).unwrap();
        let base = directory.join("managed");
        fs::write(&base, "preserve").unwrap();
        std::os::unix::fs::symlink(&base, store.path()).unwrap();
        assert!(store.save(&second, 3).is_err());
        assert_eq!(fs::read_to_string(base).unwrap(), "preserve");
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn favorite_and_recent_usage_rank_without_changing_identity() {
        let mut usage = Usage::default();
        usage.record("editor.desktop");
        let ranking = usage.ranking(&["terminal.desktop".to_owned()]);
        assert!(ranking["terminal.desktop"] > ranking["editor.desktop"]);
        let mut stored = Usage::default();
        stored.record("editor.desktop");
        stored.merge_local(&usage);
        assert_eq!(stored.items["editor.desktop"].0, 2);
    }
}
