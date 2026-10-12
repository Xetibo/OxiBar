# Oxibar configuration

Oxibar reads its configuration from `$XDG_CONFIG_HOME/oxibar/config.toml`
(usually `~/.config/oxibar/config.toml`). The file is plain TOML. If it is
missing, oxibar starts with built-in defaults and no plugins enabled. GUI edits
are stored separately in `$XDG_CONFIG_HOME/oxibar/overrides.toml` and override base
values. Tables merge recursively; scalars and arrays replace as whole values.
Explicit GUI values remain overrides even when equal to the base.

Plugins are loaded from `$XDG_CONFIG_HOME/oxibar/plugins/`. Drop the built
`.so` files there (e.g. `libclock.so`, `libworkspaces.so`) and list the file
names you want enabled in the top-level `plugins` array or `launcher.default_plugins`.
The colon-separated `OXIBAR_PLUGIN_DIR` environment variable supplies additional
search directories after that user directory. The Nix host sets a default fallback
to its pinned Applications package, so enabling `libapplications.so` supplies its
catalog/icons without a separate XDG installation. This does not implicitly enable
any provider; a user-installed library always takes precedence.

For manual development installation, run `nix develop -c cargo build --workspace`
then `scripts/install-plugins.sh` (or pass `release` after a release build). The
installer includes Applications, respects `CARGO_TARGET_DIR` and `XDG_CONFIG_HOME`,
and verifies all thirteen plugin files before installing any of them. Restart after
replacing initialized libraries; build the host and plugins together.

## Top-level keys

| Key       | Type            | Default | Description                                               |
| --------- | --------------- | ------- | --------------------------------------------------------- |
| `plugins` | array of string | `[]`    | File names (not paths) of plugin dylibs to load and run.  |

Example:

```toml
plugins = ["libclock.so", "libworkspaces.so", "libbattery.so", "libbluetooth.so", "libnotifications.so"]
```

A plugin file present on disk but in neither enabled list is not initialized; it remains
visible in the installed catalog. Unknown enabled names
are skipped with a warning. Plugins whose `abi_version()` does not match the
host's are also skipped with a warning — rebuild them against the current
`oxibar-plugin-api`. Plugins can also export an availability check; oxibar
may skip them before model initialization/subscriptions when mandatory dependencies
are missing. Reconnecting native plugins can stay loaded with empty/offline state;
Bluetooth recovers when the service or adapters later appear.

## Installed catalog and session startup

`oxibar plugins` returns installed filenames, enabled/running state, ABI version
and optional capabilities. Settings → Plugins uses the same metadata without
starting disabled models or subscriptions. Enablement applies live: one background
loader initializes new providers; disabled models remain resident for stateful
re-enablement. Their surfaces/subscriptions close and pending host requests are ignored.
Applications, Tools and System are listed even when their package is missing, with
an installation diagnostic. Installing packages is separate from enabling them.
Replacing already-loaded library code still requires restart; host and native
libraries must use the same pinned Rust/dependency graph.

The Home Manager module supports `programs.oxibar.extraPlugins` for installed but
disabled libraries. `config.plugins` still defines the immutable enabled package
set. For example, install the SDK example separately:

```nix
programs.oxibar = {
  enable = true;
  extraPlugins = [ inputs.oxibar.packages.${pkgs.system}.oxibar-example ];
  systemd.enable = true;
  systemd.target = "hyprland-session.target";
};
```

`systemd.enable` defaults to **false**; the default target is
`graphical-session.target`. Deliberate startup attaches the user service to the
chosen session target, requires `WAYLAND_DISPLAY`, restarts on failure and gives
the shell its own control-group termination scope. Import the compositor session
environment into the user manager first. See [Home Manager](examples/home-manager.nix)
and [Sway](examples/sway.conf) binding/startup examples; no compositor bindings are
installed automatically.

Application, tool, calendar and locker launches prefer a native transient user
service in `app.slice`, outside the shell service's termination cgroup. The actual
job result is awaited; uncertain acceptance never causes a second direct launch.
When the user manager is definitely unavailable/unsupported, a reaped argument-
based process-group fallback is used. It cannot promise independent cgroup lifetime
without that manager. This does not change explicit user-configured calendar shell
commands into implicit shell execution for other providers.

The optional SDK example exposes a shared counter and confirmation form:
`oxibar action Example increment`, `reset`, `popup`, and `osd`. Its
`example.label` is live; `example.available` is a restart-only availability probe.
Clock also supports `oxibar action Clock popup`, `refresh`, and
`day YYYY-MM-DD`, using the configured calendar launcher.

See [release checks](docs/RELEASE_CHECKS.md) for reproducible package, catalog,
Home Manager, private app-lifetime, compositor smoke and performance verification.

## `[oxipaste]` — persistent clipboard history

The `liboxipaste.so` provider, named **OxiPaste**, starts its own persistent
clipboard backend when enabled. It captures text/images through native Wayland
data-control while the picker is closed, without a separate daemon or visible
bar. Disabling the plugin stops capture but retains saved history. The Home
Manager module, also exported as `nixosModules.home-manager`, installs and enables
`oxibar-oxipaste` by default. If you replace `programs.oxibar.config.plugins`
with your own enabled package list, include
`inputs.oxibar.packages.${pkgs.system}.oxibar-oxipaste` to retain it. `extraPlugins`
can instead install it as a disabled catalog entry when it is absent from that
enabled list.

```toml
plugins = ["liboxipaste.so"] # Retain your other enabled libraries.

[bar]
end = ["OxiPaste"]

[control_center]
widgets = ["OxiPaste"]

[oxipaste]
paste_shortcut = "ctrl+v"
poll_seconds = 2
max_items = 100
```

| Key | Default | Description |
| --- | --- | --- |
| `paste_shortcut` | `"ctrl+v"` | `"ctrl+v"` or `"ctrl+shift+v"`; choose the target application's paste shortcut. Restart-only. |
| `poll_seconds` | `2` | Refresh interval for an already running external OxiPaste service, from 1 to 3,600 seconds. Bundled capture is event-driven. Restart-only. |
| `max_items` | `100` | Bundled backend's retained history limit, from 1 to 5,000 items, also bounded by 16 MiB per item and 64 MiB total. Restart-only. |

Open the shell's centered, search-focused picker with `oxibar oxipaste open`.
`oxibar oxipaste` toggles it; explicit `close` and `toggle` are also supported.
The picker uses the focused output and works with `bar.enabled = false`, without
placing OxiPaste in a layout. The installed provider must be enabled. Escape uses
the shared `shortcuts.close` binding; opening other exclusive shell overlays
dismisses the picker. `oxibar status` reports `oxipaste_open` and `oxipaste_output`.

Clicking a row copies its exact bytes/MIME, then dismisses the shell's interactive
surfaces and waits for native keyboard focus release before invoking `wtype`.
**Copy** changes the clipboard without dismissal or key injection. Failed writes
keep the picker open and show the actual backend error. Startup/unavailable status
is distinct from a healthy empty history. Search uses text/MIME and preserves
original payloads.

The provider also supports an interactive desktop widget through the layout editor.
History is stored atomically with private permissions in
`$XDG_CONFIG_HOME/oxipaste/items`, compatible with existing OxiPaste history. The
bundled backend uses Oxibar's `[oxipaste]` settings. It serves the existing
`org.Xetibo.OxiPasteDaemon` session API; if an updated external daemon already owns
that name, the plugin uses it instead. Native capture prefers ext-data-control
and falls back to wlr-data-control; unsupported compositors show an inline error.
This provider does not opt into Tools' separate ephemeral launcher clipboard
history. See the [plugin setup guide](plugins/oxipaste/README.md).

## `[bar]` — bar surface

| Key           | Type            | Default          | Description                                                                                                                                                          |
| ------------- | --------------- | ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `transparent` | bool            | `false`          | When `true`, the bar's container background is fully transparent. Plugins still render their own backgrounds. Useful with a wallpaper or a compositor blur effect. |
| `show_labels` | bool            | `false`          | Show names/status values beside bar icons. Applies live, with per-output profile overrides. Clock time and workspace numbers remain visible. |
| `width`       | integer (`> 0`) | active output width | Optional explicit layer width. Omit it to let layer shell size the bar from the compositor-selected active Wayland output. Startup fallback before the first configure event is `3440`. |
| `height`      | integer (`> 0`) | `31`             | Visible bar height. The layer reserves this as its exclusive zone unless auto-hide is enabled, and adds popup headroom internally. |
| `auto_hide`   | bool            | `false`          | Reveal near the configured edge and hide on pointer departure. Open dropdowns keep the bar visible; auto-hidden bars reserve no desktop space. |
| `reveal_zone` | integer `1..128` | `8`              | Logical pixels from the edge that trigger reveal, capped at the visible bar height. Applies live and supports per-output profiles. |
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
`"control-center"` controls, then `"settings"`, enabled `"session"`, then plugins. In explicit layouts,
add these names to any section to display their buttons; they are host controls,
not plugin library names. With labels enabled, the control-center button uses `Controls`.

Host actions have shared SVG icons, optional vertically centered labels and theme-derived
entry spacing. Settings → Bar/Layout shows equal Left, Center and Right columns
with small widget cards. Drag a card onto another card or a column's append area;
a noninteractive preview follows the pointer. Up/down actions remain available.
A drop saves all three arrays atomically and rejects stale starting revisions
instead of changing a newer layout.

Settings → Bar & layout → **Show bar labels** toggles icon-only rendering live.
Bar icons stay visually flat on hover; host action icons do not show name tooltips.
Built-in status providers support
the same preference without changing their shared model; their popup/details retain
the full status. Older third-party providers without the optional `bar_view` hook
keep their legacy rendering. Rebuild/reinstall built-in libraries to use the new hook.

Explicit GUI section overrides suppress fallback even when all arrays are empty,
allowing the GUI to remove every widget. Resetting those overrides restores the legacy
fallback. Add `"settings"` to an explicit Nix/base layout to show its button.

Popup size precedence is: `[bar.popup_sizes]` override, runtime
`popup_metrics(model)`, plugin metadata, then the host default `320 × 300`.
Audio declares `460 × 640`; Bluetooth and Network declare `460 × 420` via metadata.
The default audio dropdown and taller control center fit media plus input/output
controls without scrolling. Popups are bounded to the output's available headroom;
smaller outputs or explicit size overrides retain scrolling.

For a bottom bar that appears when the pointer approaches it:

```toml
[bar]
position = "bottom"
auto_hide = true
reveal_zone = 8
```

The native bar and provider models remain resident. Moving away hides the bar;
an open dropdown pins it until dismissed. Escape closes every bar dropdown and
the optional standalone notification panel.

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
- Exclusive zone: follows `[bar] height`, or zero with auto-hide. A separate `[bar] exclusive_zone = N` may be added later.
- Keyboard interactivity — normally `None`; all attached dropdown menus
  temporarily request `Exclusive` and restore the bar policy when dismissed.
  Session owns a separate centered keyboard-focused overlay.

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
show_labels = true
auto_hide = true
reveal_zone = 8
center = ["clock"]
```

Profiles support `width`, `height`, `transparent`, `show_labels`, `position`,
`auto_hide`, `reveal_zone`, `start`, `center` and `end`; omitted fields inherit common settings. Profile widget arrays replace
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

View output inventory in Settings → Monitors → Arrange displays (Alt+6 selects
the Monitors root) or:

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

### Native display configuration

Settings → Monitors → Arrange displays provides a draggable logical-layout diagram and mode/precise
refresh, scale, X/Y, rotation, enablement and supported VRR controls. Hyprland uses
native version-aware IPC; wlr-output-management uses actual head/mode proxies and
configuration serials. Inventory-only backends show unsupported-control diagnostics.

Dragging snaps opposing or matching display edges within 150 logical units,
following ReSet-style arrangement behavior. Alignment guides show the snapped axes;
dropping an overlapping display restores its starting position. Geometry respects
fractional scale, rotation and negative coordinates. Numeric X/Y edits remain exact
and unsnapped. Arrangement edits do not apply to the compositor until Test layout.

**Test layout** applies a temporary layout with a controller-owned 20-second rollback
lease. **Keep and save** persists one sparse, revision-checked profile array;
**Revert**, expiry, failed persistence and orderly controller shutdown restore the
connected baseline. This never edits the Nix base or compositor dotfiles. Profiles
retain fractional refresh, negative coordinates and unknown fields. At least one
output must stay enabled; new/disconnected outputs are handled during rollback.
In-process rollback cannot execute after an abrupt process kill.

**Apply saved** reapplies profiles explicitly. Startup application is enabled by
default; `apply_on_start` changes take effect after restarting the shell.

```toml
[monitor]
apply_on_start = true

[[monitor.profiles]]
name = "DP-1"
enabled = true
width = 2560
height = 1440
refresh_hz = 143.981
scale = 1.25
x = -2048
y = 0
transform = 0
# vrr = true # Only when supported by this backend/output.
```

Bounds: at most 32 unique connectors; width/height `1..16384`, scale `0.25..8`,
positions `-32768..32768`, transform `0..7`, refresh `0..1000` Hz. Zero denotes
compositor-defined refresh where advertised by a virtual head. `oxibar status`
includes editor/preview diagnostics; merely editing saved profile values does not
substitute for Test Layout/Apply Saved or startup application.

Hotplug creates/removes only the affected surfaces. Popup/modal/panel requests
follow their originating bar; removed outputs dismiss owned surfaces while
retaining plugin state. Unknown/retired window events cannot become bars. Plugins
may optionally export `bar_view(model, context, show_labels)` for output-aware,
label-configurable rendering. It takes precedence over `view_with_context` and
ordinary `view`; those existing hooks remain supported with unchanged signatures.

## Dock and desktop widgets

The dock shares Applications' resident desktop-entry/icon catalog. Pinned entries
use desktop IDs, such as `org.gnome.Nautilus.desktop`; focused accent-hue backgrounds
and exact-window activation use the host's single Hyprland event broker. Clicking
an existing window activates it instead of launching another process. Other
compositors retain pinned launches when window state is unavailable. Inspect cached
window identities and diagnostics with `oxibar windows` or `oxibar status`.
Entries have theme-derived gaps and no below-icon dot. Enable Applications for its
real application catalog/icons; only unresolved icons use the portable fallback.

Application SVGs use the vector renderer; PNG/raster paths use the image renderer.
Missing icons show a shared portable application icon, not an initial letter.
Running-window matching prefers explicit window class/desktop identity over a
generic executable, including Steam game classes derived from their desktop URI.

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

## Window overview

With the resident daemon running:

```sh
oxibar overview             # toggle
oxibar overview open
oxibar overview close
```

Overview is a host capability; it requires no thumbnail plugin per monitor. Add
the reserved `overview` item through Bar & layout, or include it in an explicit
bar section. Bind `oxibar overview` through the compositor for a global shortcut.

Each connected named monitor gets its own large centered modal. Cards show actual
window content with preserved aspect ratio, title/application labels and themed
selection. Tab/Shift+Tab cycles windows within the active monitor; h/j/k/l moves
left/down/up/right between monitors using their logical positions. Arrow keys
select within the grid, Home/End selects the first/last card, Enter or a click activates
the exact existing window, and Escape closes all overview surfaces. Keyboard
selection scrolls into view. Only one monitor owns keyboard focus, and native
overview grabs retire before activation is sent to the compositor.

| `[overview]` key | Default | Meaning |
| --- | --- | --- |
| `enabled` | `true` | Allow overview CLI/bar actions; disabling closes active capture. |
| `width` / `height` | `1400` / `900` | Maximum centered modal size, bounded to 90% of each output; `320..7680` / `240..4320`. |
| `refresh_fps` | `8` | Maximum preview cadence, `1..8`; live. |
| `thumbnail_size` | `512` | Maximum longest thumbnail edge in pixels, `128..640`; rebuilds active capture surfaces. |
| `columns` | `4` | Maximum columns, `1..8`; grids adapt to available logical width. |

```toml
[bar]
start = ["overview", "launcher", "settings"]

[overview]
enabled = true
width = 1400
height = 900
refresh_fps = 8
thumbnail_size = 512
columns = 4
```

These preferences are in Settings → Launcher and apply without daemon restart.
Navigation bindings are in Settings → Shortcuts.
Capture prefers modern ext toplevel source/copy protocols with an exact Hyprland
mapping/wlr activation bridge, or Hyprland export v2 with an exact wlr handle.
Unsupported or denied capture is reported explicitly; an output crop or icon is
not substituted for an occluded window. Sway 1.11 can list/activate windows but
does not provide a supported per-window preview path here. Other compositor
extensions remain capability-dependent.

One visible-owned worker handles all monitors. At most two captures/one per window
are in flight, with 64 MiB staging and 96 MiB thumbnail image data, up to 256 windows.
Capture pixels stay in memory and clear on close, denial or source retirement.
Closed overview retains no capture worker, session or pixel cache. Modern frames
can wait for source damage/readiness; legacy export refreshes at the configured
bounded cadence. `oxibar status` reports capabilities, window identities, output
groups and budgets, without exposing captured pixel bytes. See the implemented
[overview design](docs/WINDOW_OVERVIEW_PLAN.md) for protocol/proof boundaries.

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
can opt into search through the shared API's optional runner hooks. `libtools.so`
(`oxibar-tools`, also in the default Home Manager set) adds the other providers.
The longest advertised prefix selects a provider scope; unprefixed searches may
combine providers. The footer lists prefixes and status, and stars toggle favorites
through sparse GUI overrides. Activations retain the displayed provider/local ID.

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
| `default_plugins` | string array | `[]` | Additional installed provider filenames, deduplicated with `plugins`; live. |

Settings → Launcher has a **Default launcher plugins** switch for installed
Applications and Tools, plus individual provider switches and diagnostics. It
writes a sparse `launcher.default_plugins` list and loads installed providers
immediately; it does not install libraries or initialize a second copy.
Turning it off does not remove a provider explicitly enabled in ordinary/Nix
`plugins`. Disabling a provider in the installed catalog removes it from both lists.

```toml
plugins = ["libclock.so"]
[launcher]
default_plugins = ["libapplications.so", "libtools.so"]
```

### `[applications]` — application provider

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `terminal` | string | `"kitty"` | Terminal command prefix, including any needed execution flags, for `Terminal=true` entries. Parsed as arguments; no implicit shell evaluation. |
| `max_entries` | integer (`> 0`) | `7` | Provider result limit, capped at `50`. Increase alongside the host limit for more application results. |
| `refresh_seconds` | integer (`> 0`) | `300` | Background index refresh interval; opening/querying never scans the filesystem. |
| `prefix` | string | `"a "` | Application-only prefix, including its trailing space; live. |
| `locale` | string | `""` | Live desktop-entry locale; empty uses `LC_ALL`, `LC_MESSAGES`, then `LANG`. |
| `icon_theme` | string | `"auto"` | Live GTK4/GTK3/KDE icon-theme detection, or an explicit theme; inherited parents and hicolor fallback. |
| `favorites` | string array | `[]` | Live desktop/action IDs; normally changed with result stars. |

The provider searches desktop entry names, categories, keywords and generic names.
Empty search combines favorites/recent usage with alphabetical fallback. XDG user data
directories take precedence over system directories, including hidden-entry
masking. Commands use argument-based process launching and support common desktop
Exec field codes. Localized desktop actions are separate stable choices and are
excluded from the dock catalog. Icons honor theme inheritance, user precedence,
pixmaps, absolute paths and names with `.svg`/`.png` suffixes. Nix user profiles,
`NIX_PROFILES`, per-user system profiles and current-system data directories are
also searched, even with an incomplete session `XDG_DATA_DIRS`; XDG paths retain
precedence. An empty index reports its searched directories. Filesystem changes refresh the resident index with
debouncing; the interval is a fallback. Latest-only index/query workers cancel
superseded work. See `docs/TECHNICAL_DEBT.md` for remaining desktop-entry coverage.

### `[tools]` — commands, calculator, files, bookmarks and clipboard

| Prefix | Provider | Action |
| --- | --- | --- |
| `> ` | PATH command | Parse quoted arguments; execute directly, optionally in a terminal. |
| `= ` | Calculator | Evaluate bounded arithmetic/functions and copy the result. |
| `/ ` | Files | Search cached configured roots and open the exact UTF-8 path. |
| `@ ` | Bookmarks | Open a configured URL through the argument-based opener. |
| `! ` | Named actions | Execute the configured argument vector and working directory. |
| `: ` | Clipboard history | Copy a frozen history item; capture must be enabled. |

Calculator copy works with history disabled. Clipboard capture is opt-in, in memory
only, limited to UTF-8 text up to 16 KiB each and 512 KiB total. Disabling it clears
history immediately; nothing is persisted in usage/favorites. Native data-control
prefers ext-data-control with wlr compatibility. Unsupported compositors show a
diagnostic while commands/files/bookmarks remain usable.

```toml
[tools]
max_entries = 7
terminal = "kitty"
opener = "xdg-open"
command_terminal = true
file_roots = ["~/Documents", "~/Downloads"]
file_limit = 20000
favorites = []
clipboard_enabled = false
clipboard_limit = 50
refresh_seconds = 300
bookmarks = [{ id = "docs", name = "Documentation", url = "https://docs.rs/iced/" }]
actions = [{ id = "notes", name = "Notes", command = "editor '/home/me/Notes/file with spaces.txt'", terminal = false }]
```

Result limits are `1..50`, file limits `1..100000`, clipboard count `1..100` and
fallback refresh `1..3600` seconds. Only refresh interval requires restart; other
preferences apply live. Bookmark/action IDs are unique ASCII letters/digits/`-_.`
(1–128 characters). Bookmark schemes are `http`, `https`, `file` or `mailto`;
actions may set `directory`. Commands use quoted argument lists with no implicit
shell evaluation; explicitly name a shell if shell syntax is intended.

Usage counts live in `$XDG_STATE_HOME/oxibar/{applications,tools}-usage.toml`
(default `~/.local/state/oxibar`), with owner-only atomic writes. Initial state loads
before local counts are saved; unsupported/corrupt state is diagnosed and retained.
Stars save only the corresponding `applications.favorites` or `tools.favorites`
override. Query/title/clipboard text is omitted from `oxibar status` diagnostics.

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
notification, calendar and System controls inside its source bar by default. Clock
opens it with the configured calendar selected. Alt+number selects a left-menu
entry; Escape closes the dropdown without removing the bar. The same models/subscriptions
serve bar, popup/panel and center, including password/pairing modals and
notification actions/replies. Settings, launcher and center dismiss each other.

```toml
[control_center]
enabled = true
width = 960
height = 960
presentation = "dropdown"
attach_to_clock = true
widgets = ["Audio", "Network", "Bluetooth", "Battery", "Notifications", "Clock", "System"]
```

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `enabled` | bool | `true` | Enable center content, bar control and open/toggle commands. |
| `width` / `height` | integer `1..16384` | `960` / `960` | Logical dimensions; attached content is clamped to output headroom. |
| `presentation` | `"dropdown"` / `"window"` | `"dropdown"` | Attached bar menu or optional standalone overlay. |
| `attach_to_clock` | bool | `true` | Clock opens the center/calendar instead of an independent popup. |
| `widgets` | string array | Seven providers shown above | Ordered plugin names, matched case-insensitively and deduplicated. Empty means no controls. |

Only running, available plugins with dedicated control-center or popup/panel
content appear. Listing a provider here does not load/install its library;
configure `plugins` and packaged libraries as usual. Plugins may optionally
export `control_center_view`; legacy popup/panel content remains supported.

Preferences apply live and are editable on the Control Center settings page
(Alt+7). Left navigation stays visible; only selected detail content scrolls.
Opening from a bar targets that output; global commands use focused-output
selection. Attached menus close with their owning bar; optional standalone centers
remain available when bars are hidden. Output removal closes affected surfaces
without discarding plugin state. Drawing, input and outside dismissal use the same
actual clicked-button offset, clamped to output edges.

Selecting Notifications closes visible toasts and suppresses new notification
toasts while that inbox is visible. Stored notifications remain available; later
toasts resume after switching away or closing. Native audio and BlueZ devices are
supported. System adds brightness, power profiles and session actions. More shell
features remain tracked in `docs/SHELL_ROADMAP.md`.

### Inbox and session menu

Notifications opens its shared bar inbox by default, retaining inline replies, DND
and records. Escape/Close dismisses it without deleting notifications; selecting
that inbox suppresses visible toasts. Disable integration for the separate panel.

The reserved `"session"` bar item opens a centered OxiShut-style menu, up to
800×480 logical units and bounded to 90% of its output. Three equal columns contain
large icon-only action tiles with themed neutral fills, accent selection, hover
descriptions and a selected-action label. The six default actions form two rows.
Choices come from the existing System provider's cached capabilities. Lock invokes
its configured locker; logout/power choices open the ordinary confirmation modal
before native logind/policy delegation. The menu releases its surface before that
handoff and preserves the originating output. Arrow/Tab navigation and Enter are
scoped to its current window and skip unavailable actions.
Compositor bindings can open the same resident menu directly:

```sh
oxibar session           # toggle
oxibar session open
oxibar session close
oxibar session toggle
```

Repeated `open` keeps the existing surface. These commands show the menu; choosing
an action uses its ordinary capability and confirmation flow. The version-1 IPC
request is `{"version":1,"command":"session","action":"open"}`.

If System is disabled, the menu offers a Plugin settings button; enable the
installed `libsystem.so` there without restarting. A missing library is explicitly
marked as needing the System plugin package.

```toml
[menus]
notifications_dropdown = true
notification_width = 600
notification_height = 640
session_enabled = true
session_actions = ["lock", "logout", "suspend", "hibernate", "reboot", "power-off"]
```

Ordering/inclusion is configurable; unavailable actions are disabled. Control
center and inbox commands retain standalone/panel fallback when no bar exists.
For an independent clock popup, set `control_center.attach_to_clock = false`.
For OxiShut's three-action arrangement, set
`menus.session_actions = ["power-off", "reboot", "suspend"]`.

## Notification and music toast placement

Notification toasts and Audio's music/media-volume feedback share an output-local
stack. Their chosen edges align; notifications appear first and music occupies the
next slot. Updating placement, resizing or dismissing a notification restacks live.

```toml
[toasts]
position = "top-right"
margin = 16
spacing = 12
```

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `position` | string | `"top-right"` | `top-left`, `top-center`, `top-right`, `bottom-left`, `bottom-center` or `bottom-right`. |
| `margin` | integer `0..16384` | `16` | Logical distance from the selected vertical and horizontal output edges. |
| `spacing` | integer `0..16384` | `12` | Logical gap between stack entries. |

All fields apply live in Settings → Appearance. Music remains read-only and
pointer-transparent; notifications retain their existing actions and inline replies.
Device-volume and brightness OSDs continue to use `[osd]` placement below.

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
Audio `media` and `media-volume` use `[toasts]` positioning while retaining the OSD
size and timeout; other topics use `[osd]` position and margin.

The version-1 action request uses `provider`, `action` and `args`:

```json
{"version":1,"command":"action","provider":"System","action":"brightness","args":["+5"]}
```

## Live theme preferences

The built-in Catppuccin palette uses readable muted text, neutral hover surfaces
and contrasting text on filled accents. Explicit colors in an existing oxiced
theme file remain authoritative. Host and upgraded libraries receive one immutable snapshot;
older providers without a theme hook keep their fallback. Theme changes do not
restart services or discard plugin state.

| `[theme]` key | Default | Bounds / behavior |
| --- | --- | --- |
| `preset` | `"current"` | `current`, `compact` or `roomy`; preserves colors, adjusts spacing. |
| `font` | `""` | Live family name; empty inherits the startup bar family. |
| `density` | `1.0` | `0.75..1.5`, multiplies the preset spacing. |
| `font_scale` | `1.0` | `0.75..1.5`, scales theme font tokens. |
| `reduced_motion` | `false` | Render final layout directly when true. |
| `animation_ms` | `150` | `0..1000`; zero also bypasses animation. |
| `radius` | inherited | Optional `0..128`; reset reveals the seed radius. |
| `accent` | inherited | Optional final RGB/RGBA accent; shared by all upgraded surfaces. |

`[theme.palette]` accepts declared color roles as `RRGGBB`/`RRGGBBAA`, optionally
prefixed by `#`. The Theme settings page lists all roles with source/reset controls.
Font files are resolved/read with bounds and uploaded before the family switches;
late font work cannot overwrite newer preferences. Glyph icons keep their icon font.
`bar.font`/`bar.font_file` remain legacy startup settings; use `theme.font` for live
changes. Explicit plugin-specific font sizes remain their own settings.

### Base16/Base24 scheme and contrast

`theme.base24` supports all slots `base00` through `base17`. It also accepts a
Base16 scheme's `base00`–`base0f` without extra colors. Partial schemes inherit
the validated seed; when a Base16 slot is supplied, missing extensions derive as
`10←01`, `11←00`, `12←08`, `13←0a`, `14←0b`, `15←0c`, `16←0d`, `17←0e`.
Explicit Base24 extensions win. Semantic `theme.palette` overrides follow scheme mapping and
`theme.accent` is final. Extra bright red/yellow/green/cyan/blue/magenta states use
`base12`–`base17`; settings/forms do not receive a separate palette.

Schemes are compatibility inputs, while widgets consume semantic roles. Automatic
normal/muted text and filled-control text target WCAG 4.5:1 across their opaque
idle/hover/pressed backgrounds; shared accent-text helpers adjust a display color
without changing the requested accent. Neutral hover stays neutral. Explicit
semantic color overrides are preserved, including intentionally low-contrast ones;
transparent surfaces depend on their backdrop.

This keeps Base24 useful for dashNix/tinted-theming and accepts Stylix's Base16
colors through the same table. A GUI should not use terminal/syntax slots directly:
Catppuccin Mocha's `base04` is Surface2, which is too dim for ordinary helper text.
Keep the adapter and semantic roles rather than replace them with another scheme.
See the [Base16 specification](https://github.com/tinted-theming/home/blob/main/styling.md),
[Catppuccin Base24 source](https://github.com/tinted-theming/schemes/blob/spec-0.11/base24/catppuccin-mocha.yaml)
and [Stylix palette API](https://github.com/nix-community/stylix/blob/master/stylix/palette.nix).

```toml
[theme]
accent = "#ffc857"

[theme.base24]
base00 = "#20252e" # Mantle/background chrome.
base01 = "#252b36"
base05 = "#eef2f6" # Ordinary text.
base0d = "#86b8e8" # Primary when no accent override is set.
base10 = "#2d3542" # Neutral form cards.
base12 = "#f09090"
base13 = "#f8da8e"
base14 = "#b9dc9c"
base15 = "#98dbd4"
base16 = "#a4c9ee"
base17 = "#d3b4ee"
```

Standalone `$XDG_CONFIG_HOME/oxiced/theme.toml` accepts flat slot keys or `[base24]`
and top-level `accent`. Slots are case-insensitive; duplicate spellings are rejected.
Use canonical lowercase host keys for GUI fields. Settings → Theme displays the
accent, all 24 slots and inherited semantic colors; Reset reveals the scheme/seed.

```toml
[theme]
preset = "current"
font = ""
density = 1.0
font_scale = 1.0
reduced_motion = false
animation_ms = 150
```

Compatibility handles retain at most 128 distinct immutable palette/font values
per library. Revision/motion-only changes reuse values; exhausting the budget is
diagnosed before another value is adopted. `oxibar status` reports theme revision,
active family/loading, upgraded providers and hook failures.

## GUI settings and overrides

Open from the settings bar control, launcher/control-center footer, or:

```sh
oxibar settings
oxibar settings open
oxibar settings close
```

Root categories cover Appearance, Bar/Layout, Launcher, Plugins, Advanced, Monitors,
Control Center, Dock, Desktop widgets, Theme and Shortcuts. Each expands into nested
pages: for example Appearance → Notifications & feedback → Notification toasts,
and Theme → Colors → Base24 palette. Search spans all declared settings independently
of the selected page. Fields show effective value, base/default value, source
and application mode. Text/number drafts save with Enter/Apply; switch/pick-list
changes save immediately. Reset removes the GUI override and reveals the current
base/default. Advanced → GUI overrides edits sparse override TOML while preserving
unknown plugin fields.

Forms use spacious themed cards, normal-sized actions and switches. Desktop
widths use indented expandable navigation on the left, with animated child groups,
child-page cards and clickable breadcrumbs. Opening another root folds the previous
root while retaining its nested expansion state. Narrow widths use a full-path page
picker. Live motion preferences control expansion; reduced/zero motion is immediate.
Default settings dimensions are **1440×960**, configurable with `settings.width` /
`settings.height`. Navigation and cards reserve scrollbar gutters. Cards and menu
selection use the same neutral theme roles as launcher/bar controls.

Keyboard controls: Ctrl+F focuses search, Alt+1 through Alt+9 selects the first nine
root categories, Alt+0 selects Theme and Alt+S selects Shortcuts,
Tab/Shift+Tab traverses text fields, Enter applies a field, and Escape closes the
settings surface. Settings access remains available when the launcher is disabled.

### Shortcuts

Settings → Shortcuts contains every host-owned key binding: overlay dismissal,
form traversal, settings search/categories, control-center sections, launcher,
session, overview and desktop editing. Changes apply live. Global daemon-opening
bindings remain in the compositor configuration.

Bindings are comma-separated alternatives with `Ctrl`, `Alt`, `Shift` or `Super`
modifiers joined by `+`. Named keys include `Escape`, `Enter`, `Tab`, `Space`,
`ArrowLeft/Down/Up/Right`, `Home`, `End`, `PageUp/Down`, `Backspace`, `Delete`
and `F1`–`F12`; use `Plus`/`Comma` for literal separator keys. An empty string
disables an action. Invalid names and conflicting bindings in overlapping contexts
are rejected before saving; unrelated surfaces may reuse a key.

```toml
[shortcuts]
close = "Escape"
focus_next = "Tab"
focus_previous = "Shift+Tab"

[shortcuts.overview]
next_window = "Tab"
previous_window = "Shift+Tab"
monitor_left = "h"
monitor_down = "j"
monitor_up = "k"
monitor_right = "l"
activate = "Enter"

[shortcuts.settings]
search = "Ctrl+f"
```

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

- **Live / surface update:** bar transparency/order/geometry/edge reveal, toast placement, launcher enablement,
  dimensions and result limit, settings/control-center dimensions, control-center
  enablement/order, OSD preferences, output selection/profiles, inventory interval,
  inbox/session preferences, Base24/accent and saved display profile values,
  application-provider terminal and result limit, clock text size/bold/calendar
  command, notification default timeout, preferred backlight and locker preferences.
  Dock geometry/content, desktop layout/input, theme preferences and supported runner
  prefix/locale/icon/favorite/Tools settings also update live.
- **Restart required:** legacy bar font, plugin enable/disable/order, instance/control
  endpoint/startup policy, audio backend selection, Bluetooth discovery/pairing
  limits, System backlight root/fallback interval/session selection, existing plugin
  poll timers and clock format granularity.
  Pending changes are displayed; loaded plugin models and system functionality stay
  available until restart. Resetting back to the running value clears the indicator.
- Plugin schemas/hooks are optional. Legacy plugin settings can be edited through
   Advanced → GUI overrides and are treated as restart-only. Plugins are selected from installed
  libraries; adding missing packages remains a Nix/package-manager operation.

Explicit surface dimensions are validated within `1..16384` logical units.
Launcher/provider result limits are `1..50`. The terminal prefix must parse into
at least one executable/argument token. Geometry changes also update bar exclusive
zone and input regions.

### `[settings]` — settings surface

| Key | Type | Default | Description |
| --- | --- | --- | --- |
| `width` | positive integer | `1440` | Settings surface width. |
| `height` | positive integer | `960` | Settings surface height. |

Reset all overrides sits in the header, immediately left of Reload and Close.
Save/reload messages occupy a compact fixed-height footer row. Settings keep the same
surface and form geometry while applying changes; long diagnostics scroll within
that row instead of resizing the form viewport. Selected pages, expansion state
and unapplied drafts survive configuration saves/reloads. The configuration
directory is shown on Advanced → GUI overrides.

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
| `bold`         | bool            | `false`   | Render the time label with bold weight in the current theme family. |
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
