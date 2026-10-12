use std::{
    collections::BTreeMap,
    fmt, fs,
    io::{self, Read},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

const ROOTS: &[&str] = &[
    "/run/current-system/sw/share/wayland-sessions",
    "/usr/local/share/wayland-sessions",
    "/usr/share/wayland-sessions",
];
const EXEC_PATH: &[&str] = &[
    "/run/current-system/sw/bin",
    "/usr/local/bin",
    "/usr/bin",
    "/bin",
];
const MAX_DESKTOP_BYTES: u64 = 65_536;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub cmd: Vec<String>,
    pub env: Vec<String>,
}
impl fmt::Display for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.name.fmt(f)
    }
}

pub struct Catalog {
    pub sessions: Vec<Session>,
    pub warnings: Vec<String>,
}
impl Catalog {
    pub fn load() -> io::Result<Self> {
        let mut entries = BTreeMap::new();
        let mut warnings = Vec::new();
        for root in ROOTS {
            let root = Path::new(root);
            if !root.exists() {
                continue;
            }
            trusted(root)?;
            for entry in fs::read_dir(root)? {
                let path = entry?.path();
                if path
                    .extension()
                    .is_none_or(|extension| extension != "desktop")
                {
                    continue;
                }
                let Some(id) = path.file_stem().and_then(|name| name.to_str()) else {
                    continue;
                };
                if entries.contains_key(id) {
                    continue;
                }
                match load_entry(&path) {
                    Ok(Some(session)) => {
                        entries.insert(id.to_owned(), session);
                    }
                    Ok(None) => {}
                    Err(_) => warnings.push(format!("Ignored invalid or untrusted session: {id}")),
                }
            }
        }
        let mut sessions: Vec<_> = entries.into_values().collect();
        sessions.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
        Ok(Self { sessions, warnings })
    }
}

fn load_entry(path: &Path) -> io::Result<Option<Session>> {
    trusted(path)?;
    if !fs::metadata(path)?.is_file() {
        return Err(invalid("session entry is not a regular file"));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_DESKTOP_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_DESKTOP_BYTES {
        return Err(invalid("oversized desktop entry"));
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid("invalid desktop encoding"))?;
    parse(path, text, executable)
}

fn trusted(path: &Path) -> io::Result<()> {
    let resolved = path.canonicalize()?;
    for ancestor in path.ancestors().chain(resolved.ancestors()) {
        let metadata = fs::symlink_metadata(ancestor)?;
        let sticky_root_directory = metadata.is_dir() && metadata.mode() & 0o1000 != 0;
        if metadata.uid() != 0
            || (metadata.mode() & 0o022 != 0
                && !sticky_root_directory
                && !metadata.file_type().is_symlink())
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "session path is not administrator-owned",
            ));
        }
    }
    Ok(())
}

fn executable(name: &str) -> Option<PathBuf> {
    let paths = if Path::new(name).is_absolute() {
        vec![PathBuf::from(name)]
    } else if !name.contains('/') {
        EXEC_PATH
            .iter()
            .map(|root| Path::new(root).join(name))
            .collect()
    } else {
        return None;
    };
    paths.into_iter().find(|path| {
        fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.mode() & 0o111 != 0)
            && trusted(path).is_ok()
    })
}

fn parse(
    path: &Path,
    text: &str,
    resolve: impl Fn(&str) -> Option<PathBuf>,
) -> io::Result<Option<Session>> {
    let mut fields = BTreeMap::new();
    let mut entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            entry = line == "[Desktop Entry]";
            continue;
        }
        if !entry || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(invalid("invalid desktop field"));
        };
        if fields.insert(key.trim(), value.trim()).is_some() {
            return Err(invalid("duplicate desktop field"));
        }
    }
    if fields.get("Hidden") == Some(&"true") || fields.get("NoDisplay") == Some(&"true") {
        return Ok(None);
    }
    if fields.get("Type") != Some(&"Application") {
        return Err(invalid("desktop session is not an application"));
    }
    let id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| invalid("invalid desktop identifier"))?;
    if id.is_empty()
        || id.len() > 128
        || !id
            .chars()
            .all(|c| c.is_alphanumeric() || "-_ .".contains(c))
    {
        return Err(invalid("invalid session identifier"));
    }
    let name = fields.get("Name").copied().unwrap_or(id);
    if name.is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
        return Err(invalid("invalid session name"));
    }
    if let Some(try_exec) = fields.get("TryExec")
        && resolve(try_exec).is_none()
    {
        return Ok(None);
    }
    let exec = fields
        .get("Exec")
        .ok_or_else(|| invalid("session has no executable"))?;
    let words = shell_words::split(exec).map_err(|_| invalid("invalid session command"))?;
    if words.first().is_some_and(|word| word.contains('%')) {
        return Err(invalid("field code in executable"));
    }
    let mut cmd = Vec::new();
    for word in words {
        if ["%f", "%F", "%u", "%U", "%i"].contains(&word.as_str()) {
            continue;
        }
        let mut expanded = String::new();
        let mut characters = word.chars();
        while let Some(character) = characters.next() {
            if character != '%' {
                expanded.push(character);
                continue;
            }
            match characters.next() {
                Some('%') => expanded.push('%'),
                Some('c') => expanded.push_str(name),
                Some('k') => expanded.push_str(
                    path.to_str()
                        .ok_or_else(|| invalid("invalid desktop path"))?,
                ),
                _ => return Err(invalid("unsupported session field code")),
            }
        }
        if expanded.len() > 4096 || expanded.contains('\0') {
            return Err(invalid("invalid session argument"));
        }
        cmd.push(expanded);
    }
    if cmd.is_empty() || cmd.len() > 128 {
        return Err(invalid("empty or excessive session command"));
    }
    cmd[0] = resolve(&cmd[0])
        .ok_or_else(|| invalid("session executable unavailable"))?
        .to_string_lossy()
        .into_owned();
    let desktops = fields
        .get("DesktopNames")
        .copied()
        .unwrap_or(id)
        .trim_end_matches(';')
        .replace(';', ":");
    if desktops.len() > 256 || desktops.chars().any(|c| c.is_control() || c == '=') {
        return Err(invalid("invalid desktop names"));
    }
    let env = vec![
        "XDG_SESSION_TYPE=wayland".into(),
        format!("DESKTOP_SESSION={id}"),
        format!("XDG_SESSION_DESKTOP={id}"),
        format!("XDG_CURRENT_DESKTOP={desktops}"),
    ];
    Ok(Some(Session {
        id: id.into(),
        name: name.into(),
        cmd,
        env,
    }))
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_fields_are_argv_not_shell_and_metadata_is_explicit() {
        let session = parse(Path::new("/system/fixture.desktop"), "[Desktop Entry]\nType=Application\nName=Fixture Desktop\nExec=fixture --name %c --entry %k %%literal %f\nTryExec=fixture\nDesktopNames=Fixture;Wayland;\n", |name| (name == "fixture").then(|| PathBuf::from("/system/bin/fixture"))).unwrap().unwrap();
        assert_eq!(
            session.cmd,
            [
                "/system/bin/fixture",
                "--name",
                "Fixture Desktop",
                "--entry",
                "/system/fixture.desktop",
                "%literal"
            ]
        );
        assert!(
            session
                .env
                .contains(&"XDG_CURRENT_DESKTOP=Fixture:Wayland".into())
        );
        let base = "[Desktop Entry]\nType=Application\nName=Fixture\n";
        assert!(
            parse(
                Path::new("fixture.desktop"),
                &format!("{base}Exec=fixture %unknown"),
                |_| Some("/bin/fixture".into())
            )
            .is_err()
        );
        assert!(
            parse(
                Path::new("fixture.desktop"),
                &format!("{base}Exec=fixture\nTryExec=missing"),
                |_| None
            )
            .unwrap()
            .is_none()
        );
        assert!(
            parse(
                Path::new("fixture.desktop"),
                &format!("{base}Exec=fixture\nHidden=true"),
                |_| Some("/bin/fixture".into())
            )
            .unwrap()
            .is_none()
        );
        use std::os::unix::fs::PermissionsExt;
        let path =
            std::env::temp_dir().join(format!("oxi-untrusted-session-{}", std::process::id()));
        fs::write(&path, b"fixture").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(trusted(&path).is_err());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn untrusted_link_cannot_borrow_ownership_from_its_target() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let directory =
            std::env::temp_dir().join(format!("oxi-untrusted-links-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o777)).unwrap();
        let link = directory.join("session-link");
        symlink("/", &link).unwrap();
        assert!(trusted(&link).is_err());
        fs::remove_file(link).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
