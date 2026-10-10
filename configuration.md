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
may skip them before model initialization/subscriptions when mandatory dependencies
are missing. Reconnecting native plugins can stay loaded with empty/offline state;
Bluetooth recovers when the service or adapters later appear.

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

Fallback layout starts with the enabled host-owned `"launcher"` and
`"control-center"` controls, then `"settings"`, then plugins. In explicit layouts,
add these names to any section to display their buttons; they are host controls,
not plugin library names. The control-center button is labeled `Controls`.

Explicit GUI section overrides suppress fallback even when all arrays are empty,
allowing the GUI to remove every widget. Resetting those overrides restores the legacy
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

- Layer — `Top`. Will become `[bar] layer = "background" | "bottom" | "top" | "overlay"`.
- Margins — `(0, 0, 0, 0)`. Will become `[bar] margin = [t, r, b, l]`.
- Exclusive zone: follows `[bar] height`. A separate `[bar] exclusive_zone = N` may be added later.
- Keyboard interactivity — `None` for the main bar/popup layer; modal dialogs use `Exclusive`. Will become `[bar] keyboard = "none" | "on-demand" | "exclusive"`.

Top/bottom placement is already configured through `bar.position`. Native output
and buffer scaling follow the compositor through the layer-shell backend.

## Outputs and per-output bars

The default `bar.outputs = []` retains one selected output, matching earlier
single-bar behavior. Use `["*"]` for every connected output or a list of connector
names for selected outputs. Selection changes apply live. One plugin model and
subscription remains shared across all bar views.

```toml
[bar]
outputs = ["*"]
enabled = true
position = "top"
output_fallback = true
height = 31
start = ["launcher", "control-center", "settings"]
center = ["clock"]

[bar.profiles."DP-2"]
position = "bottom"
height = 44
center = ["clock"]
```

Profiles support `width`, `height`, `transparent`, `position`, `start`, `center`
and `end`; omitted fields inherit common settings. Profile widget arrays replace
the corresponding common arrays. Dimensions and positions are logical units;
native output/buffer scaling is handled by the layer-shell backend. Top bars place
popups below; bottom bars place them above, with output-local input regions.

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `bar.enabled` | bool | `true` | Show bars; IPC/settings/launcher/control center remain available when false. |
| `bar.outputs` | string array | `[]` | One retained selected output, `["*"]` for all, or explicit names. |
| `bar.position` | `"top"` / `"bottom"` | `"top"` | Common bar edge; may be overridden by profile. |
| `bar.output_fallback` | bool | `true` | Fall back to a connected output if all selected outputs disappear. |
| `monitor.refresh_seconds` | integer `1..3600` | `2` | Background inventory refresh interval; unchanged snapshots are suppressed. |

View the read-only inventory in the Monitors settings page (Alt+6) or:

```sh
oxibar outputs
oxibar status
oxibar config set bar.outputs '["*"]'
oxibar config set bar.profiles.DP-2.height 44
```

Hyprland IPC reports mode/scale/rotation/position/refresh and available modes;
generic wl_output/xdg_output provides a Wayland fallback. Refresh rates retain
fractional precision. Monitor discovery does not change display modes, compositor
configuration or Nix-managed files.

Hotplug creates/removes only the affected surfaces. Popup/modal/panel requests
follow their originating bar; removed outputs dismiss owned surfaces while
retaining plugin state. Unknown/retired window events cannot become bars. Plugins
may optionally export `view_with_context` for output-aware views; existing views
remain supported.

## Dock and desktop widgets

The dock shares Applications' resident desktop-entry/icon catalog. Pinned entries
use desktop IDs, such as `org.gnome.Nautilus.desktop`; running/focused indicators
and exact-window activation use the host's single Hyprland event broker. Clicking
an existing window activates it instead of launching another process. Other
compositors retain pinned launches when window state is unavailable. Inspect cached
window identities and diagnostics with `oxibar windows` or `oxibar status`.

| `[dock]` key | Default | Meaning |
| --- | --- | --- |
| `enabled` | `true` | Show selected output docks. |
| `outputs` | `[]` | One selected output; `["*"]` selects all, names select connectors. |
| `output_fallback` | `true` | Use an available output when a requested one is missing. |
| `pinned` | `[]` | Ordered desktop IDs from the loaded Applications catalog. |
| `show_running` | `true` | Include running-window groups. |
| `position` | `"bottom"` | `top` or `bottom`. |
| `icon_size` | `40` | Logical units, `16..128`. |
| `margin` | `12` | Edge gap, `0..1024`; same-edge bars are accounted for. |
| `reserve_space` | `false` | Reserve dock space instead of overlapping windows. |

Desktop widgets reuse provider models. Clock/calendar, Audio media, Battery and
Stats offer dedicated content; legacy popup content is also supported. Default
layout is empty. Add/edit from the Desktop widgets settings page or:

```sh
oxibar desktop edit
oxibar desktop save
oxibar desktop cancel
```

Explicit edit mode replaces normal widgets with output-wide previews. Select a
provider/output and Add; drag the title or resize corner, use arrow keys (Shift
for larger steps), hide/show/remove, reset placement, then Save or Cancel. Saves
are revision-checked sparse overrides. A conflict retains the draft and error.
Unknown instance fields and the immutable base remain intact.

```toml
[dock]
outputs = ["*"]
pinned = ["org.gnome.Nautilus.desktop"]

[desktop]
enabled = true
output_fallback = true
grid = 16

[[desktop.widgets]]
id = "calendar-one"
plugin = "Clock"
output = "DP-1"
x = 64
y = 128
width = 360
height = 360
enabled = true

[[desktop.widgets]]
id = "stats-one"
plugin = "Stats"
output = "DP-1"
x = 480
y = 128
width = 300
height = 360
```

Up to 32 uniquely named instances are supported. IDs use letters, digits, `-`, `_`
or `.`. Positions are output-relative logical units (`0..16384`), dimensions
`64..16384`; grid is `1..128`. Missing outputs render a clamped fallback when enabled,
without changing requested placement. Reconnecting restores the intended output.
Normal background widgets ignore reserved bar space and take no keyboard focus.
Automatic input policy makes Stats pointer-transparent and media interactive only
when controls exist. Optional `interactive = true/false` overrides it; the editor's
Input button cycles automatic/on/off. Hidden widgets remove surfaces. Clock/Stats
pause their one refresh stream only when no bar/widget/other consumer remains.

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

## Control center

Open the `Controls` bar button or bind a compositor shortcut to:

```sh
oxibar control-center           # toggle
oxibar control-center open
oxibar control-center close
```

The center combines existing audio/media, network, Bluetooth, battery,
notification, calendar and System controls in the resident host. Alt+number selects a
provider tile; Escape closes this surface. The same plugin models/subscriptions
serve bar, popup/panel and center, including password/pairing modals and
notification actions/replies. Settings, launcher and center dismiss each other.

```toml
[control_center]
enabled = true
width = 680
height = 760
widgets = ["Audio", "Network", "Bluetooth", "Battery", "Notifications", "Clock", "System"]
```

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `enabled` | bool | `true` | Enable the overlay, bar control and open/toggle commands. |
| `width` / `height` | integer `1..16384` | `680` / `760` | Overlay dimensions in logical units; live resize. |
| `widgets` | string array | Seven providers shown above | Ordered plugin names, matched case-insensitively and deduplicated. Empty means no controls. |

Only running, available plugins with dedicated control-center or popup/panel
content appear. Listing a provider here does not load/install its library;
configure `plugins` and packaged libraries as usual. Plugins may optionally
export `control_center_view`; legacy popup/panel content remains supported.

Preferences apply live and are editable on the Control Center settings page
(Alt+7). Tiles collapse to one column at narrow widths, and detail content scrolls.
Opening from a bar targets that output; global commands use focused-output
selection. Hiding bars retains the center; removing its output closes the surface
without discarding plugin state.

Selecting Notifications closes visible toasts and suppresses new notification
toasts while that inbox is visible. Stored notifications remain available; later
toasts resume after switching away or closing. Native audio and BlueZ devices are
supported. System adds brightness, power profiles and session actions. More shell
features remain tracked in `docs/SHELL_ROADMAP.md`.

## OSDs and resident actions

Provider actions use the running model and report actual success/failure:

```sh
oxibar action Audio volume +5
oxibar action Audio mute toggle
oxibar action Audio input-volume -5
oxibar action Audio media-volume 50
oxibar action Audio play-pause
oxibar action System brightness +5
oxibar action System profile balanced
oxibar action System suspend       # opens confirmation; does not suspend yet
oxibar action System lock
oxibar osd Audio volume
oxibar osd System brightness
oxibar osd close
```

Audio also supports `input-mute`, `next` and `previous`. Device percentages are
`0..150`; player volume and brightness are `0..100`. Signed steps clamp to bounds.
System session verbs are `suspend`, `hibernate`, `reboot`, `power-off` and `logout`;
they open the same confirmation modal as the GUI. Lock requires a configured
dedicated locker. Unknown/unavailable actions return errors. Slow actions do not
block launcher/status IPC.

```toml
[osd]
enabled = true
width = 360
height = 128
position = "bottom"
margin = 64
timeout_ms = 1800
```

All OSD fields apply live from Appearance settings. Width/height use `1..16384`
logical units, margin `0..16384`, position `top`/`bottom`, duration `100..10000` ms.
OSDs are pointer-transparent and take no keyboard focus or exclusive zone. One
instance per output coalesces updates and renews expiry. Initial snapshots are
quiet; successful actions stay on their original output, while external changes
follow focus. Disabling OSDs does not disable provider actions.

The version-1 action request uses `provider`, `action` and `args`:

```json
{"version":1,"command":"action","provider":"System","action":"brightness","args":["+5"]}
```

## GUI settings and overrides

Open from the settings bar control, launcher/control-center footer, or:

```sh
oxibar settings
oxibar settings open
oxibar settings close
```

Pages cover Appearance, Bar/Layout, Launcher, Plugins, Advanced, Monitors,
Control Center, Dock and Desktop widgets. Search spans all declared settings. Fields show effective value,
base/default value, source
and application mode. Text/number drafts save with Enter/Apply; switch/pick-list
changes save immediately. Reset removes the GUI override and reveals the current
base/default. The Advanced page edits sparse override TOML while preserving
unknown plugin fields.

Forms use spacious themed cards, normal-sized actions and switches. Desktop
widths use category navigation on the left; narrow widths use a category picker.

Keyboard controls: Ctrl+F focuses search, Alt+1 through Alt+9 selects pages,
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
  dimensions and result limit, settings/control-center dimensions, control-center
  enablement/order, OSD preferences, output selection/profiles, inventory interval,
  application-provider terminal and result limit, clock text size/bold/calendar
  command, notification default timeout, preferred backlight and locker preferences.
  Dock geometry/content and desktop layout/input preferences also update live.
- **Restart required:** font loading, plugin enable/disable/order, instance/control
  endpoint/startup policy, audio backend selection, Bluetooth discovery/pairing
  limits, System backlight root/fallback interval/session selection, existing plugin
  poll timers and clock format granularity.
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

This plugin has no configuration keys. It consumes the host's shared event-driven
compositor snapshot and filters each bar to its output. There is no second event
thread or font probe. Missing compositor support is diagnosed through `status`;
the provider stays loaded and recovers when the host broker reconnects.

The host-level `launch(focused_index)` ABI hook is implemented: index `N`
into the id-sorted workspace list activates that workspace. There is no
default keybinding for this yet — wire one up via your compositor.

### `[stats]` — lightweight system summaries

`libstats.so` is included in Home Manager's default plugin set. CPU, memory and
uptime come from bounded proc-file reads and share one model across bar, desktop
and optional control-center views. Refresh pauses when no view uses it, then resumes
immediately. Errors retain the last useful snapshot with inline health.

| Key | Type | Default | Mode |
| --- | --- | --- | --- |
| `refresh_seconds` | integer `1..3600` | `2` | Restart; visible refresh cadence. |
| `proc_root` | string | `"/proc"` | Restart; advanced namespace/fixture root. |

### `[system]` — brightness, power profiles and session controls

Install/select `libsystem.so` (included in Home Manager's default plugin set).
One owner serves bar, popup, control center and OSDs. Backlights use sysfs plus the
current user's verified login1 `SetBrightness` helper when available; ordinary
file permissions are the fallback, without privilege escalation. Profiles prefer
modern UPower PowerProfiles and support the older hadess service. Session controls
display allowed/authentication-required capabilities and delegate confirmed actions
to login1 and the system policy agent. Logout/brightness recheck session ownership.

| Key | Type | Default | Mode / description |
| --- | --- | --- | --- |
| `preferred_backlight` | string | `""` | Live; empty selects first device, otherwise prefer this name. |
| `backlight_root` | string | `"/sys/class/backlight"` | Restart; advanced namespace/fixture directory. |
| `backlight_poll_seconds` | integer `1..60` | `2` | Restart; sysfs fallback only, not remote service polling. |
| `session_id` | string | `""` | Restart; empty resolves environment/process session; foreign UIDs are rejected. |
| `lock_enabled` | bool | `true` | Live; enable dedicated-locker action. |
| `lock_command` | argument string | unset | Live; executable/arguments, quoting supported, no shell evaluation. |

```toml
[system]
lock_command = "swaylock"
```

Unavailable domains show independent inline health while useful cached state stays
visible. A missing profile/login service does not disable ordinary file brightness.
Owners reconnect and reject expired/offline work rather than replaying it. Locking
launches the configured locker; Oxibar does not turn its overlay into a session lock.

### `[audio]` — Audio plugin

Defaults to native PulseAudio events and actions, also compatible with PipeWire's
PulseAudio server. One cached owner serves the bar, popup and control center.
Output/input selection, volume and mute use the native connection; device changes
do not wait for the media timer or invoke `pactl`. Channel balance is preserved
when adjusting volume, and monitor sources are excluded from input choices.

Media metadata, playback and player volume use asynchronous MPRIS DBus calls.
Device updates and audio-server reconnects preserve independent media state.
The popup scrolls to keep all controls reachable within its configured height.

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `backend` | `"native"` / `"pactl"` | `"native"` | Native device events/actions, or explicit CLI compatibility. Restart required. |
| `poll_seconds` | integer (`> 0`) | `4` | MPRIS refresh interval; also refreshes devices in `pactl` mode. Restart required. |

```toml
[audio]
backend = "native"
poll_seconds = 4
```

Native mode retains the last device state and shows an inline error while the
server is unavailable, then reconnects and queries fresh state. Offline/expired
actions are rejected rather than replayed on reconnect. It does not autostart the
server or silently switch adapters. Select `backend = "pactl"` and restart for
compatibility; that mode requires `pactl` on `PATH`. Nix packages include the native
client library. Card profiles, per-stream routing and event-driven media refresh
remain follow-ups.

### `[battery]` — Battery plugin

Reads Linux power-supply data from `/sys/class/power_supply/BAT*`. The bar
button shows a charging or draining icon followed by the rounded percentage.
Hovering shows the exact percentage and either remaining runtime while draining
or time to 100% while charging, when sysfs exposes rate data.

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `poll_seconds` | integer (`> 0`) | `30` | Background refresh interval for capacity and time estimates. |

### `[bluetooth]` — Bluetooth plugin

Uses native BlueZ system-bus events and actions. One cached adapter/device owner
serves the bar, popup and control center without `bluetoothctl` or periodic device
inventory. Controls include adapter selection/power, owned discovery sessions,
connect/disconnect, trust/untrust and forgetting devices. Named devices without an
icon remain usable; unnamed unpaired address-only entries are filtered as noise.

Pairing keeps a real `KeyboardDisplay` agent request pending while the modal asks
for PIN/passkey input, displays a code/progress, or requests numeric confirmation,
authorization or service approval. PINs are 1–16 printable ASCII characters;
passkeys are 1–6 digits, with displayed codes padded to six digits. Enter submits
input; confirmation requests have explicit Allow/Reject actions. Scroll longer
prompts to reach their buttons. Answers are validated without losing the prompt.

The full Pair → Trust → Connect flow is cancellable. Cancel, modal Escape, host
dismissal/replacement or modal output removal ends the pending request; cancellation
during a new Connect also disconnects it. The modal stays open after an answer
until completion or the next request. The agent serves the active user-initiated
device and does not replace the system's global default pairing agent.

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `refresh_seconds` | integer `1..3600` | `20` | Owned scan duration, retaining the legacy key name; no longer an inventory poll interval. Restart required. |
| `pairing_timeout_seconds` | integer `10..300` | `90` | Full pairing/trust/connect deadline. Restart required. |

```toml
[bluetooth]
refresh_seconds = 20
pairing_timeout_seconds = 90
```

Scan/Stop scan affects this client's discovery session only. Its timeout and owner
cleanup release that session; another application's discovery may remain active.
Service loss retains cached devices with an inline diagnostic, rejects offline
actions, cancels pairing, then reconnects and resnapshots without replaying stale
commands. Adapter/device hotplug is event-driven; explicit Refresh resnapshots.
The configured plugin remains loaded even without a current adapter/service.

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
