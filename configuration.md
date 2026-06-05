# Oxibar configuration

Oxibar reads its configuration from `$XDG_CONFIG_HOME/oxibar/config.toml`
(usually `~/.config/oxibar/config.toml`). The file is plain TOML. If it is
missing, oxibar starts with all defaults and no plugins enabled.

Plugins are loaded from `$XDG_CONFIG_HOME/oxibar/plugins/`. Drop the built
`.so` files there (e.g. `libclock.so`, `libworkspaces.so`) and list the file
names you want enabled in the top-level `plugins` array.

## Top-level keys

| Key       | Type            | Default | Description                                               |
| --------- | --------------- | ------- | --------------------------------------------------------- |
| `plugins` | array of string | `[]`    | File names (not paths) of plugin dylibs to load and run.  |

Example:

```toml
plugins = ["libclock.so", "libworkspaces.so", "libbluetooth.so", "libnotifications.so"]
```

A plugin file present on disk but not listed here is ignored. Unknown names
are skipped with a warning. Plugins whose `abi_version()` does not match the
host's are also skipped with a warning — rebuild them against the current
`oxibar-plugin-api`.

## `[bar]` — bar surface

| Key           | Type            | Default          | Description                                                                                                                                                          |
| ------------- | --------------- | ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `transparent` | bool            | `false`          | When `true`, the bar's container background is fully transparent. Plugins still render their own backgrounds. Useful with a wallpaper or a compositor blur effect. |
| `font`        | string          | `"Adwaita Sans"` | Default font *family name* used across the entire bar. Oxibar resolves it through `fc-match`, loads the matched font file into iced/cosmic-text, then uses the matched family name as iced's `default_font`. Plugins that don't override their own font inherit it. |
| `font_file`   | string          | _unset_          | Absolute path to a `.ttf` / `.otf` file. When set, this explicit file is loaded instead of the `fc-match` result. The `font` value must still match the family name inside the file. |
| `start`       | array of string | `[]`             | Plugin names to render in the **start** (left) section, in the given order.                                                                                          |
| `center`      | array of string | `[]`             | Plugin names to render in the **center** section, in the given order.                                                                                                |
| `end`         | array of string | `[]`             | Plugin names to render in the **end** (right) section, in the given order.                                                                                           |
| `popup_sizes` | table of arrays  | `{}`             | Optional popup size overrides by plugin name. Each value is `[width, height]`; matching is case-insensitive.                                                         |

The bar is divided into three equal-width sections — start, center, end —
each aligning its widgets to its own edge. A plugin is rendered only if it
appears in one of the three arrays; the name used is the one the plugin
itself reports via its `name()` entry point (typically the same as the
config table name, e.g. `"clock"`, `"workspaces"`). Matching is
**case-insensitive**, so `"clock"`, `"Clock"` and `"CLOCK"` all refer to the
same plugin. Names that don't match a loaded plugin are skipped with a
`tracing::warn!`.

As a backward-compat shortcut, if **all three** of `start`, `center` and
`end` are omitted (or empty), every loaded plugin is placed in `start` in
the same order as the top-level `plugins = [...]` array — the old single-row
layout, now deterministic.

Popup size precedence is: `[bar.popup_sizes]` override, plugin metadata,
then the host default `320 × 300`. `audio`, `bluetooth`, and `network`
declare `460 × 420` via metadata.

Example:

```toml
[bar]
transparent = true
font      = "Adwaita Sans"
font_file = "/run/current-system/sw/share/fonts/Adwaita/AdwaitaSans-Regular.ttf"
start  = ["workspaces"]
center = ["clock"]
end    = ["bluetooth", "notifications"]

[bar.popup_sizes]
clock = [340, 320]
```

### Not yet configurable

These are currently hardcoded in `src/main.rs` and will move to `[bar]` in a
later pass:

- Window size — `3440 × 31`. Will be derived from the active wayland output
  or a `[bar] width` / `[bar] height` knob.
- Anchor — `Top`. Will become `[bar] anchor = ["top"]` (array because layer
  shell anchors are bitflags).
- Layer — `Top`. Will become `[bar] layer = "background" | "bottom" | "top" | "overlay"`.
- Margins — `(0, 0, 0, 0)`. Will become `[bar] margin = [t, r, b, l]`.
- Exclusive zone — `31`. Will become `[bar] exclusive_zone = N`.
- Keyboard interactivity — `None` for the main bar/popup layer; modal dialogs use `Exclusive`. Will become `[bar] keyboard = "none" | "on-demand" | "exclusive"`.
- Scale factor — `1.0`. Will become `[bar] scale = ...`.

## Plugin configuration

Each plugin reads its own block. The canonical location is a top-level table
named after the plugin (`[clock]`, `[workspaces]`, ...). The host passes the
entire config document to every plugin's `model()` entry point; the plugin
extracts what it needs.

### `[clock]` — clock plugin

Renders the current local time as a transparent button. Click toggles an
internal `calendar_open` flag and opens a popup calendar.

| Key            | Type            | Default   | Description                                                                                                                                                                              |
| -------------- | --------------- | --------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `format`       | string          | `"%H:%M"` | `chrono` strftime spec. See <https://docs.rs/chrono/latest/chrono/format/strftime/index.html>. Invalid specs are caught at render time and fall back to RFC3339 with a `tracing::warn!`. |
| `tick_seconds` | integer (`> 0`) | `60`      | How often the clock refreshes, in whole seconds. Use `1` if your `format` includes seconds (`%S`). Read once at startup; changes require a restart.                                      |
| `font_size`    | number (`> 0`)  | `14.0`    | Time-label font size in iced units. Accepts integers or floats.                                                                                                                          |
| `bold`         | bool            | `false`   | When `true`, the time label is rendered with a bold font weight. Falls back to a sans-serif family when bold is enabled.                                                                 |
| `calendar_command` | string | _unset_ | Shell command run when clicking a calendar day. Supports `{date}`, `{year}`, `{month}`, `{day}` placeholders, e.g. `gnome-calendar --date {date}`. |
| `calendar_app` | string | _unset_ | Convenience integration. `"thunderbird"` maps to `thunderbird --calendar`; Thunderbird does not provide reliable date-focused CLI navigation, so use `calendar_command` for custom behavior. |

Example:

```toml
[clock]
format       = "%a %H:%M"
tick_seconds = 60
font_size    = 16
bold         = true
calendar_command = "gnome-calendar --date {date}"
```

For Thunderbird:

```toml
[clock]
calendar_app = "thunderbird"
```

For Nextcloud/CalDAV event sync, configure an HTTPS CalDAV collection URL and put the app password in an environment variable. Oxibar rejects non-HTTPS URLs, forces curl to use HTTPS protocols, verifies TLS certificates by default, and passes credentials through a temporary `0600` curl config instead of command-line arguments. Sync starts at current month start, expands common recurring `VEVENT` rules, and marks event days in the calendar popup. Hover an event day to see summary, location, and description.

```toml
[clock.caldav]
url = "https://cloud.example.com/remote.php/dav/calendars/alice/personal/"
username = "alice"
password_env = "OXIBAR_CALDAV_PASSWORD"
refresh_minutes = 15
days_ahead = 30
```

### `[workspaces]` — Hyprland workspaces plugin

Renders one button per Hyprland workspace, ordered by workspace id. The
active workspace is highlighted. Clicking a workspace dispatches to it.

This plugin currently has no configuration keys. It connects to Hyprland via
`$HYPRLAND_INSTANCE_SIGNATURE` (set by Hyprland itself); if that environment
variable is missing it logs an error to its `errors()` channel on startup.

The host-level `launch(focused_index)` ABI hook is implemented: index `N`
into the id-sorted workspace list activates that workspace. There is no
default keybinding for this yet — wire one up via your compositor.

### `[bluetooth]` — Bluetooth plugin

Uses `bluetoothctl`. Click opens a popup with connected devices and available
devices. Available devices with no type/icon and names that are just MAC IDs
are hidden as noise. Clicking an available device starts pairing; if the CLI
reports that a PIN/passkey/code is needed, Oxibar opens a modal for it.

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `refresh_seconds` | integer (`> 0`) | `20` | Background refresh interval. The popup `Scan` button triggers an active scan. |

### `[notifications]` — notification center plugin

Owns `org.freedesktop.Notifications` and stores notifications until they are
closed or cleared. Click the bar button to open a right-side full-height panel.
The panel top row has `DND` and `Clear` controls. If another notification daemon
already owns the DBus name, this plugin logs a warning and cannot receive
notifications until that daemon is stopped.

## Logging

Oxibar uses `tracing` + `tracing-subscriber` with `EnvFilter`. Control verbosity
via the standard `RUST_LOG` environment variable:

```sh
RUST_LOG=info  oxibar     # default
RUST_LOG=debug oxibar
RUST_LOG=oxibar=debug,workspaces=trace oxibar
```

Warnings from plugin loading (missing symbols, ABI mismatch, duplicate names),
plugin error queues, and view/update failures all surface here.

## Full example

```toml
plugins = ["libclock.so", "libworkspaces.so", "libbluetooth.so", "libnotifications.so"]

[bar]
transparent = true
font      = "Adwaita Sans"
font_file = "/run/current-system/sw/share/fonts/Adwaita/AdwaitaSans-Regular.ttf"
start  = ["workspaces"]
center = ["clock"]
end    = ["bluetooth", "notifications"]

[clock]
format       = "%H:%M"
tick_seconds = 60
font_size    = 16
bold         = true
```
