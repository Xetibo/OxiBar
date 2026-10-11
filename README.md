# Oxibar

A Rust/Iced widget shell for Wayland: multi-output bars, a resident launcher,
control center, dock, desktop widgets, notifications and live GUI settings.
Your compositor continues to own compositing and window management.

Oxibar preserves its native Rust plugin interface. Shared models and cached
services drive all surfaces; opening a launcher or panel does not start another
provider process or rescan the application catalog.

## Build and run

```sh
nix build .#oxibar .#oxibar-applications .#oxibar-tools .#oxibar-clock
nix run .#oxibar
```

Install the selected plugin libraries in `$XDG_CONFIG_HOME/oxibar/plugins/` and
enable their filenames in `config.toml`, or use the Home Manager module. Build
host and plugins from the same workspace/toolchain graph.

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
oxibar settings
oxibar control-center
oxibar desktop edit
oxibar action Audio volume +5
oxibar action System brightness -5
oxibar plugins
oxibar status
```

## Features

- Sparse GUI overrides over immutable Nix/base settings, source badges and reset.
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
- Existing CalDAV, network password modals, notification inline replies/DND,
  StatusNotifier/dbusmenu and workspace flows.

[Configuration](configuration.md) describes defaults, supported capabilities and
commands. [Plugin SDK](docs/PLUGIN_SDK.md) documents ABI 6, lifecycle rules and the
[example plugin](plugins/example/README.md).

## Verification

```sh
nix develop -c cargo fmt --all -- --check
nix develop -c cargo test --workspace
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
