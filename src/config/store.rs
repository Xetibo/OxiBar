use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use oxibar_plugin_api::SettingDescriptor;
use toml::{Table, Value};

use super::schema::{self, HostConfig};

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Builtin,
    Base,
    Gui,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Self::Builtin => "Built-in",
            Self::Base => "Nix / base",
            Self::Gui => "GUI override",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigSnapshot {
    pub revision: u64,
    pub base: Table,
    pub overrides: Table,
    pub effective: Table,
    pub host: HostConfig,
    pub schema: Vec<SettingDescriptor>,
}

impl ConfigSnapshot {
    pub fn from_tables(
        base: Table,
        overrides: Table,
        schema: Vec<SettingDescriptor>,
        revision: u64,
    ) -> Result<Self, String> {
        let mut effective = schema::defaults(&schema)?;
        merge(&mut effective, &base);
        merge(&mut effective, &overrides);
        schema::validate(&effective, &schema)?;
        let host = HostConfig::from_validated(&effective);
        Ok(Self {
            revision,
            base,
            overrides,
            effective,
            host,
            schema,
        })
    }

    pub fn source(&self, path: &str) -> Source {
        if get_path(&self.overrides, path).is_some() {
            Source::Gui
        } else if get_path(&self.base, path).is_some() {
            Source::Base
        } else {
            Source::Builtin
        }
    }

    pub fn base_value(&self, path: &str) -> Option<Value> {
        get_path(&self.base, path).cloned().or_else(|| {
            self.schema
                .iter()
                .find(|field| field.path == path)
                .and_then(|field| field.default.clone())
        })
    }
}

#[derive(Debug, Clone)]
pub enum Mutation {
    Set(String, Value),
    Reset(String),
    ResetAll,
    Replace(Table),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Inputs {
    base: Option<Vec<u8>>,
    overrides: Option<Vec<u8>>,
}

pub struct ConfigStore {
    pub directory: PathBuf,
    snapshot: Arc<ConfigSnapshot>,
    observed: Option<Inputs>,
    pub error: Option<String>,
    persistent: bool,
}

impl ConfigStore {
    pub fn open(directory: PathBuf) -> Self {
        let mut store = Self {
            directory,
            snapshot: Arc::new(
                ConfigSnapshot::from_tables(Table::new(), Table::new(), schema::host_schema(), 0)
                    .expect("valid defaults"),
            ),
            observed: None,
            error: None,
            persistent: true,
        };
        if let Err(error) = fs::create_dir_all(&store.directory)
            .map_err(|error| error.to_string())
            .and_then(|_| store.reload().map(|_| ()))
        {
            store.error = Some(error);
        }
        store
    }

    pub fn snapshot(&self) -> Arc<ConfigSnapshot> {
        self.snapshot.clone()
    }

    pub fn memory(base: Table, overrides: Table) -> Self {
        Self {
            directory: PathBuf::new(),
            snapshot: Arc::new(
                ConfigSnapshot::from_tables(base, overrides, schema::host_schema(), 1)
                    .expect("valid test configuration"),
            ),
            observed: Some(Inputs::default()),
            error: None,
            persistent: false,
        }
    }

    pub fn register_schema(&mut self, fields: Vec<SettingDescriptor>) -> Result<(), String> {
        let mut schema = self.snapshot.schema.clone();
        for field in fields {
            path_parts(&field.path)?;
            if !schema.iter().any(|existing| existing.path == field.path) {
                schema.push(field);
            }
        }
        self.snapshot = Arc::new(ConfigSnapshot::from_tables(
            self.snapshot.base.clone(),
            self.snapshot.overrides.clone(),
            schema,
            self.snapshot.revision,
        )?);
        Ok(())
    }

    fn read_inputs(&self) -> Result<Inputs, String> {
        if !self.persistent {
            return Ok(self.observed.clone().unwrap_or_default());
        }
        Ok(Inputs {
            base: read_optional(&self.directory.join("config.toml"))?,
            overrides: read_optional(&self.directory.join("overrides.toml"))?,
        })
    }

    pub fn reload(&mut self) -> Result<bool, String> {
        let result = self.reload_inner();
        self.error = result.as_ref().err().cloned();
        result
    }

    fn reload_inner(&mut self) -> Result<bool, String> {
        let inputs = self.read_inputs()?;
        if self.observed.as_ref() == Some(&inputs) {
            return Ok(false);
        }
        let base = parse_input(&inputs.base, "config.toml")?;
        let overrides = parse_input(&inputs.overrides, "overrides.toml")?;
        let snapshot = ConfigSnapshot::from_tables(
            base,
            overrides,
            self.snapshot.schema.clone(),
            self.snapshot.revision + 1,
        )?;
        self.snapshot = Arc::new(snapshot);
        self.observed = Some(inputs);
        Ok(true)
    }

    pub fn transact(
        &mut self,
        mutation: Mutation,
        expected_revision: Option<u64>,
    ) -> Result<Arc<ConfigSnapshot>, String> {
        let _lock = if self.persistent {
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(self.directory.join("settings.lock"))
                .map_err(|error| format!("settings lock: {error}"))?;
            rustix::fs::flock(&lock, rustix::fs::FlockOperation::LockExclusive)
                .map_err(|error| error.to_string())?;
            Some(lock)
        } else {
            None
        };
        self.reload()?;
        if expected_revision.is_some_and(|revision| revision != self.snapshot.revision) {
            return Err("Configuration changed; reload before applying this edit".to_owned());
        }
        let mut overrides = self.snapshot.overrides.clone();
        match mutation {
            Mutation::Set(path, value) => set_path(&mut overrides, &path, value)?,
            Mutation::Reset(path) => {
                remove_path(&mut overrides, &path)?;
            }
            Mutation::ResetAll => overrides.clear(),
            Mutation::Replace(table) => overrides = table,
        }
        let candidate = ConfigSnapshot::from_tables(
            self.snapshot.base.clone(),
            overrides,
            self.snapshot.schema.clone(),
            self.snapshot.revision + 1,
        )?;
        if candidate.overrides == self.snapshot.overrides {
            return Ok(self.snapshot());
        }
        let bytes = toml::to_string_pretty(&candidate.overrides)
            .map_err(|error| error.to_string())?
            .into_bytes();
        if self.observed.as_ref() != Some(&self.read_inputs()?) {
            return Err(
                "Configuration files changed during this edit; reload and retry".to_owned(),
            );
        }
        if self.persistent
            && let Err(error) = atomic_write(&self.directory.join("overrides.toml"), &bytes)
        {
            let _ = self.reload();
            return Err(error);
        }
        if let Some(inputs) = self.observed.as_mut() {
            inputs.overrides = Some(bytes);
        }
        self.snapshot = Arc::new(candidate);
        self.error = None;
        Ok(self.snapshot())
    }
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let mut bytes = Vec::new();
    match File::open(path) {
        Ok(file) => {
            file.take(MAX_CONFIG_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| format!("{}: {error}", path.display()))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    }
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(format!("{}: configuration exceeds 1 MiB", path.display()));
    }
    Ok(Some(bytes))
}

fn parse_input(input: &Option<Vec<u8>>, name: &str) -> Result<Table, String> {
    match input {
        None => Ok(Table::new()),
        Some(bytes) => std::str::from_utf8(bytes)
            .map_err(|error| format!("{name}: {error}"))
            .and_then(|text| toml::from_str(text).map_err(|error| format!("{name}: {error}"))),
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err("Overrides exceed 1 MiB".to_owned());
    }
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(
            "overrides.toml must be a writable regular file, not a managed symlink".to_owned(),
        );
    }
    let directory = path.parent().ok_or("missing override directory")?;
    let temp = directory.join(format!(
        ".overrides-{}-{}.tmp",
        std::process::id(),
        TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        File::open(directory)?.sync_all()
    })();
    let _ = fs::remove_file(temp);
    result.map_err(|error| format!("could not save overrides: {error}"))
}

pub fn merge(target: &mut Table, layer: &Table) {
    for (key, value) in layer {
        match (target.get_mut(key), value) {
            (Some(Value::Table(existing)), Value::Table(layer)) => merge(existing, layer),
            _ => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}

fn path_parts(path: &str) -> Result<Vec<&str>, String> {
    let parts: Vec<_> = path.split('.').collect();
    if parts.iter().any(|part| {
        part.is_empty()
            || !part
                .chars()
                .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-')
    }) {
        Err("Setting paths must be dotted nonempty names".to_owned())
    } else {
        Ok(parts)
    }
}

pub fn get_path<'a>(table: &'a Table, path: &str) -> Option<&'a Value> {
    let mut parts = path.split('.');
    let mut value = table.get(parts.next()?)?;
    for part in parts {
        value = value.as_table()?.get(part)?;
    }
    Some(value)
}

pub fn set_path(table: &mut Table, path: &str, value: Value) -> Result<(), String> {
    let parts = path_parts(path)?;
    let mut table = table;
    for part in &parts[..parts.len() - 1] {
        table = table
            .entry((*part).to_owned())
            .or_insert_with(|| Value::Table(Table::new()))
            .as_table_mut()
            .ok_or_else(|| format!("{part}: existing parent is not a table"))?;
    }
    table.insert(parts[parts.len() - 1].to_owned(), value);
    Ok(())
}

pub fn remove_path(table: &mut Table, path: &str) -> Result<(), String> {
    fn remove(table: &mut Table, parts: &[&str]) {
        if parts.len() == 1 {
            table.remove(parts[0]);
            return;
        }
        if let Some(child) = table.get_mut(parts[0]).and_then(Value::as_table_mut) {
            remove(child, &parts[1..]);
            if child.is_empty() {
                table.remove(parts[0]);
            }
        }
    }
    remove(table, &path_parts(path)?);
    Ok(())
}
