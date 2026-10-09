# Oxibar configuration

Oxibar reads its configuration from `$XDG_CONFIG_HOME/oxibar/config.toml`
(usually `~/.config/oxibar/config.toml`). The file is plain TOML. If it is
missing, oxibar starts with built-in defaults and no plugins enabled. GUI edits
are stored separately in `$XDG_CONFIG_HOME/oxibar/overrides.toml` and override base
values. Tables merge recursively; scalars and arrays replace as whole values.
Explicit GUI values remain overrides even when equal to the base.

Plugins are loaded from `$XDG_CONFIG_HOME/oxibar/plugins/`. Drop the built
`.so` files there (e.g. `libclock.so`, `libworkspaces.so`) and list the file
names you want enabled in the top-level `plugins` array.

## Top-level keys

| Key       | Type            | Default | Description                                               |
| --------- | --------------- | ------- | --------------------------------------------------------- |
| `plugins` | array of string | `[]`    | File names (not paths) of plugin dylibs to load and run.  |

Example:

```toml
plugins = ["libclock.so", "libworkspaces.so", "libbattery.so", "libbluetooth.so", "libnotifications.so"]
```

A plugin file present on disk but not listed here is ignored. Unknown names
are skipped with a warning. Plugins whose `abi_version()` does not match the
host's are also skipped with a warning — rebuild them against the current
`oxibar-plugin-api`. Plugins can also export an availability check; oxibar
skips them before model initialization and polling when required hardware or
system services are missing.

## `[bar]` — bar surface

| Key           | Type            | Default          | Description                                                                                                                                                          |
| ------------- | --------------- | ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `transparent` | bool            | `false`          | When `true`, the bar's container background is fully transparent. Plugins still render their own backgrounds. Useful with a wallpaper or a compositor blur effect. |
| `width`       | integer (`> 0`) | active output width | Optional explicit layer width. Omit it to let layer shell size the bar from the compositor-selected active Wayland output. Startup fallback before the first configure event is `3440`. |
| `height`      | integer (`> 0`) | `31`             | Visible bar height. The layer reserves this as its exclusive zone and adds popup headroom internally. |
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

When the launcher is enabled, fallback layout also adds the host-owned
`"launcher"` search button first. In explicit layouts, add `"launcher"` to any
section to display it. It is a host control, not a plugin library name.

Fallback layout also includes a host-owned `"settings"` control. Explicit GUI
section overrides suppress fallback even when all arrays are empty, allowing
the GUI to remove every widget. Resetting those overrides restores the legacy
fallback. Add `"settings"` to an explicit Nix/base layout to show its button.

Popup size precedence is: `[bar.popup_sizes]` override, runtime
`popup_metrics(model)`, plugin metadata, then the host default `320 × 300`.
`audio`, `bluetooth`, and `network` declare `460 × 420` via metadata.

Example:

```toml
[bar]
transparent = true
height      = 31
font      = "Adwaita Sans"
font_file = "/run/current-system/sw/share/fonts/Adwaita/AdwaitaSans-Regular.ttf"
start  = ["workspaces"]
center = ["clock"]
end    = ["bluetooth", "notifications"]

[bar.popup_sizes]
clock = [340, 320]
```

### Not yet configurable

These are currently fixed in `src/layout.rs` and will move to `[bar]` in a
later pass:

- Anchor — `Top`. Will become `[bar] anchor = ["top"]` (array because layer
  shell anchors are bitflags).
- Layer — `Top`. Will become `[bar] layer = "background" | "bottom" | "top" | "overlay"`.
- Margins — `(0, 0, 0, 0)`. Will become `[bar] margin = [t, r, b, l]`.
- Exclusive zone: follows `[bar] height`. A separate `[bar] exclusive_zone = N` may be added later.
- Keyboard interactivity — `None` for the main bar/popup layer; modal dialogs use `Exclusive`. Will become `[bar] keyboard = "none" | "on-demand" | "exclusive"`.
- Scale factor — `1.0`. Will become `[bar] scale = ...`.

## Resident launcher

Start `oxibar` once from the compositor/session startup configuration, then bind
your launcher key to:

```sh
oxibar launcher          # toggle
oxibar launcher open
oxibar launcher close
```

For example, a Hyprland binding can use `bind = SUPER, SPACE, exec, oxibar launcher`.
The command sends a request to the running bar without starting another GUI
process. Escape closes the overlay, Up/Down or Tab/Shift+Tab selects a result,
and Enter/click activates it. The bar and plugin subscriptions continue running.
The overlay targets the focused Hyprland output when available, with the same
fallback as toast output selection on other compositors.

Load `libapplications.so` in `plugins = [...]` for desktop application search.
It is packaged as `oxibar-applications` and included in the Home Manager module's
default plugins. If you supply a custom plugin package list, include it there.
No separate OxiRun executable or legacy OxiRun library is needed. Other plugins
can opt into search through the shared API's optional runner hooks.

```toml
plugins = ["libapplications.so", "libclock.so"]

[bar]
start = ["launcher"]
center = ["clock"]

[launcher]
enabled = true
width = 600
height = 600
max_entries = 7

[applications]
terminal = "kitty"         # include terminal execution flags here if needed
max_entries = 7
refresh_seconds = 300
```

### `[launcher]` — host overlay

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `enabled` | bool | `true` | Enable launcher surfaces, bar control and command listener. |
| `width` / `height` | integer (`> 0`) | `600` / `600` | Overlay dimensions in logical units. |
| `max_entries` | integer (`> 0`) | `7` | Maximum globally ranked visible results, capped at `50`. |

### `[applications]` — application provider

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `terminal` | string | `"kitty"` | Terminal command prefix, including any needed execution flags, for `Terminal=true` entries. Parsed as arguments; no implicit shell evaluation. |
| `max_entries` | integer (`> 0`) | `7` | Provider result limit, capped at `50`. Increase alongside the host limit for more application results. |
| `refresh_seconds` | integer (`> 0`) | `300` | Background index refresh interval; opening/querying never scans the filesystem. |

The provider searches desktop entry names, categories, keywords and generic names.
Empty search displays initial alphabetically ordered applications. XDG user data
directories take precedence over system directories, including hidden-entry
masking. Commands use argument-based process launching and support common desktop
Exec field codes; icons use hicolor/Adwaita/pixmap lookup or absolute image paths.
See `docs/TECHNICAL_DEBT.md` for advanced desktop-entry/icon features not yet covered.

### `[ipc]` — command socket

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `socket_path` | string | `$XDG_RUNTIME_DIR/oxibar.sock` | Optional custom socket path. Without a runtime directory, falls back to the Oxibar config directory. |

The socket is created with owner-only permissions and stale sockets are recovered
on restart. Multiple independently controlled bars need distinct instance locks
and socket paths. The CLI must read the same config/environment as its daemon.

## GUI settings and overrides

Open from the settings bar control, launcher footer, or:

```sh
oxibar settings
oxibar settings open
oxibar settings close
```

Pages cover Appearance, Bar/Layout, Launcher, Plugins and Advanced. Search spans
all declared settings. Fields show effective value, base/default value, source
and application mode. Text/number drafts save with Enter/Apply; checkbox/pick-list
changes save immediately. Reset removes the GUI override and reveals the current
base/default. The Advanced page edits sparse override TOML while preserving
unknown plugin fields.

Keyboard controls: Ctrl+F focuses search, Alt+1 through Alt+5 selects pages,
Tab/Shift+Tab traverses text fields, Enter applies a field, and Escape closes the
settings surface. Settings access remains available when the launcher is disabled.

Base `config.toml` may remain an immutable Home Manager/Nix-store symlink. Oxibar
never writes it. Override writes are validated, revision-checked, owner-only and
atomic. File edits and base-symlink replacement reload automatically; invalid
input retains the last valid running configuration and shows an error.

```toml
# overrides.toml: only settings explicitly changed in the GUI
[bar]
height = 38

[launcher]
max_entries = 10
```

A Nix rebuild changing base height to 34 leaves GUI height at 38. Resetting the
height then reveals 34. Unoverridden fields follow the regenerated base.

### Application modes

- **Live / surface update:** bar transparency/order/geometry, launcher enablement,
  dimensions and result limit, settings dimensions, application-provider terminal
  and result limit, clock text size/bold/calendar command, notification default timeout.
- **Restart required:** font loading, plugin enable/disable/order, instance/control
  endpoint/startup policy, existing plugin poll timers and clock format granularity.
  Pending changes are displayed; loaded plugin models and system functionality stay
  available until restart. Resetting back to the running value clears the indicator.
- Plugin schemas/hooks are optional. Legacy plugin settings can be edited through
  Advanced and are treated as restart-only. Plugins are selected from installed
  libraries; adding missing packages remains a Nix/package-manager operation.

Explicit surface dimensions are validated within `1..16384` logical units.
Launcher/provider result limits are `1..50`. The terminal prefix must parse into
at least one executable/argument token. Geometry changes also update bar exclusive
zone and input regions.

### `[settings]` — settings surface

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `width` | positive integer | `960` | Settings surface width. |
| `height` | positive integer | `720` | Settings surface height. |

### Configuration commands

```sh
oxibar config get
oxibar config get bar.height
oxibar config set bar.height 38
oxibar config set bar.start '["launcher", "settings", "workspaces"]'
oxibar config set bar.font '"Adwaita Sans"'
oxibar config reset bar.height
oxibar config reset-all
oxibar config reload
oxibar config schema
oxibar config export effective
oxibar config export overrides
oxibar status
```

Set values are TOML literals. Configuration/status commands return versioned JSON
with revision, success, data and error fields; export prints TOML. The schema
response describes typed bounds/options and source/application mode. Mutations
report validation/persistence failures instead of only acknowledging a queue.

Socket clients may supply a revision to reject stale edits:

```json
{"version":1,"command":"config","action":"set","path":"bar.height","value":"38","revision":2}
```

The daemon records its active endpoint in `control.socket` under the config
directory, so CLI access continues while a new `ipc.socket_path` awaits restart.
Legacy text launcher socket commands remain supported.

Home Manager owns the base and installed plugin symlinks using the configured
`xdg.configHome`; it does not manage `overrides.toml`. Existing
`programs.oxibar.config.plugin_config` remains the declarative base interface.

Further shell milestones are tracked in `docs/SHELL_ROADMAP.md`. Shared UI helpers
now live in this repository at `crates/oxiced`.

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
| `tick_seconds` | integer (`> 0`) | `60`      | Real-clock polling interval. Seconds formats still display simulated per-second ticks between polls. Timer changes require restart. |
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

### `[audio]` — Audio plugin

Uses `pactl` for PulseAudio/PipeWire device state and MPRIS for media controls.

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `poll_seconds` | integer (`> 0`) | `4` | Background refresh interval for output/input devices and MPRIS state. |

### `[battery]` — Battery plugin

Reads Linux power-supply data from `/sys/class/power_supply/BAT*`. The bar
button shows a charging or draining icon followed by the rounded percentage.
Hovering shows the exact percentage and either remaining runtime while draining
or time to 100% while charging, when sysfs exposes rate data.

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `poll_seconds` | integer (`> 0`) | `30` | Background refresh interval for capacity and time estimates. |

### `[bluetooth]` — Bluetooth plugin

Uses `bluetoothctl`. Click opens a popup with connected devices and available
devices. Available devices with no type/icon and names that are just MAC IDs
are hidden as noise. Clicking an available device starts pairing; if the CLI
reports that a PIN/passkey/code is needed, Oxibar opens a modal for it.

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `refresh_seconds` | integer (`> 0`) | `20` | Background refresh interval. The popup `Scan` button triggers an active scan. |

### `[network]` — Network plugin

Uses NetworkManager's `nmcli` for active connections, saved profiles, and Wi-Fi scans.

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `scan_seconds` | integer (`> 0`) | `15` | Background refresh interval for NetworkManager state. |

### `[notifications]` — notification center plugin

Owns `org.freedesktop.Notifications` and stores notifications until they are
closed or cleared. Click the bar button to open a right-side full-height panel.
The panel top row has `DND` and `Clear` controls. If another notification daemon
already owns the DBus name, this plugin logs a warning and cannot receive
notifications until that daemon is stopped.

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `timeout` | integer (`> 0`) | `3` | Default toast lifetime in seconds when a notification does not provide a positive `expire_timeout`. Timeout closes only the toast; the notification remains visible in the notification center until dismissed, cleared, replied to, or actioned. |

Example:

```toml
[notifications]
timeout = 5
```

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
plugins = ["libclock.so", "libworkspaces.so", "libbattery.so", "libbluetooth.so", "libnotifications.so"]

[bar]
transparent = true
font      = "Adwaita Sans"
font_file = "/run/current-system/sw/share/fonts/Adwaita/AdwaitaSans-Regular.ttf"
start  = ["workspaces"]
center = ["clock"]
end    = ["battery", "bluetooth", "notifications"]

[clock]
format       = "%H:%M"
tick_seconds = 60
font_size    = 16
bold         = true
```
