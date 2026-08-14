use std::{
    fs::{File, OpenOptions},
    io,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use rustix::fs::{FlockOperation, flock};

use crate::config::{InstancePolicy, get_oxirun_dir};

pub struct SingleInstanceGuard {
    _file: File,
}

pub enum SingleInstanceError {
    AlreadyRunning,
    Io(io::Error),
}

impl std::fmt::Display for SingleInstanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRunning => write!(f, "another oxibar instance is already running"),
            Self::Io(err) => write!(f, "could not acquire single-instance lock: {err}"),
        }
    }
}

impl std::fmt::Debug for SingleInstanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}

fn default_lock_path() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(get_oxirun_dir)
        .join("oxibar.lock")
}

pub fn acquire(
    policy: &InstancePolicy,
) -> Result<Option<SingleInstanceGuard>, SingleInstanceError> {
    if policy.allow_multiple_instances {
        return Ok(None);
    }

    let path = policy.lock_path.clone().unwrap_or_else(default_lock_path);
    let file = open_lock_file(&path)?;

    match flock(&file, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Ok(Some(SingleInstanceGuard { _file: file })),
        Err(err) => {
            let io_err: io::Error = err.into();
            if io_err.kind() == io::ErrorKind::WouldBlock {
                return Err(SingleInstanceError::AlreadyRunning);
            }
            Err(SingleInstanceError::Io(io_err))
        }
    }
}

fn open_lock_file(path: &Path) -> Result<File, SingleInstanceError> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
        .map_err(SingleInstanceError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    fn temp_lock_path() -> PathBuf {
        temp_dir().join(format!("oxibar-lock-test-{}", std::process::id()))
    }

    #[test]
    fn acquire_returns_none_when_multiple_instances_allowed() {
        let policy = InstancePolicy {
            allow_multiple_instances: true,
            lock_path: None,
        };
        assert!(acquire(&policy).unwrap().is_none());
    }

    #[test]
    fn second_acquire_reports_already_running() {
        let path = temp_lock_path();
        let _ = std::fs::remove_file(&path);
        let policy = InstancePolicy {
            allow_multiple_instances: false,
            lock_path: Some(path.clone()),
        };

        let first = acquire(&policy).expect("first acquire should succeed");
        assert!(first.is_some());

        let second = acquire(&policy);
        assert!(matches!(second, Err(SingleInstanceError::AlreadyRunning)));

        drop(first);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lock_is_released_when_guard_dropped() {
        let path = temp_lock_path();
        let _ = std::fs::remove_file(&path);
        let policy = InstancePolicy {
            allow_multiple_instances: false,
            lock_path: Some(path.clone()),
        };

        let first = acquire(&policy).expect("first acquire should succeed");
        drop(first);

        let second = acquire(&policy).expect("acquire should succeed after guard dropped");
        drop(second);
        let _ = std::fs::remove_file(&path);
    }
}
