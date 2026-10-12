# Oxibar

Current UI refinements include clock-attached controls, a bar inbox, centered
OxiShut-style session tiles, icon-only bars with live optional labels,
three-column layout dragging with pointer previews, live provider enablement,
configurable shortcuts, animated nested settings navigation, monitor-edge snapping,
shared music/notification placement, pointer-revealed bars,
contrast-aware Base16/Base24 themes and native monitor preview/save/revert.
Implemented [window overview](docs/WINDOW_OVERVIEW_PLAN.md) shows real window-content
thumbnails in large centered modals per monitor. Tab selects windows; h/j/k/l moves
between monitors by their layout. All shell keys are editable in Settings → Shortcuts.

A Rust/Iced widget shell for Wayland: multi-output bars, a resident launcher,
control center, dock, desktop widgets, notifications and live GUI settings.
Your compositor continues to own compositing and window management.

Oxibar preserves its native Rust plugin interface. Shared models and cached
services drive all surfaces; opening a launcher or panel does not start another
provider process or rescan the application catalog.

## Build and run

Standalone [protocol locker and kiosk greeter](docs/AUTH_DEPLOYMENT.md) packages:
`nix build .#oxilock .#oxisession`. The NixOS authentication module supplies PAM and
greetd integration; the user-only Home Manager module installs the locker.

```sh
nix build .#oxibar .#oxibar-applications .#oxibar-tools .#oxibar-clock
nix run .#oxibar
```

Install the selected plugin libraries in `$XDG_CONFIG_HOME/oxibar/plugins/` and
enable their filenames in `config.toml`, or use the Home Manager module. Build
host and plugins from the same workspace/toolchain graph.

The Nix host also bundles an Applications library fallback: enable
`libapplications.so` for application search and real dock icons even without a
separate XDG installation. User-installed libraries take precedence.

For a Cargo development installation:

```sh
nix develop -c cargo build --workspace
scripts/install-plugins.sh
```

The installer includes Applications and checks all twelve libraries before writing
anything. It respects `CARGO_TARGET_DIR` and `XDG_CONFIG_HOME`; pass `release` after
a release build. Restart Oxibar after replacing initialized plugin libraries.

```nix
{
  imports = [ inputs.oxibar.homeManagerModules.default ];
  programs.oxibar = {
    enable = true;
    systemd.enable = true; # Opt-in; defaults to false.
    systemd.target = "hyprland-session.target";
  };
}
```

The compositor session must import `WAYLAND_DISPLAY` and its session environment
into the user manager before starting that target. See
[Home Manager](examples/home-manager.nix) and [Sway](examples/sway.conf) examples.
The module supplies separately selectable plugin packages, including a disabled
installed catalog through `extraPlugins`.

With the daemon running:

```sh
oxibar launcher
oxibar session
oxibar settings
oxibar control-center
oxibar overview
oxibar desktop edit
oxibar action Audio volume +5
oxibar action System brightness -5
oxibar plugins
oxibar status
```

Window commands accept `open`, `close` or `toggle`; omitting the action toggles.
They control the resident shell through IPC. For compositor keybinds:

```ini
# Hyprland
bind = SUPER, SPACE, exec, oxibar launcher toggle
bind = SUPER, ESCAPE, exec, oxibar session toggle
```

```conf
# Sway
bindsym $mod+space exec oxibar launcher toggle
bindsym $mod+Escape exec oxibar session toggle
```

## Features

- Sparse GUI overrides over immutable Nix/base settings, source badges and reset.
- Live Applications/Tools/System provider controls with installed/missing-library
  diagnostics; enabling an installed provider immediately updates its consumers.
- Icon-only bar controls by default; Settings → Bar & layout → Show bar labels
   applies live and supports per-output overrides. Clock time and workspace numbers remain visible.
- Optional `bar.auto_hide` reveals the bar near its configured edge, hides it on
  pointer departure and keeps it visible while a dropdown is open. Output profiles
  can set their own behavior and `bar.reveal_zone` distance.
- Music and notification toasts share configurable `toasts.position`, `margin` and
  `spacing`, with independent stacks per output. All dropdowns close with Escape.
- Centered OxiShut-style session tiles using System's cached capabilities and native
  confirmation flow, with configurable action order and inclusion.
- Existing oxiced palette with spacious Android-inspired forms; explicit live
  theme snapshots, fonts, density and reduced-motion preferences.
- Localized applications and desktop actions, inherited icons, favorites/usage
  ranking, watched indexes and cancellable searches.
- Launcher prefixes: `a ` applications, `>` commands, `=` calculator, `/` files,
  `@` bookmarks, `!` named argument actions and `:` opt-in clipboard history.
- Native PulseAudio/PipeWire and BlueZ controls, cancellable typed pairing,
  brightness/power profiles/session controls and input-transparent OSDs.
- Shared dock catalog and compositor window activation; persistent logical desktop
  placements, explicit move/resize editing and passive widget input policies.
- Capability-gated real per-window overview previews, exact native activation,
  monitor navigation and visible-only bounded capture ownership. Dock SVG/raster
  icons retain their correct renderer and use a portable icon fallback.
- Existing CalDAV, network password modals, notification inline replies/DND,
  StatusNotifier/dbusmenu and workspace flows.

[Configuration](configuration.md) describes defaults, supported capabilities and
commands. [Plugin SDK](docs/PLUGIN_SDK.md) documents ABI 6, lifecycle rules and the
[example plugin](plugins/example/README.md).

[Native lock screen and kiosk session-manager plan](docs/LOCK_SCREEN_SESSION_MANAGER_PLAN.md)
covers a Rust/Iced `ext-session-lock-v1` locker and a greetd-backed greeter hosted
alone in Cage or Hyprland kiosk mode.

## Verification

```sh
nix develop -c cargo fmt --all -- --check
nix develop -c env -u HYPRLAND_INSTANCE_SIGNATURE cargo test --workspace
nix develop -c cargo clippy --workspace --all-targets -- -D warnings
```

[Testing](docs/TESTING.md) and [release checks](docs/RELEASE_CHECKS.md) include
repeatable isolated compositor tests, real dylib/private-service checks and actual
presentation-time benchmarks. The named release reference passed warm presentation
p95 **20.83 ms**, local query max **0.83 ms**, idle **0.30% of one core**, zero idle
redraws and bounded reopen memory/workers. See the recorded workload and limitations;
these are measurements, not compositor configure timestamps.

All roadmap implementation milestones are complete. Remaining native extensions,
compositor capabilities and accessibility work are tracked in
[technical debt](docs/TECHNICAL_DEBT.md).
