use std::{
    fs::{self, File},
    io::{Read, Write},
    path::PathBuf,
    sync::Arc,
};

use crate::backend::{Item, MAX_HISTORY_BYTES, MAX_ITEM_BYTES};

const MAGIC: &[u8] = b"OXIPASTE_ITEMS_V1\0";
const FILE_LIMIT: usize = MAX_HISTORY_BYTES + 2 * 1024 * 1024;

pub(super) fn path() -> Result<PathBuf, String> {
    let directory = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or("Cannot locate clipboard history: HOME and XDG_CONFIG_HOME are unset")?;
    Ok(directory.join("oxipaste/items"))
}

pub(super) fn validate(item: &Item) -> Result<(), String> {
    if item.bytes.len() > MAX_ITEM_BYTES
        || item.mime.is_empty()
        || item.mime.len() > 256
        || item.mime.contains(['\0', '\n', '\r'])
    {
        Err("Invalid or oversized clipboard item".into())
    } else {
        Ok(())
    }
}

pub(super) struct History {
    path: PathBuf,
    limit: usize,
    pub items: Vec<Arc<Item>>,
}

impl History {
    pub fn load(path: PathBuf, limit: usize) -> Result<Self, String> {
        let mut bytes = Vec::new();
        match File::open(&path) {
            Ok(file) => {
                file.take(FILE_LIMIT as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|error| format!("Cannot read clipboard history: {error}"))?;
                if bytes.len() > FILE_LIMIT {
                    return Err("Clipboard history file exceeds supported limits".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(format!("Cannot read clipboard history: {error}")),
        }
        let items = if bytes.is_empty() {
            Vec::new()
        } else {
            parse(&bytes).ok_or("Cannot parse clipboard history; existing file was preserved")?
        };
        let mut history = Self {
            path,
            limit: limit.clamp(1, 5000),
            items,
        };
        history.trim();
        Ok(history)
    }

    fn trim(&mut self) {
        self.items.truncate(self.limit);
        let mut total = 0usize;
        self.items.retain(|item| {
            total = total.saturating_add(item.bytes.len() + item.mime.len());
            validate(item).is_ok() && total <= MAX_HISTORY_BYTES
        });
    }

    pub fn capture(&mut self, item: Arc<Item>) -> Result<bool, String> {
        validate(&item)?;
        if self.items.first() == Some(&item) {
            return Ok(false);
        }
        self.change(|items| {
            items.retain(|old| old != &item);
            items.insert(0, item);
            Ok(())
        })?;
        Ok(true)
    }

    pub fn delete(&mut self, item: &Item) -> Result<(), String> {
        self.change(|items| {
            let index = items
                .iter()
                .position(|value| value.as_ref() == item)
                .ok_or("Clipboard item no longer exists")?;
            items.remove(index);
            Ok(())
        })
    }

    pub fn clear(&mut self) -> Result<(), String> {
        self.change(|items| {
            items.clear();
            Ok(())
        })
    }

    fn change(
        &mut self,
        change: impl FnOnce(&mut Vec<Arc<Item>>) -> Result<(), String>,
    ) -> Result<(), String> {
        let previous = self.items.clone();
        change(&mut self.items)?;
        self.trim();
        if let Err(error) = self.save() {
            self.items = previous;
            return Err(format!("Cannot save clipboard history: {error}"));
        }
        Ok(())
    }

    fn save(&self) -> std::io::Result<()> {
        let directory = self
            .path
            .parent()
            .ok_or_else(|| std::io::Error::other("Invalid history path"))?;
        fs::create_dir_all(directory)?;
        let mut file = tempfile::NamedTempFile::new_in(directory)?;
        file.write_all(MAGIC)?;
        file.write_all(&(self.items.len() as u64).to_le_bytes())?;
        for item in &self.items {
            for bytes in [item.bytes.as_slice(), item.mime.as_bytes()] {
                file.write_all(&(bytes.len() as u64).to_le_bytes())?;
                file.write_all(bytes)?;
            }
        }
        file.as_file().sync_all()?;
        file.persist(&self.path).map_err(|error| error.error)?;
        let _ = File::open(directory).and_then(|directory| directory.sync_all());
        Ok(())
    }
}

fn parse(bytes: &[u8]) -> Option<Vec<Arc<Item>>> {
    if !bytes.starts_with(MAGIC) {
        let text = std::str::from_utf8(bytes).ok()?;
        let mut items = Vec::new();
        let mut total = 0usize;
        for part in text.split("<><>\n").filter(|part| !part.is_empty()) {
            if items.len() >= 5000 {
                return None;
            }
            let (content, mime) = part.split_once("<>:<>")?;
            let item = Item {
                bytes: content.as_bytes().to_vec(),
                mime: mime.into(),
            };
            validate(&item).ok()?;
            total = total.checked_add(item.bytes.len() + item.mime.len())?;
            if total > MAX_HISTORY_BYTES {
                return None;
            }
            items.push(Arc::new(item));
        }
        return Some(items);
    }
    let mut input = &bytes[MAGIC.len()..];
    fn number(input: &mut &[u8]) -> Option<usize> {
        let value = u64::from_le_bytes(input.get(..8)?.try_into().ok()?);
        *input = &input[8..];
        usize::try_from(value).ok()
    }
    fn part(input: &mut &[u8], limit: usize) -> Option<Vec<u8>> {
        let count = number(input)?;
        if count > limit {
            return None;
        }
        let value = input.get(..count)?.to_vec();
        *input = &input[count..];
        Some(value)
    }
    let count = number(&mut input)?;
    if count > 5000 {
        return None;
    }
    let mut items = Vec::with_capacity(count);
    let mut total = 0usize;
    for _ in 0..count {
        let item = Item {
            bytes: part(&mut input, MAX_ITEM_BYTES)?,
            mime: String::from_utf8(part(&mut input, 256)?).ok()?,
        };
        validate(&item).ok()?;
        total = total.checked_add(item.bytes.len() + item.mime.len())?;
        if total > MAX_HISTORY_BYTES {
            return None;
        }
        items.push(Arc::new(item));
    }
    input.is_empty().then_some(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn item(bytes: &[u8], mime: &str) -> Arc<Item> {
        Arc::new(Item {
            bytes: bytes.into(),
            mime: mime.into(),
        })
    }

    #[test]
    fn binary_history_is_private_persistent_and_content_addressed() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("items");
        let mut history = History::load(path.clone(), 2).unwrap();
        let selected = item(b"original\0\xff<>:<>\n", "application/octet-stream");
        history.capture(selected.clone()).unwrap();
        history.capture(item(b"newer", "text/plain")).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let mut loaded = History::load(path, 2).unwrap();
        assert_eq!(loaded.items, history.items);
        loaded.delete(&selected).unwrap();
        assert_eq!(loaded.items.len(), 1);
        loaded.capture(item(b"newer", "UTF8_STRING")).unwrap();
        assert_eq!(loaded.items.len(), 2, "MIME is part of payload identity");
        loaded.clear().unwrap();
        assert!(
            History::load(loaded.path.clone(), 2)
                .unwrap()
                .items
                .is_empty()
        );
    }

    #[test]
    fn migration_count_limits_corruption_and_failed_writes_preserve_history() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("items");
        fs::write(
            &path,
            b"first<>:<>text/plain<><>\nsecond<>:<>UTF8_STRING<><>\n",
        )
        .unwrap();
        let mut history = History::load(path.clone(), 1).unwrap();
        assert_eq!(history.items.len(), 1);
        history.capture(item(b"third", "text/plain")).unwrap();
        assert!(fs::read(&path).unwrap().starts_with(MAGIC));
        let before = history.items.clone();
        history.path = directory.path().join("missing/../items");
        fs::write(directory.path().join("missing"), b"not a directory").unwrap();
        assert!(history.clear().is_err());
        assert_eq!(history.items, before);
        let oversized = b"first<>:<>text/plain<><>\n".repeat(5001);
        fs::write(&path, &oversized).unwrap();
        assert!(History::load(path.clone(), 100).is_err());
        assert_eq!(fs::read(&path).unwrap(), oversized);
        fs::write(&path, b"OXIPASTE_ITEMS_V1\0broken").unwrap();
        assert!(History::load(path.clone(), 100).is_err());
        assert_eq!(fs::read(path).unwrap(), b"OXIPASTE_ITEMS_V1\0broken");
    }
}
