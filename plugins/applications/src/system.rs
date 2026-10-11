use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use oxibar_plugin_api::worker::CancelToken;

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
    pub(crate) action: bool,
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

pub(crate) fn data_dirs() -> Vec<PathBuf> {
    let base = xdg::BaseDirectories::new();
    base.get_data_home()
        .into_iter()
        .chain(base.get_data_dirs())
        .collect()
}

pub(crate) fn load_entries_cancellable(config: &Config, cancel: &CancelToken) -> Vec<Entry> {
    let dirs = data_dirs();
    let desktops: Vec<_> = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|desktop| !desktop.is_empty())
        .map(str::to_owned)
        .collect();
    let mut entries = load_entries_from_cancel(&dirs, config, &desktops, cancel);
    let wanted: HashSet<_> = entries
        .iter()
        .filter_map(|entry| entry.icon_name.as_deref())
        .filter(|name| !Path::new(name).is_absolute())
        .collect();
    let icons = load_icons(&dirs, &config.icon_theme, &wanted, cancel);
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

#[cfg(test)]
fn load_entries_from(dirs: &[PathBuf], config: &Config, desktops: &[String]) -> Vec<Entry> {
    load_entries_from_cancel(dirs, config, desktops, &CancelToken::default())
}

fn load_entries_from_cancel(
    dirs: &[PathBuf],
    config: &Config,
    desktops: &[String],
    cancel: &CancelToken,
) -> Vec<Entry> {
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for dir in dirs {
        if entries.len() >= 50_000 {
            break;
        }
        let root = dir.join("applications");
        let mut paths = Vec::new();
        collect_desktop_paths(&root, &mut HashSet::new(), &mut paths, cancel);
        paths.sort();
        for path in paths {
            if entries.len() >= 50_000 {
                break;
            }
            if cancel.is_cancelled() {
                return Vec::new();
            }
            // Hidden user entries must also mask lower-priority system entries.
            let id = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('/', "-");
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Ok(contents) = read_small_file(&path)
                && let Some(mut entry) = parse_entry(&contents, &path, config, desktops)
            {
                entry.desktop_id = id.clone();
                let actions = parse_actions(&contents, &path, config, &entry);
                entries.push(entry);
                let remaining = 50_000usize.saturating_sub(entries.len());
                entries.extend(actions.into_iter().take(remaining));
            }
        }
    }
    entries.sort_by_cached_key(|entry| entry.name.to_lowercase());
    entries
}

fn collect_desktop_paths(
    dir: &Path,
    visited: &mut HashSet<PathBuf>,
    paths: &mut Vec<PathBuf>,
    cancel: &CancelToken,
) {
    if cancel.is_cancelled() || paths.len() >= 50_000 || visited.len() >= 4096 {
        return;
    }
    let Ok(canonical) = dir.canonicalize() else {
        return;
    };
    if !visited.insert(canonical) {
        return;
    }
    let Ok(files) = fs::read_dir(dir) else { return };
    for file in files.flatten() {
        if cancel.is_cancelled() || paths.len() >= 50_000 {
            break;
        }
        let path = file.path();
        if path.is_dir() {
            collect_desktop_paths(&path, visited, paths, cancel);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "desktop")
        {
            paths.push(path);
        }
    }
}

fn parse_entry(contents: &str, path: &Path, config: &Config, desktops: &[String]) -> Option<Entry> {
    let values = group_values(contents, DESKTOP_GROUP);
    parse_values(&values, path, config, desktops)
}

fn group_values<'a>(contents: &'a str, group: &str) -> HashMap<&'a str, &'a str> {
    let mut in_group = false;
    let mut values = HashMap::new();
    for line in contents.lines().map(str::trim) {
        if line.starts_with('[') {
            if in_group {
                break;
            }
            in_group = line == group;
        } else if in_group
            && !line.starts_with('#')
            && let Some((key, value)) = line.split_once('=')
        {
            values.entry(key.trim()).or_insert(value.trim());
        }
    }
    values
}

fn locale_names(locale: &str) -> Vec<String> {
    let (locale, modifier) = locale
        .split_once('@')
        .map_or((locale, None), |(base, modifier)| (base, Some(modifier)));
    let locale = locale.split('.').next().unwrap_or(locale);
    if locale.is_empty() || ["C", "POSIX"].contains(&locale) {
        return Vec::new();
    }
    let language = locale.split('_').next().unwrap_or(locale);
    let mut names = Vec::new();
    if let Some(modifier) = modifier {
        names.push(format!("{locale}@{modifier}"));
    }
    names.push(locale.to_owned());
    if language != locale {
        if let Some(modifier) = modifier {
            names.push(format!("{language}@{modifier}"));
        }
        names.push(language.to_owned());
    }
    names
}

fn localized<'a>(values: &HashMap<&'a str, &'a str>, key: &str, locale: &str) -> Option<&'a str> {
    locale_names(locale)
        .iter()
        .find_map(|locale| values.get(format!("{key}[{locale}]").as_str()).copied())
        .or_else(|| values.get(key).copied())
}

fn parse_values(
    values: &HashMap<&str, &str>,
    path: &Path,
    config: &Config,
    desktops: &[String],
) -> Option<Entry> {
    if values.get("Type") != Some(&"Application")
        || values.get("Hidden") == Some(&"true")
        || values.get("NoDisplay") == Some(&"true")
        || !desktop_visible(values, desktops)
    {
        return None;
    }
    if let Some(exec) = values.get("TryExec")
        && !executable_exists(exec)
    {
        return None;
    }
    let name = unescape_value(localized(values, "Name", &config.locale)?);
    let icon_name = values.get("Icon").map(|value| unescape_value(value));
    let mut argv = expand_exec(values.get("Exec")?, &name, icon_name.as_deref(), path).ok()?;
    if name.is_empty() || argv.is_empty() || argv[0].is_empty() {
        return None;
    }
    let executable = Path::new(&argv[0])
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    if values.get("Terminal") == Some(&"true") {
        argv.splice(0..0, config.terminal.clone());
    }
    let keywords = ["Categories", "Keywords", "GenericName"]
        .into_iter()
        .filter_map(|key| localized(values, key, &config.locale))
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
        executable,
        name,
        description: localized(values, "Comment", &config.locale)
            .or_else(|| localized(values, "GenericName", &config.locale))
            .map(unescape_value)
            .unwrap_or_default(),
        icon: None,
        action: false,
        icon_name,
        keywords,
        argv,
        working_dir: values
            .get("Path")
            .filter(|value| !value.is_empty())
            .map(|value| PathBuf::from(unescape_value(value))),
    })
}

fn parse_actions(contents: &str, path: &Path, config: &Config, main: &Entry) -> Vec<Entry> {
    let values = group_values(contents, DESKTOP_GROUP);
    values
        .get("Actions")
        .into_iter()
        .flat_map(|actions| actions.split(';'))
        .filter(|id| !id.is_empty())
        .take(64)
        .filter_map(|id| {
            let action = group_values(contents, &format!("[Desktop Action {id}]"));
            let label = unescape_value(localized(&action, "Name", &config.locale)?);
            let icon_name = action
                .get("Icon")
                .map(|value| unescape_value(value))
                .or_else(|| main.icon_name.clone());
            let mut argv =
                expand_exec(action.get("Exec")?, &main.name, icon_name.as_deref(), path).ok()?;
            if argv.is_empty() || argv[0].is_empty() || label.is_empty() {
                return None;
            }
            if values.get("Terminal") == Some(&"true") {
                argv.splice(0..0, config.terminal.clone());
            }
            Some(Entry {
                id: 0,
                desktop_id: format!("{}::{id}", main.desktop_id),
                name: format!("{} — {label}", main.name),
                description: "Desktop action".to_owned(),
                icon: None,
                icon_name,
                argv,
                action: true,
                keywords: main.keywords.clone(),
                window_class: main.window_class.clone(),
                executable: main.executable.clone(),
                working_dir: main.working_dir.clone(),
            })
        })
        .collect()
}

fn read_small_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > 1_048_576 {
        return Err(std::io::Error::other(
            "application index input must be a bounded regular file",
        ));
    }
    let mut content = String::new();
    file.by_ref().take(1_048_577).read_to_string(&mut content)?;
    if content.len() > 1_048_576 {
        return Err(std::io::Error::other(
            "application index input exceeds its limit",
        ));
    }
    Ok(content)
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

fn icon_roots(dirs: &[PathBuf]) -> Vec<PathBuf> {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".icons"))
        .into_iter()
        .chain(dirs.iter().map(|dir| dir.join("icons")))
        .collect()
}

fn valid_theme(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', '\0'])
}

fn theme_order(
    name: &str,
    roots: &[PathBuf],
    visited: &mut HashSet<String>,
    order: &mut Vec<String>,
) {
    if !valid_theme(name) || visited.len() >= 64 || !visited.insert(name.to_owned()) {
        return;
    }
    order.push(name.to_owned());
    if let Some(contents) = roots
        .iter()
        .find_map(|root| read_small_file(&root.join(name).join("index.theme")).ok())
    {
        let values = group_values(&contents, "[Icon Theme]");
        for parent in values
            .get("Inherits")
            .into_iter()
            .flat_map(|parents| parents.split(','))
            .map(str::trim)
        {
            theme_order(parent, roots, visited, order);
        }
    }
}

fn load_icons(
    dirs: &[PathBuf],
    theme: &str,
    wanted: &HashSet<&str>,
    cancel: &CancelToken,
) -> HashMap<String, PathBuf> {
    let mut icons = HashMap::new();
    if wanted.is_empty() {
        return icons;
    }
    let roots = icon_roots(dirs);
    let mut themes = Vec::new();
    let mut seen = HashSet::new();
    theme_order(theme, &roots, &mut seen, &mut themes);
    theme_order("hicolor", &roots, &mut seen, &mut themes);
    if theme == "hicolor" {
        theme_order("Adwaita", &roots, &mut seen, &mut themes);
    }
    let mut visited = HashSet::new();
    for theme in themes {
        for root in &roots {
            let root = root.join(&theme);
            let declared = read_small_file(&root.join("index.theme"))
                .ok()
                .map(|contents| {
                    group_values(&contents, "[Icon Theme]")
                        .get("Directories")
                        .map(|dirs| {
                            dirs.split(',')
                                .take(4096)
                                .map(str::trim)
                                .filter(|dir| {
                                    Path::new(dir)
                                        .components()
                                        .all(|part| matches!(part, std::path::Component::Normal(_)))
                                })
                                .map(|dir| root.join(dir))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            if declared.is_empty() {
                collect_icons(&root, wanted, &mut icons, &mut visited, cancel);
            } else {
                let mut directories = declared;
                directories.sort_by_key(|directory| {
                    let name = directory
                        .strip_prefix(&root)
                        .unwrap_or(directory)
                        .to_string_lossy();
                    if name.contains("scalable") {
                        0
                    } else {
                        name.split('x')
                            .next()
                            .and_then(|size| size.parse::<u32>().ok())
                            .map_or(1000, |size| size.abs_diff(64) + 1)
                    }
                });
                for directory in directories {
                    collect_icons(&directory, wanted, &mut icons, &mut visited, cancel);
                }
            }
        }
    }
    for dir in dirs {
        collect_icons(
            &dir.join("pixmaps"),
            wanted,
            &mut icons,
            &mut visited,
            cancel,
        );
    }
    icons
}

fn collect_icons(
    dir: &Path,
    wanted: &HashSet<&str>,
    icons: &mut HashMap<String, PathBuf>,
    visited: &mut HashSet<PathBuf>,
    cancel: &CancelToken,
) {
    if cancel.is_cancelled() || visited.len() >= 4096 || icons.len() == wanted.len() {
        return;
    }
    let Ok(canonical) = dir.canonicalize() else {
        return;
    };
    if !visited.insert(canonical) {
        return;
    }
    let Ok(files) = fs::read_dir(dir) else { return };
    for file in files.flatten() {
        if cancel.is_cancelled() {
            return;
        }
        let path = file.path();
        if path.is_dir() {
            collect_icons(&path, wanted, icons, visited, cancel);
            continue;
        }
        if !path
            .extension()
            .is_some_and(|extension| extension == "png" || extension == "svg")
        {
            continue;
        }
        if let Some(name) = path.file_stem().and_then(|name| name.to_str())
            && wanted.contains(name)
            && path
                .metadata()
                .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= 16 * 1024 * 1024)
        {
            icons.entry(name.to_owned()).or_insert(path);
        }
    }
}

#[cfg(test)]
pub(crate) fn search(entries: &[Entry], query: &str, limit: usize) -> Vec<Match> {
    search_ranked(
        entries,
        query,
        limit,
        &std::collections::BTreeMap::new(),
        &CancelToken::default(),
    )
}

pub(crate) fn search_ranked(
    entries: &[Entry],
    query: &str,
    limit: usize,
    ranking: &std::collections::BTreeMap<String, i64>,
    cancel: &CancelToken,
) -> Vec<Match> {
    let matcher = SkimMatcherV2::default().ignore_case();
    let query = query.trim();
    let mut matches: Vec<Match> = Vec::with_capacity(limit.min(50));
    for (index, entry) in entries.iter().enumerate() {
        if cancel.is_cancelled() {
            return Vec::new();
        }
        let score = if query.is_empty() {
            Some(0)
        } else {
            std::iter::once(&entry.name)
                .chain(&entry.keywords)
                .filter_map(|candidate| matcher.fuzzy_match(candidate, query))
                .max()
        };
        let Some(score) = score else {
            continue;
        };
        let matched = Match {
            index,
            score: score
                .saturating_add(ranking.get(&entry.desktop_id).copied().unwrap_or_default()),
        };
        let position = matches
            .binary_search_by(|current| {
                matched
                    .score
                    .cmp(&current.score)
                    .then(current.index.cmp(&matched.index))
            })
            .unwrap_or_else(|position| position);
        if position < limit.min(50) {
            matches.insert(position, matched);
            matches.truncate(limit.min(50));
        }
    }
    matches
}

pub(crate) fn watch_roots() -> Vec<PathBuf> {
    let dirs = data_dirs();
    dirs.iter()
        .map(|dir| dir.join("applications"))
        .chain(icon_roots(&dirs))
        .collect()
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

    #[test]
    fn locale_fallback_and_desktop_actions_keep_distinct_argument_intents() {
        let mut config = config();
        config.locale = "fr_CA.UTF-8@latin".to_owned();
        let contents = "[Desktop Entry]\nType=Application\nName=Editor\nName[fr]=Éditeur\nName[fr_CA]=Québec\nComment[fr]=Commentaire\nExec=editor\nActions=New;Missing;\n[Desktop Action New]\nName=New\nName[fr]=Nouveau\nExec=editor --new %c\n";
        let mut main = parse_entry(contents, Path::new("editor.desktop"), &config, &[]).unwrap();
        main.desktop_id = "editor.desktop".to_owned();
        assert_eq!(main.name, "Québec");
        assert_eq!(main.description, "Commentaire");
        let actions = parse_actions(contents, Path::new("editor.desktop"), &config, &main);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].desktop_id, "editor.desktop::New");
        assert_eq!(actions[0].name, "Québec — Nouveau");
        assert_eq!(actions[0].argv, ["editor", "--new", "Québec"]);
        assert_eq!(
            locale_names("sr_RS.UTF-8@latin"),
            ["sr_RS@latin", "sr_RS", "sr@latin", "sr"]
        );
    }

    #[test]
    fn icon_inheritance_respects_child_priority_and_stops_cycles() {
        let root = std::env::temp_dir().join(format!(
            "oxibar-icon-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for theme in ["child", "parent"] {
            fs::create_dir_all(root.join("icons").join(theme).join("scalable/apps")).unwrap();
        }
        fs::write(
            root.join("icons/child/index.theme"),
            "[Icon Theme]\nDirectories=scalable/apps\nInherits=parent\n",
        )
        .unwrap();
        fs::write(
            root.join("icons/parent/index.theme"),
            "[Icon Theme]\nDirectories=scalable/apps\nInherits=child\n",
        )
        .unwrap();
        let parent = root.join("icons/parent/scalable/apps/oxibar-fixture.svg");
        fs::write(&parent, "<svg/>").unwrap();
        let wanted = HashSet::from(["oxibar-fixture"]);
        assert_eq!(
            load_icons(
                std::slice::from_ref(&root),
                "child",
                &wanted,
                &CancelToken::default()
            )["oxibar-fixture"],
            parent
        );
        let child = root.join("icons/child/scalable/apps/oxibar-fixture.svg");
        fs::write(&child, "<svg/>").unwrap();
        assert_eq!(
            load_icons(
                std::slice::from_ref(&root),
                "child",
                &wanted,
                &CancelToken::default()
            )["oxibar-fixture"],
            child
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ranked_top_k_is_bounded_and_cooperative_cancellation_is_empty() {
        let entries: Vec<_> = (0..10_000)
            .map(|index| {
                let mut entry = entry("").unwrap();
                entry.desktop_id = format!("{index}.desktop");
                entry
            })
            .collect();
        let ranking = std::collections::BTreeMap::from([("9999.desktop".to_owned(), 1_000_000)]);
        let results = search_ranked(&entries, "", 7, &ranking, &CancelToken::default());
        assert_eq!(results.len(), 7);
        assert_eq!(results[0].index, 9999);
        let cancel = CancelToken::default();
        cancel.cancel();
        assert!(search_ranked(&entries, "Text", 7, &ranking, &cancel).is_empty());
        assert!(load_entries_from_cancel(&[], &config(), &[], &cancel).is_empty());
    }
}
