use iced::{
    Event,
    event::Status,
    keyboard::{self, Key, Modifiers, key::Named},
};
use iced_layershell::reexport::IcedId;
use oxibar_plugin_api::{ApplyMode, SettingDescriptor, SettingKind};
use oxiced::widgets::oxi_navigation::{Action as Navigation, Bindings};
use toml::{Table, Value};

use crate::{app::OxiBar, messages::Message};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Context {
    Any,
    Forms,
    Overview,
    Settings,
    Launcher,
    OxiPaste,
    Controls,
    Session,
    Desktop,
    Modal,
    Dropdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Close,
    Focus(bool),
    Navigate(Navigation),
    Search,
    Category(usize),
    Control(usize),
    Next(bool),
    Mode(bool),
    Activate,
    Window(i32),
    Move(i32, i32),
    Monitor(i32, i32),
    First,
    Last,
    Nudge(i32, i32, bool),
}

struct Definition {
    path: String,
    label: String,
    default: String,
    context: Context,
    command: Command,
}

fn definitions() -> Vec<Definition> {
    use Context::*;
    let mut result = Vec::new();
    let mut add = |path: &str, label: &str, default: &str, context, command| {
        result.push(Definition {
            path: format!("shortcuts.{path}"),
            label: label.into(),
            default: default.into(),
            context,
            command,
        });
    };
    add(
        "close",
        "Close current surface / cancel edit",
        "Escape",
        Any,
        Command::Close,
    );
    add(
        "focus_next",
        "Next control / form field",
        "Tab",
        Forms,
        Command::Focus(true),
    );
    add(
        "focus_previous",
        "Previous control / form field",
        "Shift+Tab",
        Forms,
        Command::Focus(false),
    );
    for (name, label, keys, action) in [
        ("next", "Next control", "ArrowDown, j", Navigation::Next),
        (
            "previous",
            "Previous control",
            "ArrowUp, k",
            Navigation::Previous,
        ),
        (
            "left",
            "Previous control / decrease value",
            "ArrowLeft, h",
            Navigation::Left,
        ),
        (
            "right",
            "Next control / increase value",
            "ArrowRight, l",
            Navigation::Right,
        ),
        ("first", "First control / option", "Home", Navigation::First),
        ("last", "Last control / option", "End", Navigation::Last),
        (
            "activate",
            "Activate control / choose option",
            "Enter, Space",
            Navigation::Activate,
        ),
        (
            "context_menu",
            "Open focused item's context menu",
            "Shift+F10, Menu",
            Navigation::ContextMenu,
        ),
    ] {
        add(
            &format!("navigation.{name}"),
            label,
            keys,
            Forms,
            Command::Navigate(action),
        );
    }
    add(
        "settings.search",
        "Settings: focus search",
        "Ctrl+f",
        Settings,
        Command::Search,
    );
    for (i, category) in crate::settings::Category::ALL.iter().enumerate() {
        let key = if i == 10 {
            "Alt+s".into()
        } else {
            format!("Alt+{}", (i + 1) % 10)
        };
        add(
            &format!("settings.category_{}", i + 1),
            &format!("Settings: {category}"),
            &key,
            Settings,
            Command::Category(i),
        );
    }
    for i in 0..9 {
        add(
            &format!("controls.section_{}", i + 1),
            &format!("Control center: section {}", i + 1),
            &format!("Alt+{}", i + 1),
            Controls,
            Command::Control(i),
        );
    }
    add(
        "launcher.next",
        "Launcher: next result",
        "ArrowDown, Tab",
        Launcher,
        Command::Next(true),
    );
    add(
        "launcher.previous",
        "Launcher: previous result",
        "ArrowUp, Shift+Tab",
        Launcher,
        Command::Next(false),
    );
    add(
        "launcher.next_mode",
        "Launcher: next mode",
        "Ctrl+Tab",
        Launcher,
        Command::Mode(true),
    );
    add(
        "launcher.previous_mode",
        "Launcher: previous mode",
        "Ctrl+Shift+Tab",
        Launcher,
        Command::Mode(false),
    );
    add(
        "launcher.activate",
        "Launcher: open result",
        "Enter",
        Launcher,
        Command::Activate,
    );
    add(
        "session.next",
        "Session: extra next-control binding",
        "",
        Session,
        Command::Next(true),
    );
    add(
        "session.previous",
        "Session: extra previous-control binding",
        "",
        Session,
        Command::Next(false),
    );
    add(
        "session.activate",
        "Session: extra activation binding",
        "",
        Session,
        Command::Activate,
    );
    add(
        "overview.next_window",
        "Overview: next window on monitor",
        "Tab",
        Overview,
        Command::Window(1),
    );
    add(
        "overview.previous_window",
        "Overview: previous window on monitor",
        "Shift+Tab",
        Overview,
        Command::Window(-1),
    );
    add(
        "overview.activate",
        "Overview: open selected window",
        "Enter",
        Overview,
        Command::Activate,
    );
    add(
        "overview.first",
        "Overview: first window",
        "Home",
        Overview,
        Command::First,
    );
    add(
        "overview.last",
        "Overview: last window",
        "End",
        Overview,
        Command::Last,
    );
    for (direction, key, vim, x, y) in [
        ("left", "ArrowLeft", "h", -1, 0),
        ("down", "ArrowDown", "j", 0, 1),
        ("up", "ArrowUp", "k", 0, -1),
        ("right", "ArrowRight", "l", 1, 0),
    ] {
        add(
            &format!("overview.window_{direction}"),
            &format!("Overview: select window {direction}"),
            key,
            Overview,
            Command::Move(x, y),
        );
        add(
            &format!("overview.monitor_{direction}"),
            &format!("Overview: monitor {direction}"),
            vim,
            Overview,
            Command::Monitor(x, y),
        );
        add(
            &format!("desktop.move_{direction}"),
            &format!("Desktop editor: move {direction}"),
            key,
            Desktop,
            Command::Nudge(x, y, false),
        );
        add(
            &format!("desktop.resize_{direction}"),
            &format!("Desktop editor: resize {direction}"),
            &format!("Shift+{key}"),
            Desktop,
            Command::Nudge(x, y, true),
        );
    }
    result
}

pub(crate) fn schema() -> Vec<SettingDescriptor> {
    definitions().into_iter().map(|d| {
        SettingDescriptor::new(&d.path, &d.label, SettingKind::Text, Some(Value::String(d.default)), ApplyMode::Live)
            .description("Key or comma-separated alternatives; modifiers: Ctrl, Alt, Shift, Super. Empty disables. Applies inside Oxibar; global shell toggles are compositor bindings.")
    }).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Chord {
    key: Key,
    modifiers: Modifiers,
}

fn parse(value: &str) -> Result<Vec<Chord>, String> {
    if value.trim().is_empty() {
        return Ok(Vec::new());
    }
    value
        .split(',')
        .map(|value| {
            let parts: Vec<_> = value.trim().split('+').map(str::trim).collect();
            let mut modifiers = Modifiers::empty();
            for part in &parts[..parts.len() - 1] {
                let modifier = match part.to_ascii_lowercase().as_str() {
                    "ctrl" | "control" => Modifiers::CTRL,
                    "alt" => Modifiers::ALT,
                    "shift" => Modifiers::SHIFT,
                    "super" | "logo" | "meta" => Modifiers::LOGO,
                    _ => return Err(format!("unknown modifier `{part}`")),
                };
                if modifiers.contains(modifier) {
                    return Err(format!("duplicate modifier `{part}`"));
                }
                modifiers |= modifier;
            }
            let name = parts
                .last()
                .expect("split has an item")
                .to_ascii_lowercase();
            let key = match name.as_str() {
                "escape" | "esc" => Key::Named(Named::Escape),
                "enter" | "return" => Key::Named(Named::Enter),
                "tab" => Key::Named(Named::Tab),
                "space" => Key::Named(Named::Space),
                "arrowleft" | "left" => Key::Named(Named::ArrowLeft),
                "arrowright" | "right" => Key::Named(Named::ArrowRight),
                "arrowup" | "up" => Key::Named(Named::ArrowUp),
                "arrowdown" | "down" => Key::Named(Named::ArrowDown),
                "home" => Key::Named(Named::Home),
                "end" => Key::Named(Named::End),
                "pageup" => Key::Named(Named::PageUp),
                "pagedown" => Key::Named(Named::PageDown),
                "backspace" => Key::Named(Named::Backspace),
                "delete" => Key::Named(Named::Delete),
                "menu" | "contextmenu" => Key::Named(Named::ContextMenu),
                "plus" => Key::Character("+".into()),
                "comma" => Key::Character(",".into()),
                "f1" => Key::Named(Named::F1),
                "f2" => Key::Named(Named::F2),
                "f3" => Key::Named(Named::F3),
                "f4" => Key::Named(Named::F4),
                "f5" => Key::Named(Named::F5),
                "f6" => Key::Named(Named::F6),
                "f7" => Key::Named(Named::F7),
                "f8" => Key::Named(Named::F8),
                "f9" => Key::Named(Named::F9),
                "f10" => Key::Named(Named::F10),
                "f11" => Key::Named(Named::F11),
                "f12" => Key::Named(Named::F12),
                _ if name.chars().count() == 1 => Key::Character(name.into()),
                _ => return Err(format!("unknown key `{name}`")),
            };
            Ok(Chord { key, modifiers })
        })
        .collect()
}

pub(crate) fn validate_value(value: &str) -> Result<(), String> {
    parse(value).map(|_| ())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Binding {
    path: String,
    chord: Chord,
    context: Context,
    command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    bindings: Vec<Binding>,
}

fn applies(binding: Context, current: Context) -> bool {
    binding == Context::Any
        || binding == current
        || binding == Context::Forms
            && matches!(
                current,
                Context::Settings
                    | Context::Controls
                    | Context::Modal
                    | Context::OxiPaste
                    | Context::Dropdown
            )
}

impl Config {
    pub(crate) fn session_navigation(&self) -> Bindings {
        let mut bindings: Vec<_> = self
            .bindings
            .iter()
            .filter_map(|binding| {
                if binding.context != Context::Session {
                    return None;
                }
                let action = match binding.command {
                    Command::Next(true) => Navigation::Next,
                    Command::Next(false) => Navigation::Previous,
                    Command::Activate => Navigation::Activate,
                    _ => return None,
                };
                Some((binding.chord.key.clone(), binding.chord.modifiers, action))
            })
            .collect();
        bindings.extend(self.navigation().0);
        Bindings(bindings)
    }

    pub(crate) fn navigation(&self) -> Bindings {
        Bindings(
            self.bindings
                .iter()
                .filter_map(|binding| {
                    let action = match binding.command {
                        Command::Focus(true) => Navigation::FocusNext,
                        Command::Focus(false) => Navigation::FocusPrevious,
                        Command::Navigate(action) => action,
                        Command::Close => Navigation::Cancel,
                        _ => return None,
                    };
                    Some((binding.chord.key.clone(), binding.chord.modifiers, action))
                })
                .collect(),
        )
    }

    pub(crate) fn read(table: &Table) -> Result<Self, String> {
        let mut bindings: Vec<Binding> = Vec::new();
        for d in definitions() {
            let value = crate::config::store::get_path(table, &d.path)
                .and_then(Value::as_str)
                .unwrap_or(&d.default);
            for chord in parse(value).map_err(|error| format!("{}: {error}", d.path))? {
                if let Some(other) = bindings.iter().find(|b| {
                    b.chord == chord
                        && (applies(b.context, d.context) || applies(d.context, b.context))
                }) {
                    if other.path == d.path {
                        continue;
                    }
                    return Err(format!(
                        "{}: shortcut conflicts with {}",
                        d.path, other.path
                    ));
                }
                bindings.push(Binding {
                    path: d.path.clone(),
                    chord,
                    context: d.context,
                    command: d.command,
                });
            }
        }
        Ok(Self { bindings })
    }

    fn command(
        &self,
        context: Context,
        key: Key,
        modifiers: Modifiers,
        status: Status,
    ) -> Option<Command> {
        let key = match key {
            Key::Character(value) => Key::Character(value.to_lowercase().into()),
            key => key,
        };
        self.bindings
            .iter()
            .find(|b| {
                applies(b.context, context) && b.chord.key == key && b.chord.modifiers == modifiers
            })
            .map(|b| b.command)
            .filter(|command| {
                status == Status::Ignored
                    || !(matches!(
                        command,
                        Command::Focus(_) | Command::Navigate(_) | Command::Nudge(..)
                    ) || matches!(command, Command::Close)
                        && (applies(Context::Forms, context)
                            || matches!(context, Context::Launcher | Context::Session))
                        || matches!(context, Context::Launcher | Context::Session)
                            && matches!(command, Command::Activate | Command::Next(_)))
            })
    }
}

pub(crate) fn event(event: Event, status: Status, id: IcedId) -> Option<Message> {
    match event {
        Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
            Some(Message::ShortcutKey(id, key, modifiers, status))
        }
        Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left))
            if status == Status::Ignored =>
        {
            Some(Message::SettingsDragRelease(id))
        }
        _ => None,
    }
}

impl OxiBar {
    pub(crate) fn shortcut_label(&self, name: &str) -> &str {
        crate::config::store::get_path(
            &self.config.snapshot.effective,
            &format!("shortcuts.{name}"),
        )
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("disabled")
    }

    pub(crate) fn shortcut(
        &self,
        id: IcedId,
        key: Key,
        modifiers: Modifiers,
        status: Status,
    ) -> Option<Message> {
        use Command::*;
        let context = if self
            .overview
            .surfaces
            .iter()
            .any(|(output, s)| s.id == id && Some(output) == self.overview.active.as_ref())
        {
            Context::Overview
        } else if self.settings.window_id == Some(id) {
            Context::Settings
        } else if self.launcher.window_id == Some(id) {
            Context::Launcher
        } else if self.shell.oxipaste_window == Some(id) {
            Context::OxiPaste
        } else if self.control_center.window_id == Some(id) {
            Context::Controls
        } else if self.shell.session_window == Some(id) {
            Context::Session
        } else if self.modal_window_id == Some(id) {
            Context::Modal
        } else if (self.popup_open && self.shell.popup_bar == Some(id))
            || self.panel_window_id == Some(id)
        {
            Context::Dropdown
        } else if self.desktop.draft.is_some() {
            Context::Desktop
        } else {
            return None;
        };
        let command = self
            .config
            .snapshot
            .host
            .shortcuts
            .command(context, key, modifiers, status)?;
        Some(match (context, command) {
            (Context::Overview, command) => Message::OverviewKey(
                id,
                match command {
                    Close => crate::overview::Action::Close,
                    Activate => crate::overview::Action::ActivateSelected,
                    Window(delta) => crate::overview::Action::Cycle(delta),
                    Move(x, y) => crate::overview::Action::Move(x, y),
                    Monitor(x, y) => crate::overview::Action::MonitorDirection(x, y),
                    First => crate::overview::Action::First,
                    Last => crate::overview::Action::Last,
                    _ => return None,
                },
            ),
            (Context::Settings, Close) => Message::SettingsEscape(id),
            (Context::Settings, Focus(forward)) => Message::SettingsFocus(id, forward),
            (Context::Settings, Search) => Message::SettingsSearchFocus(id),
            (Context::Settings, Category(index)) => {
                Message::SettingsCategory(id, *crate::settings::Category::ALL.get(index)?)
            }
            (Context::Controls, Close) => {
                Message::ControlCenterKey(id, crate::control_center::Action::Close)
            }
            (Context::Controls, Focus(forward)) => Message::ControlCenterFocus(id, forward),
            (Context::Controls, Control(index)) => {
                Message::ControlCenterKey(id, crate::control_center::Action::SelectIndex(index))
            }
            (Context::Launcher, command) => Message::LauncherKey(
                id,
                match command {
                    Close => crate::launcher::Action::Close,
                    Next(forward) => crate::launcher::Action::MoveFocus(forward),
                    Mode(forward) => crate::launcher::Action::CycleMode(forward),
                    Activate => crate::launcher::Action::LaunchFocused,
                    _ => return None,
                },
            ),
            (Context::Session, command) => Message::SessionKey(
                id,
                match command {
                    Close => crate::session_menu::Action::Close,
                    Next(forward) => {
                        crate::session_menu::Action::Move(if forward { 1 } else { -1 })
                    }
                    Activate => crate::session_menu::Action::LaunchFocused,
                    _ => return None,
                },
            ),
            (Context::Modal, Close) => Message::ModalEscape(id),
            (Context::OxiPaste, Close) => Message::OxiPasteEscape(id),
            (Context::OxiPaste, Focus(forward)) => Message::OxiPasteFocus(id, forward),
            (Context::Modal, Focus(forward)) => Message::ModalFocus(id, forward),
            (Context::Dropdown, Close) => Message::DropdownEscape(id),
            (Context::Desktop, Close) => Message::Desktop(crate::desktop::Action::Escape(id)),
            (Context::Desktop, Nudge(x, y, resize)) => {
                Message::Desktop(crate::desktop::Action::Nudge(id, x, y, resize))
            }
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_are_valid_and_keys_are_scoped() {
        let config = Config::read(&Table::new()).unwrap();
        assert_eq!(
            config.command(
                Context::Overview,
                Key::Named(Named::Tab),
                Modifiers::empty(),
                Status::Ignored
            ),
            Some(Command::Window(1))
        );
        assert_eq!(
            config.command(
                Context::Overview,
                Key::Character("h".into()),
                Modifiers::empty(),
                Status::Ignored
            ),
            Some(Command::Monitor(-1, 0))
        );
        assert_eq!(
            config.command(
                Context::Settings,
                Key::Character("h".into()),
                Modifiers::empty(),
                Status::Ignored
            ),
            Some(Command::Navigate(Navigation::Left))
        );
        assert_eq!(
            config.command(
                Context::Launcher,
                Key::Named(Named::Enter),
                Modifiers::empty(),
                Status::Captured
            ),
            None
        );
    }
    #[test]
    fn remaps_disable_and_conflicts_are_validated() {
        let mut table = Table::new();
        crate::config::store::set_path(
            &mut table,
            "shortcuts.overview.monitor_left",
            Value::String("Ctrl+Left".into()),
        )
        .unwrap();
        let config = Config::read(&table).unwrap();
        assert_eq!(
            config.command(
                Context::Overview,
                Key::Named(Named::ArrowLeft),
                Modifiers::CTRL,
                Status::Ignored
            ),
            Some(Command::Monitor(-1, 0))
        );
        crate::config::store::set_path(
            &mut table,
            "shortcuts.overview.monitor_left",
            Value::String("Tab".into()),
        )
        .unwrap();
        assert!(Config::read(&table).unwrap_err().contains("conflicts"));
        assert!(parse("Ctrl+Unknown").is_err());
        assert!(parse("").unwrap().is_empty());
        assert_eq!(parse("Super+Shift+F12, Ctrl+Plus").unwrap().len(), 2);
    }

    #[test]
    fn shared_navigation_reaches_plugins_and_nested_capture_stays_local() {
        let mut table = Table::new();
        crate::config::store::set_path(
            &mut table,
            "shortcuts.navigation.next",
            Value::String("Ctrl+n".into()),
        )
        .unwrap();
        let config = Config::read(&table).unwrap();
        let policy = config.navigation();
        assert!(policy.0.contains(&(
            Key::Character("n".into()),
            Modifiers::CTRL,
            Navigation::Next
        )));
        assert!(
            !policy
                .0
                .iter()
                .any(|(key, _, _)| key == &Key::Character("j".into()))
        );
        for context in [
            Context::Modal,
            Context::Dropdown,
            Context::Controls,
            Context::OxiPaste,
        ] {
            assert_eq!(
                config.command(
                    context,
                    Key::Named(Named::Tab),
                    Modifiers::empty(),
                    Status::Ignored
                ),
                Some(Command::Focus(true))
            );
            assert_eq!(
                config.command(
                    context,
                    Key::Named(Named::Escape),
                    Modifiers::empty(),
                    Status::Captured
                ),
                None
            );
            assert_eq!(
                config.command(
                    context,
                    Key::Named(Named::Escape),
                    Modifiers::empty(),
                    Status::Ignored
                ),
                Some(Command::Close)
            );
        }
        assert_eq!(
            parse("Menu").unwrap()[0].key,
            Key::Named(Named::ContextMenu)
        );
        crate::config::store::set_path(
            &mut table,
            "shortcuts.navigation.next",
            Value::String("Ctrl+f".into()),
        )
        .unwrap();
        assert!(
            Config::read(&table)
                .unwrap_err()
                .contains("conflicts with shortcuts.navigation.next")
        );
    }

    #[test]
    fn session_inherits_global_remaps_and_keeps_explicit_aliases() {
        let mut table = Table::new();
        crate::config::store::set_path(
            &mut table,
            "shortcuts.navigation.activate",
            Value::String("x".into()),
        )
        .unwrap();
        let config = Config::read(&table).unwrap();
        assert_eq!(config.session_navigation(), config.navigation());
        assert!(
            !config
                .session_navigation()
                .0
                .iter()
                .any(|(key, _, _)| { key == &Key::Named(Named::Enter) })
        );
        crate::config::store::set_path(
            &mut table,
            "shortcuts.session.activate",
            Value::String("Ctrl+Enter".into()),
        )
        .unwrap();
        let policy = Config::read(&table).unwrap().session_navigation();
        assert_eq!(
            policy.0[0],
            (
                Key::Named(Named::Enter),
                Modifiers::CTRL,
                Navigation::Activate
            )
        );
        assert!(policy.0.contains(&(
            Key::Character("x".into()),
            Modifiers::empty(),
            Navigation::Activate
        )));
    }
}
