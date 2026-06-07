# Architecture

Oxibar is a Rust workspace containing the host bar, a shared plugin API crate, and dynamic plugin crates.

## Workspace Structure

- `src/main.rs`: module wiring and binary entry point.
- `src/app.rs`: iced layershell daemon setup, main application state, update loop, subscriptions, and popup/modal/panel state transitions.
- `src/layout.rs`: geometry constants, layershell settings, bar dimension config/fallback parsing, popup metrics, popup config parsing, and bar section math.
- `src/surfaces.rs`: bar, popup, modal, and panel view construction.
- `src/messages.rs`: host message enum, layer-shell action conversion, and plugin message routing.
- `src/font.rs`: bar font config, fontconfig resolution, and font byte loading.
- `src/config.rs`: XDG config discovery and TOML config helpers.
- `src/plugins.rs`: dynamic library discovery, ABI symbol resolution, optional plugin availability checks, plugin model initialization, and safe wrappers for plugin view/update/error calls.
- `crates/oxibar-plugin-api`: shared ABI types used by host and plugins.
- `plugins/*`: dynamic plugin crates loaded from `$XDG_CONFIG_HOME/oxibar/plugins/` when their library filename appears in `plugins = [...]`.
- `plugins/{audio,battery,bluetooth,network,tray}/src/system.rs`: command/DBus/sysfs integration, parsing, and domain data extracted from plugin ABI/UI files.
- `plugins/clock/src/caldav.rs`: HTTPS-only CalDAV sync, current-month calendar queries, minimal iCalendar parsing, common recurrence expansion, and Nextcloud-compatible calendar query support.
- `flake.nix` and `nix/*.nix`: Nix packaging for the host and dynamic plugin libraries. Crane builds one workspace dependency artifact derivation and reuses it for every host/plugin package so shared Rust dependencies are not rebuilt per plugin.

## Runtime Flow

- `main()` configures tracing, layershell settings, font loading, theme, subscription, update, and view callbacks.
- The main layer asks layer shell for the compositor-selected active output width by using left/right/top anchors and width `0`. Iced window open/resize events report the configured surface size back into `OxiBar`, which then updates popup placement and input-region math.
- `OxiBar::new()` loads configured and available plugins in the order listed by `plugins = [...]` and reads `[bar]` section placement for start, center, and end widgets.
- Plugins are keyed by their declared `name()` string, not by filesystem path or load index. Fallback layout order follows config plugin order.
- Plugin messages are wrapped as `PluginMsg` and mapped to host messages when they carry a known host request string.
- Host-request mapping is centralized in `map_plugin_message()`, so plugin subscription messages and plugin initialization tasks use the same routing rules.
- The host owns popup, modal, panel, and toast layer-shell surfaces. Plugins render only inner content for those surfaces. Popup open/close layout animation uses a fixed quick ease-out transition so app-side popups track notification-panel layer opening speed more closely.
- Visible popup size comes from `[bar.popup_sizes]`, then optional runtime `popup_metrics(model)`, then optional plugin metadata, then host default size. Plugins can also request a larger transparent popup input region through runtime metrics or metadata for detached overlays such as tray context menus.

## Plugin ABI

- Required symbols: `abi_version`, `name`, `model`, `update`, `launch`, `view`, `errors`, and `subscription`.
- Optional symbols: `availability`, `metadata`, `popup_metrics`, `popup_view`, `modal_view`, and `panel_view`.
- `availability(config)` lets a plugin skip loading before model initialization, init tasks, and subscriptions when required hardware or system services are unavailable.
- `ABI_VERSION` in `oxibar-plugin-api` gates host/plugin compatibility.
- Plugin models are `Arc<RwLock<Box<dyn OxiAny>>>`; plugin messages are `Arc<dyn OxiAny>`.
- Plugin subscriptions return a raw pointer to a boxed stream. The host rebuilds it while keeping the dynamic library alive for function-pointer validity.

## Current Plugins

- `audio`: PulseAudio/PipeWire `pactl` plus MPRIS controls; popup surface.
- `battery`: Linux power-supply sysfs percentage, charging/draining state, and calendar-style hover tooltip with duration estimates.
- `bluetooth`: `bluetoothctl` scan, connect, disconnect, and pairing modal; popup and modal surfaces.
- `clock`: time display, local calendar popup with event-day tooltips, optional HTTPS CalDAV event sync, and configurable external calendar launcher.
- `network`: NetworkManager `nmcli` connection management and password modal; popup and modal surfaces.
- `notifications`: Freedesktop notification server, toast layers, inline replies, DND state, and side panel. Toast layers start with keyboard interactivity disabled and are switched to `OnDemand` only after user hover intent so incoming notifications do not steal keyboard focus. Toast timeout closes only the host toast surface; stored notifications remain in the panel until dismissed, cleared, replied to, or actioned.
- `tray`: StatusNotifier watcher, dynamically sized tray item popup, activation, and DBusMenu-backed detached context menu rendering. Item registration publishes a fallback row immediately and refreshes DBus metadata asynchronously so registering applications are not blocked by property queries.
- `workspaces`: Hyprland workspace display and dispatch.

Plugin `lib.rs` files should keep ABI symbols, model update, and view composition. External-system logic and parsers should live in sibling modules once they grow beyond small helpers.

## Tradeoffs

- Several runtime integrations call external CLIs or DBus APIs directly; pure parsing/state helpers should be covered by unit tests, while live integration remains manual.
- The clock plugin's CalDAV sync shells out to `curl` to avoid adding a full HTTP stack to the dynamic plugin. It enforces `https://`, curl HTTPS-only protocol flags, and normal certificate verification.
- Thunderbird integration is limited to launching `thunderbird --calendar`; date-focused navigation remains a user-configured `calendar_command` concern because Thunderbird has no stable CLI for opening a specific calendar date.
- Bar width is dynamic from the active layer-shell output when the compositor reports it. `[bar] width` and `[bar] height` provide config fallback/override values, while anchor, layer, margins, keyboard mode, and scale factor remain fixed in `src/layout.rs`.
- The stream raw-pointer ABI is documented as practical but not fully C-ABI-safe.

## Nix Packaging

- `inputs.crane` provides Cargo derivations for Nix builds.
- `flake.nix` creates one cleaned Cargo source, one vendored dependency set, and one `buildDepsOnly` workspace artifact derivation with `--workspace` package selection.
- `nix/default.nix` builds only the host package from those shared Cargo artifacts.
- `nix/plugin.nix` builds only the requested plugin package from the same shared Cargo artifacts and installs the resulting `lib*.so` under `$out/lib/`.
- This reduces rebuild time when multiple plugins are updated because dependency crates such as `iced`, `oxiced`, `zbus`, and `oxibar-plugin-api` are compiled once per dependency graph instead of once per plugin derivation. Final plugin crate compilation and linking still happen per plugin.
