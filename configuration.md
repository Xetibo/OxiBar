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
plugins = ["libclock.so", "libworkspaces.so"]
```

A plugin file present on disk but not listed here is ignored. Unknown names
are skipped with a warning. Plugins whose `abi_version()` does not match the
host's are also skipped with a warning — rebuild them against the current
`oxibar-plugin-api`.

## `[bar]` — bar surface

| Key           | Type            | Default          | Description                                                                                                                                                          |
| ------------- | --------------- | ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `transparent` | bool            | `false`          | When `true`, the bar's container background is fully transparent. Plugins still render their own backgrounds. Useful with a wallpaper or a compositor blur effect. |
| `font`        | string          | `"Adwaita Sans"` | Default font *family name* used across the entire bar. Applied via iced's `default_font`, so plugins that don't override their own font inherit it. The clock plugin also uses this as the base family when `[clock] bold = true`, so bold rendering preserves your chosen family. **Note:** this only names a family — iced still has to find it via `fontdb`. See `font_file` below if your font isn't being picked up. |
| `font_file`   | string          | _unset_          | Absolute path to a `.ttf` / `.otf` file. When set, the file's bytes are loaded and registered with iced/cosmic-text at startup, bypassing `fontdb`'s directory scan entirely. Use this when `[bar] font` "doesn't seem to load" — typically on Nix / home-manager setups, where fonts live under paths like `~/.nix-profile/share/fonts/...` or `/nix/store/.../home-manager-path/share/fonts/...` that `fontdb` doesn't scan and that `fontconfig-parser` may fail to follow. The value of `font` must still match the family name *inside* the file. |
| `start`       | array of string | `[]`             | Plugin names to render in the **start** (left) section, in the given order.                                                                                          |
| `center`      | array of string | `[]`             | Plugin names to render in the **center** section, in the given order.                                                                                                |
| `end`         | array of string | `[]`             | Plugin names to render in the **end** (right) section, in the given order.                                                                                           |

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
load order — the old single-row layout.

Example:

```toml
[bar]
transparent = true
font      = "Adwaita Sans"
font_file = "/run/current-system/sw/share/fonts/Adwaita/AdwaitaSans-Regular.ttf"
start  = ["workspaces"]
center = ["clock"]
end    = []
```

### Not yet configurable

These are currently hardcoded in `src/main.rs` and will move to `[bar]` in a
later pass:

- Window size — `3440 × 25`. Will be derived from the active wayland output
  or a `[bar] width` / `[bar] height` knob.
- Anchor — `Top`. Will become `[bar] anchor = ["top"]` (array because layer
  shell anchors are bitflags).
- Layer — `Background`. Will become `[bar] layer = "background" | "bottom" | "top" | "overlay"`.
- Margins — `(0, 0, 0, 0)`. Will become `[bar] margin = [t, r, b, l]`.
- Exclusive zone — `25`. Will become `[bar] exclusive_zone = N`.
- Keyboard interactivity — `OnDemand`. Will become `[bar] keyboard = "none" | "on-demand" | "exclusive"`.
- Scale factor — `1.0`. Will become `[bar] scale = ...`.

## Plugin configuration

Each plugin reads its own block. The canonical location is a top-level table
named after the plugin (`[clock]`, `[workspaces]`, ...). The host passes the
entire config document to every plugin's `model()` entry point; the plugin
extracts what it needs.

### `[clock]` — clock plugin

Renders the current local time as a transparent button. Click toggles an
internal `calendar_open` flag — the actual calendar popup is not yet wired
(it requires the host's planned switch to `iced_layershell::daemon` mode).

| Key            | Type            | Default   | Description                                                                                                                                                                              |
| -------------- | --------------- | --------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `format`       | string          | `"%H:%M"` | `chrono` strftime spec. See <https://docs.rs/chrono/latest/chrono/format/strftime/index.html>. Invalid specs are caught at render time and fall back to RFC3339 with a `tracing::warn!`. |
| `tick_seconds` | integer (`> 0`) | `60`      | How often the clock refreshes, in whole seconds. Use `1` if your `format` includes seconds (`%S`). Read once at startup; changes require a restart.                                      |
| `font_size`    | number (`> 0`)  | `14.0`    | Time-label font size in iced units. Accepts integers or floats.                                                                                                                          |
| `bold`         | bool            | `false`   | When `true`, the time label is rendered with a bold font weight. Falls back to a sans-serif family when bold is enabled.                                                                 |

Example:

```toml
[clock]
format       = "%a %H:%M"
tick_seconds = 60
font_size    = 16
bold         = true
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
plugins = ["libclock.so", "libworkspaces.so"]

[bar]
transparent = true
font      = "Adwaita Sans"
font_file = "/run/current-system/sw/share/fonts/Adwaita/AdwaitaSans-Regular.ttf"
start  = ["workspaces"]
center = ["clock"]
end    = []

[clock]
format       = "%H:%M"
tick_seconds = 60
font_size    = 16
bold         = true
```
