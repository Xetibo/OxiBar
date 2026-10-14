use std::{
    fs::File,
    io::{self, Read},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
};

pub(crate) fn default_username() -> Option<String> {
    let passwd = read_bounded(Path::new("/etc/passwd"), 1_048_576).ok()?;
    let definitions = read_bounded(Path::new("/etc/login.defs"), 65_536).unwrap_or_default();
    candidates(&passwd, &definitions)
        .into_iter()
        .next()
        .map(|(_, username)| username)
}

fn read_bounded(path: &Path, limit: u64) -> io::Result<String> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != 0
        || metadata.permissions().mode() & 0o022 != 0
        || metadata.len() > limit
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid user catalog",
        ));
    }
    let mut text = String::new();
    file.take(limit + 1).read_to_string(&mut text)?;
    if text.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "oversized user catalog",
        ));
    }
    Ok(text)
}

fn candidates(passwd: &str, definitions: &str) -> Vec<(u32, String)> {
    let mut minimum = 1000;
    let mut maximum = 60_000;
    for line in definitions.lines() {
        let mut words = line
            .split('#')
            .next()
            .unwrap_or_default()
            .split_whitespace();
        match (
            words.next(),
            words.next().and_then(|value| value.parse().ok()),
        ) {
            (Some("UID_MIN"), Some(value)) => minimum = value,
            (Some("UID_MAX"), Some(value)) => maximum = value,
            _ => {}
        }
    }
    let mut users = Vec::new();
    for line in passwd.lines().take(16_384) {
        let fields: Vec<_> = line.split(':').take(8).collect();
        if fields.len() != 7 {
            continue;
        }
        let Ok(uid) = fields[2].parse::<u32>() else {
            continue;
        };
        let username = fields[0];
        let shell = Path::new(fields[6])
            .file_name()
            .and_then(|name| name.to_str());
        if uid == 0
            || !(minimum..=maximum).contains(&uid)
            || username.is_empty()
            || username.len() > 256
            || username
                .chars()
                .any(|value| value.is_control() || value.is_whitespace())
            || matches!(
                username,
                "greetd" | "greeter" | "oxisession" | "gdm" | "sddm" | "lightdm" | "nobody"
            )
            || !Path::new(fields[5]).is_absolute()
            || matches!(fields[5], "/" | "/var/empty" | "/nonexistent")
            || !Path::new(fields[6]).is_absolute()
            || matches!(shell, None | Some("nologin" | "false"))
        {
            continue;
        }
        users.push((uid, username.to_owned()));
    }
    users.sort();
    users.dedup();
    users
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_user_respects_system_uid_range_and_excludes_service_accounts() {
        let passwd = "root:x:0:0::/root:/bin/bash\n\
            later:x:1010:100::/home/later:/bin/zsh\n\
            greetd:x:1001:100::/home/greetd:/bin/bash\n\
            first:x:1000:100::/home/first:/bin/bash\n\
            daemon:x:1002:100::/home/daemon:/bin/nologin\n\
            disabled:x:1003:100::/home/disabled:/bin/false\n\
            nixbuild:x:30001:30000::/var/empty:/bin/bash\n\
            malformed:x:not-a-uid:100::/home/bad:/bin/bash";
        assert_eq!(
            candidates(passwd, "UID_MIN 1000\nUID_MAX 29999"),
            vec![(1000, "first".into()), (1010, "later".into())]
        );
        assert_eq!(
            candidates(passwd, "UID_MIN 1010 # custom range\nUID_MAX 29999")[0].1,
            "later"
        );
        assert!(candidates("daemon:x:999:999::/home/daemon:/bin/bash", "").is_empty());
    }
}
