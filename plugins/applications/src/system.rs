use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};

use crate::config::Config;

const DESKTOP_GROUP: &str = "[Desktop Entry]";

#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub(crate) id: usize,
    pub(crate) desktop_id: String,
    pub(crate) window_class: String,
    pub(crate) executable: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) icon: Option<PathBuf>,
    icon_name: Option<String>,
    keywords: Vec<String>,
    argv: Vec<String>,
    working_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Match {
    pub(crate) index: usize,
    pub(crate) score: i64,
}

pub(crate) fn load_entries(config: &Config) -> Vec<Entry> {
    let base = xdg::BaseDirectories::new();
    let dirs: Vec<_> = base
        .get_data_home()
        .into_iter()
        .chain(base.get_data_dirs())
        .collect();
    let desktops: Vec<_> = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|desktop| !desktop.is_empty())
        .map(str::to_owned)
        .collect();
    let mut entries = load_entries_from(&dirs, config, &desktops);
    let wanted: HashSet<_> = entries
        .iter()
        .filter_map(|entry| entry.icon_name.as_deref())
        .filter(|name| !Path::new(name).is_absolute())
        .collect();
    let icons = load_icons(&dirs, &wanted);
    for entry in &mut entries {
        entry.icon = entry.icon_name.as_ref().and_then(|name| {
            let path = PathBuf::from(name);
            if path.is_absolute() {
                path.is_file().then_some(path)
            } else {
                icons.get(name).cloned()
            }
        });
    }
    entries
}

fn load_entries_from(dirs: &[PathBuf], config: &Config, desktops: &[String]) -> Vec<Entry> {
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for dir in dirs {
        let root = dir.join("applications");
        let mut paths = Vec::new();
        collect_desktop_paths(&root, &mut HashSet::new(), &mut paths);
        paths.sort();
        for path in paths {
            // Hidden user entries must also mask lower-priority system entries.
            let id = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('/', "-");
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Ok(contents) = fs::read_to_string(&path)
                && let Some(mut entry) = parse_entry(&contents, &path, config, desktops)
            {
                entry.desktop_id = id;
                entries.push(entry);
            }
        }
    }
    entries.sort_by_cached_key(|entry| entry.name.to_lowercase());
    entries
}

fn collect_desktop_paths(dir: &Path, visited: &mut HashSet<PathBuf>, paths: &mut Vec<PathBuf>) {
    let Ok(canonical) = dir.canonicalize() else {
        return;
    };
    if !visited.insert(canonical) {
        return;
    }
    let Ok(files) = fs::read_dir(dir) else { return };
    for file in files.flatten() {
        let path = file.path();
        if path.is_dir() {
            collect_desktop_paths(&path, visited, paths);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "desktop")
        {
            paths.push(path);
        }
    }
}

fn parse_entry(contents: &str, path: &Path, config: &Config, desktops: &[String]) -> Option<Entry> {
    let mut in_group = false;
    let mut values = HashMap::new();
    for line in contents.lines().map(str::trim) {
        if line.starts_with('[') {
            if in_group {
                break;
            }
            in_group = line == DESKTOP_GROUP;
        } else if in_group
            && !line.starts_with('#')
            && let Some((key, value)) = line.split_once('=')
        {
            values.entry(key.trim()).or_insert(value.trim());
        }
    }
    if values.get("Type") != Some(&"Application")
        || values.get("Hidden") == Some(&"true")
        || values.get("NoDisplay") == Some(&"true")
        || !desktop_visible(&values, desktops)
    {
        return None;
    }
    if let Some(exec) = values.get("TryExec")
        && !executable_exists(exec)
    {
        return None;
    }
    let name = unescape_value(values.get("Name")?);
    let icon_name = values.get("Icon").map(|value| unescape_value(value));
    let mut argv = expand_exec(values.get("Exec")?, &name, icon_name.as_deref(), path).ok()?;
    if name.is_empty() || argv.is_empty() || argv[0].is_empty() {
        return None;
    }
    if values.get("Terminal") == Some(&"true") {
        argv.splice(0..0, config.terminal.clone());
    }
    let keywords = ["Categories", "Keywords", "GenericName"]
        .into_iter()
        .filter_map(|key| values.get(key))
        .flat_map(|value| value.split(';'))
        .filter(|value| !value.is_empty())
        .map(unescape_value)
        .collect();
    Some(Entry {
        id: 0,
        desktop_id: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        window_class: values
            .get("StartupWMClass")
            .map(|value| unescape_value(value))
            .unwrap_or_default(),
        executable: Path::new(&argv[0])
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        name,
        description: values
            .get("Comment")
            .or_else(|| values.get("GenericName"))
            .map(|value| unescape_value(value))
            .unwrap_or_default(),
        icon: None,
        icon_name,
        keywords,
        argv,
        working_dir: values
            .get("Path")
            .filter(|value| !value.is_empty())
            .map(|value| PathBuf::from(unescape_value(value))),
    })
}

#[cfg(test)]
pub(crate) fn test_entry(id: &str, name: &str) -> Entry {
    parse_entry(
        &format!("[Desktop Entry]\nType=Application\nName={name}\nExec=/bin/true\n"),
        &PathBuf::from(id),
        &crate::config::read_config(&oxibar_plugin_api::toml::Table::new()),
        &[],
    )
    .unwrap()
}

fn desktop_visible(values: &HashMap<&str, &str>, desktops: &[String]) -> bool {
    let matches = |list: &&str| {
        list.split(';')
            .any(|name| desktops.iter().any(|desktop| desktop == name))
    };
    values.get("OnlyShowIn").is_none_or(matches) && !values.get("NotShowIn").is_some_and(matches)
}

fn executable_exists(exec: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let executable = |path: PathBuf| {
        path.metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    };
    let path = Path::new(exec);
    if path.is_absolute() {
        return executable(path.to_owned());
    }
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|path| executable(path.join(exec))))
}

fn unescape_value(value: &str) -> String {
    let mut result = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('s') => result.push(' '),
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some('\\') => result.push('\\'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(ch);
        }
    }
    result
}

fn expand_exec(
    exec: &str,
    name: &str,
    icon: Option<&str>,
    path: &Path,
) -> Result<Vec<String>, String> {
    let words = shell_words::split(&unescape_value(exec)).map_err(|error| error.to_string())?;
    let mut argv = Vec::new();
    for word in words {
        if word == "%i" {
            if let Some(icon) = icon {
                argv.extend(["--icon".to_owned(), icon.to_owned()]);
            }
            continue;
        }
        if ["%f", "%F", "%u", "%U", "%d", "%D", "%n", "%N", "%v", "%m"].contains(&word.as_str()) {
            continue;
        }
        let mut arg = String::new();
        let mut chars = word.chars();
        while let Some(ch) = chars.next() {
            if ch != '%' {
                arg.push(ch);
                continue;
            }
            match chars.next() {
                Some('%') => arg.push('%'),
                Some('c') => arg.push_str(name),
                Some('k') => arg.push_str(&path.to_string_lossy()),
                Some('f' | 'u' | 'd' | 'n' | 'v' | 'm') => (),
                _ => return Err("unsupported desktop Exec field code".to_owned()),
            }
        }
        argv.push(arg);
    }
    Ok(argv)
}

fn load_icons(dirs: &[PathBuf], wanted: &HashSet<&str>) -> HashMap<String, PathBuf> {
    let mut icons = HashMap::new();
    if wanted.is_empty() {
        return icons;
    }
    for dir in dirs {
        for theme in ["hicolor", "Adwaita"] {
            if let Ok(sizes) = fs::read_dir(dir.join("icons").join(theme)) {
                let mut sizes: Vec<_> = sizes.flatten().map(|size| size.path()).collect();
                sizes.sort();
                for size in sizes {
                    collect_icons(&size.join("apps"), wanted, &mut icons);
                }
            }
        }
        collect_icons(&dir.join("pixmaps"), wanted, &mut icons);
    }
    icons
}

fn collect_icons(dir: &Path, wanted: &HashSet<&str>, icons: &mut HashMap<String, PathBuf>) {
    let Ok(files) = fs::read_dir(dir) else { return };
    for file in files.flatten() {
        let path = file.path();
        if !path
            .extension()
            .is_some_and(|extension| extension == "png" || extension == "svg")
        {
            continue;
        }
        if let Some(name) = path.file_stem().and_then(|name| name.to_str())
            && wanted.contains(name)
        {
            icons.entry(name.to_owned()).or_insert(path);
        }
    }
}

pub(crate) fn search(entries: &[Entry], query: &str, limit: usize) -> Vec<Match> {
    if query.trim().is_empty() {
        return (0..entries.len().min(limit))
            .map(|index| Match { index, score: 0 })
            .collect();
    }
    let matcher = SkimMatcherV2::default().ignore_case();
    let query = query.trim();
    let mut matches: Vec<_> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let score = std::iter::once(&entry.name)
                .chain(&entry.keywords)
                .filter_map(|candidate| matcher.fuzzy_match(candidate, query))
                .max()?;
            Some(Match { index, score })
        })
        .collect();
    matches.sort_by(|first, second| {
        second
            .score
            .cmp(&first.score)
            .then(first.index.cmp(&second.index))
    });
    matches.truncate(limit);
    matches
}

pub(crate) fn launch(entry: &Entry) -> Result<(), String> {
    let Some(program) = entry.argv.first() else {
        return Err("empty application command".to_owned());
    };
    let mut command = Command::new(program);
    command
        .args(&entry.argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = entry.working_dir.as_ref() {
        command.current_dir(dir);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not launch {}: {error}", entry.name))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxibar_plugin_api::toml::Table;

    fn config() -> Config {
        crate::config::read_config(&Table::new())
    }

    fn entry(extra: &str) -> Option<Entry> {
        parse_entry(
            &format!(
                "# comment\n\n[Desktop Entry]\nType=Application\nName=Text Editor\nExec=editor %F\n{extra}"
            ),
            Path::new("/tmp/editor.desktop"),
            &config(),
            &[],
        )
    }

    #[test]
    fn filters_hidden_entries_and_ignores_desktop_actions() {
        assert!(entry("Hidden=true").is_none());
        assert!(entry("NoDisplay=true").is_none());
        assert!(entry("OnlyShowIn=GNOME;").is_none());
        let entry = entry("[Desktop Action New]\nName=Wrong\nExec=wrong").unwrap();
        assert_eq!(entry.name, "Text Editor");
        assert_eq!(entry.argv, ["editor"]);
    }

    #[test]
    fn exec_expands_fields_without_shell_interpretation() {
        let args = expand_exec(
            "\"/path with spaces/editor\" %c %k %i %% %U \"$HOME;touch /tmp/no\"",
            "Text Editor",
            Some("editor"),
            Path::new("/tmp/file with spaces.desktop"),
        )
        .unwrap();
        assert_eq!(
            args,
            [
                "/path with spaces/editor",
                "Text Editor",
                "/tmp/file with spaces.desktop",
                "--icon",
                "editor",
                "%",
                "$HOME;touch /tmp/no"
            ]
        );
        assert!(expand_exec("editor %z", "Editor", None, Path::new("a")).is_err());
        assert!(expand_exec("editor \"unterminated", "Editor", None, Path::new("a")).is_err());
    }

    #[test]
    fn terminal_and_all_keywords_survive_parsing() {
        let entry =
            entry("Terminal=true\nCategories=Utility;Development;\nKeywords=code;edit;notes;")
                .unwrap();
        assert_eq!(entry.argv, ["kitty", "editor"]);
        assert_eq!(
            entry.keywords,
            ["Utility", "Development", "code", "edit", "notes"]
        );
        assert!(!search(&[entry], "notes", 7).is_empty());
    }

    #[test]
    fn empty_query_is_useful_and_results_are_bounded() {
        let entries = vec![entry("").unwrap(); 10];
        assert_eq!(search(&entries, "", 3).len(), 3);
        assert_eq!(search(&entries, "te", 3).len(), 3);
        assert!(search(&entries, "zzzzzz", 3).is_empty());
    }

    #[test]
    fn user_hidden_entry_masks_system_entry_by_desktop_id() {
        let root =
            std::env::temp_dir().join(format!("oxibar-app-precedence-{}", std::process::id()));
        let user = root.join("user");
        let system = root.join("system");
        for dir in [&user, &system] {
            fs::create_dir_all(dir.join("applications")).unwrap();
        }
        fs::write(
            user.join("applications/editor.desktop"),
            "[Desktop Entry]\nHidden=true",
        )
        .unwrap();
        fs::write(
            system.join("applications/editor.desktop"),
            "[Desktop Entry]\nType=Application\nName=Editor\nExec=editor",
        )
        .unwrap();
        assert!(load_entries_from(&[user, system], &config(), &[]).is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
