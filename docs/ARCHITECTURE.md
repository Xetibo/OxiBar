# Architecture

Oxibar is a Rust workspace containing the host bar, a shared plugin API crate, and dynamic plugin crates.

## Workspace Structure

- `src/main.rs`: module wiring and binary entry point.
- `src/app.rs`: iced layershell daemon setup, main application state, update loop, subscriptions, and popup/modal/panel state transitions.
- `src/layout.rs`: geometry constants, layershell settings, bar dimension config/fallback parsing, popup metrics, popup config parsing, and bar section math.
- `src/monitor/`: read-only normalized output inventory, precise mode/scale/rotation
  geometry, Hyprland IPC and generic Wayland wl_output/xdg_output adapters. Focus
  remains a separate compositor capability.
- `src/shell/`: explicit surface registry, per-output bar instances, source-window
  routing, hotplug reconciliation, input regions and auxiliary surface ownership.
- `src/config/bars.rs`: common/per-output bar profile merging and validation.
- `src/single_instance.rs`: config-driven single-instance lock (`[instance]` table), flock acquisition, and the RAII `SingleInstanceGuard`.
- `src/surfaces.rs`: bar, popup, modal, and panel view construction.
- `src/messages.rs`: host message enum, layer-shell action conversion, and plugin message routing.
- `src/font.rs`: bar font config, fontconfig resolution, and font byte loading.
- `src/config/`: source-aware snapshots, typed host/plugin schemas, recursive
  base/GUI merging, atomic override persistence, revision checks and filesystem
  watching. Startup/instance compatibility helpers remain in `mod.rs`.
- `src/settings/`: settings state/actions, spacious schema-driven views in
  `view.rs`, and host-side command/application logic in `runtime.rs`.
- `src/control_center.rs`: host-owned control-center state, capability selection,
  themed composition and window-scoped keyboard routing.
- `src/background.rs`: runtime-independent bridge for serialized filesystem jobs
  into Iced tasks; rendering uses immutable snapshots without locking the store.
- `src/launcher.rs`: resident search-overlay state, cached global result ranking, keyboard routing, and themed view.
- `src/ipc.rs`: bounded, versioned Unix-socket commands for launcher, settings,
  control center, output inventory, configuration queries/mutations and status.
  CLI invocations exit before daemon/font/renderer initialization; an active-endpoint record keeps clients
  connected while socket-setting changes await restart.
- `src/plugins.rs`: dynamic library discovery, ABI symbol resolution, optional plugin availability checks, plugin model initialization, and safe wrappers for plugin view/update/error calls.
- `crates/oxibar-plugin-api`: shared ABI types used by host and plugins.
- `crates/oxiced`: local shared theme/widget library imported from the previously
  pinned upstream revision; host/plugins use one workspace dependency and lockfile.
- `plugins/*`: dynamic plugin crates loaded from `$XDG_CONFIG_HOME/oxibar/plugins/` when their library filename appears in `plugins = [...]`.
- `plugins/{audio,battery,bluetooth,network,tray}/src/system.rs`: command/DBus/sysfs integration, parsing, and domain data extracted from plugin ABI/UI files.
- `plugins/clock/src/caldav.rs`: HTTPS-only CalDAV sync, current-month calendar queries, minimal iCalendar parsing, common recurrence expansion, and Nextcloud-compatible calendar query support.
- `flake.nix` and `nix/*.nix`: Nix packaging for the host and dynamic plugin libraries. Crane builds one workspace dependency artifact derivation and reuses it for every host/plugin package so shared Rust dependencies are not rebuilt per plugin.

## Runtime Flow

- `main()` configures tracing, layershell settings, font loading, theme, subscription, update, and view callbacks.
- `run()` takes an optional single-instance lock via `single_instance::acquire()` from the `[instance]` config table; when another oxibar holds the lock the new process prints an error and exits non-zero (`allow_multiple_instances = true` skips the lock). The guard is held for the process lifetime.
- `run()` wraps startup in a retry loop driven by `[startup]` config (see `DECISIONS.md`). Tracing is initialized once before the loop. A cheap `wayland_client::Connection::connect_to_env()` probe skips attempts while the compositor socket is not yet accepting clients, and the daemon `run_bar()` is wrapped in `catch_unwind` so the layershell `Cannot create layershell` panic is converted into a backoff-and-retry instead of an immediate exit. Retries use exponential backoff (double the delay, capped at `max_delay_ms`).
- The daemon starts in background mode and creates every bar with a known ID.
  Output selection retains one selected output by default or creates one bar per
  selected connected output. Automatic width uses layer-shell anchors and size
  `0`; configured sizes update only the registered bar for that ID.
- All view routing is through registered surface kinds. Unknown/retired window
  events do not become bars. Output hotplug reconciles surface instances without
  rebuilding plugin models or subscriptions.
- `OxiBar::new()` loads configured and available plugins in the order listed by `plugins = [...]` and reads `[bar]` section placement for start, center, and end widgets.
- Plugins are keyed by their declared `name()` string, not by filesystem path or load index. Fallback layout order follows config plugin order.
- Plugin messages are wrapped as `PluginMsg` and mapped to host messages when they carry a known host request string.
- Host-request mapping is centralized in `map_plugin_message()`, so plugin subscription messages and plugin initialization tasks use the same routing rules.
- The host owns popup, modal, panel, toast and control-center layer-shell surfaces.
  Plugins render only inner content for those surfaces. Popup open/close layout
  animation uses a fixed quick ease-out transition so app-side popups track
  notification-panel layer opening speed more closely.
- The host also owns a keyboard-focused launcher overlay. Opening it reuses the resident renderer and plugin models; closing or activating a result removes only that surface. Compositor bindings invoke `oxibar launcher` over a runtime Unix socket.
- Optional runner providers return bounded result data with stable provider-local launch indexes. The host caches a globally ranked list on state updates and dispatches activation only to the selected provider's existing `launch` hook.
- Per-output bars share one plugin model/subscription per plugin. Optional
  context-aware views receive output name, native scale and an instance identifier;
  ordinary plugin views remain the fallback. UI/task messages retain their source
  bar for popup/modal/panel placement. Late task results still update shared models
  after their source surface disappears.
- Popups are visible/input-active only on their owning bar. Modal/panel/launcher/
  settings/control-center/toast surfaces carry output ownership; output removal
  dismisses their surfaces while preserving plugin data. Toast offsets are grouped
  per output.
- The control center composes one selected provider's existing shared model. It
  prefers optional `control_center_view`, then falls back to popup/panel content;
  the host owns scrolling, cards, selection and output placement. Configured order
  is case-insensitive and deduplicated, and unsupported/unavailable providers are
  omitted. Opening/closing a center never initializes another plugin or service.
- Settings, launcher and control center dismiss each other's surfaces. Plugin
  task results preserve source routing for password/pairing modals. Hiding a bar
  on a still-connected output detaches ownership and retains open auxiliary
  surfaces. Viewing the notification inbox dismisses visible toasts and suppresses
  new notification toasts while retaining stored notifications.
- Settings writes are serialized behind the config store on background workers.
  Validated sparse overrides are written with owner-only permissions using a
  temporary file, file sync, atomic rename and directory sync. Base/Nix files are
  read-only inputs. External writes and symlink replacement reload snapshots;
  invalid edits retain the last valid state and surface diagnostics.
- Bar height/width, transparency, layout and launcher settings apply live through
  layer actions and explicit input-region refresh. Effective-value and override
  provenance changes both affect layout: explicit empty GUI arrays suppress the
  legacy all-empty fallback. Control-center geometry/order/enablement also apply
  live. Settings, launcher and control-center focus are window-scoped.
- Plugin lifecycle/font/timer changes are classified as restart-required. Current
  plugin libraries/models stay loaded; optional config hooks apply supported live
  values without losing plugin state. Applications, clock presentation/calendar
  commands, and notification default timeout currently support live updates.
- Visible popup size comes from `[bar.popup_sizes]`, then optional runtime `popup_metrics(model)`, then optional plugin metadata, then host default size. Plugins can also request a larger transparent popup input region through runtime metrics or metadata for detached overlays such as tray context menus.

## Plugin ABI

- Required symbols: `abi_version`, `name`, `model`, `update`, `launch`, `view`, `errors`, and `subscription`.
- Optional symbols: `availability`, `metadata`, `popup_metrics`, `popup_view`,
  `modal_view`, `panel_view`, `toast_view`, `runner_query`, `runner_entries`,
  `settings_schema`, `config_changed`, `view_with_context`, and
  `control_center_view`.
- `availability(config)` lets a plugin skip loading before model initialization, init tasks, and subscriptions when required hardware or system services are unavailable.
- `ABI_VERSION` in `oxibar-plugin-api` gates host/plugin compatibility.
- Plugin models are `Arc<RwLock<Box<dyn OxiAny>>>`; plugin messages are `Arc<dyn OxiAny>`.
- Plugin subscriptions return a raw pointer to a boxed stream. The host rebuilds it while keeping the dynamic library alive for function-pointer validity.
- `control_center_view(model)` has the ordinary `ViewFn` signature and returns
  inner content without a second full-height scroll surface. The capability is
  optional at ABI 6; existing popup/panel exports remain compatible fallbacks.

## Current Plugins

- `audio`: PulseAudio/PipeWire `pactl` plus MPRIS controls; popup surface.
- `battery`: Linux power-supply sysfs percentage, charging/draining state, and calendar-style hover tooltip with duration estimates.
- `bluetooth`: `bluetoothctl` scan, connect, disconnect, and pairing modal; popup and modal surfaces.
- `clock`: time display, local calendar popup with event-day tooltips, optional HTTPS CalDAV event sync, and configurable external calendar launcher. `tick_seconds` is the real-clock poll interval; when the format shows seconds the plugin still emits a `Tick` every second, simulating intermediate ticks from a monotonic anchor and only re-reading `Local::now()` every `tick_seconds`.
- `network`: NetworkManager `nmcli` connection management and password modal; popup and modal surfaces.
- `notifications`: Freedesktop notification server, toast layers, inline replies, DND state, and side panel. Toast layers start with keyboard interactivity disabled and are switched to `OnDemand` only after user hover intent so incoming notifications do not steal keyboard focus. New toast layers target Hyprland's focused monitor output when available, while the notification panel remains attached to the bar output. Toast timeout closes only the host toast surface; stored notifications remain in the panel until dismissed, cleared, replied to, or actioned.
- `tray`: StatusNotifier watcher, dynamically sized tray item popup, activation, and DBusMenu-backed detached context menu rendering. Item registration publishes a fallback row immediately and refreshes DBus metadata asynchronously so registering applications are not blocked by property queries.
- `workspaces`: Hyprland workspace display and dispatch.
- `applications`: ported OxiRun desktop-entry/icon indexing, fuzzy matching, terminal command support, and application launching; a resident runner provider rather than a bar/popup view. Indexing/search use immutable snapshots and runtime-independent background workers; stale query generations are discarded.

Plugin `lib.rs` files should keep ABI symbols, model update, and view composition. External-system logic and parsers should live in sibling modules once they grow beyond small helpers.

## Tradeoffs

- Several runtime integrations call external CLIs or DBus APIs directly; pure parsing/state helpers should be covered by unit tests, while live integration remains manual.
- The clock plugin's CalDAV sync shells out to `curl` to avoid adding a full HTTP stack to the dynamic plugin. It enforces `https://`, curl HTTPS-only protocol flags, and normal certificate verification.
- Thunderbird integration is limited to launching `thunderbird --calendar`; date-focused navigation remains a user-configured `calendar_command` concern because Thunderbird has no stable CLI for opening a specific calendar date.
- Per-output width/height/layout/transparency and top/bottom placement come from
  common bar settings plus named profiles. Geometry is in logical coordinates;
  the layer-shell backend handles native compositor buffer/fractional scaling.
- Active-monitor toast placement uses a modular compositor-backend registry. The current backend uses Hyprland monitor data because Wayland/layer-shell and the available `wayland-protocols` output protocols describe/configure outputs but do not expose a compositor-generic focused-output query; non-Hyprland sessions fall back to the previous `LastOutput` behavior.
- The stream raw-pointer ABI is documented as practical but not fully C-ABI-safe.
- Dynamic plugins cannot assume the host's Tokio runtime is visible through their own copy of Tokio TLS. The runner uses worker-thread/oneshot tasks and a cancellable timer whose worker is joined before drop.

## Shell Expansion

`SHELL_ROADMAP.md` defines the proposed Rust/Iced widget shell, compatible plugin
capability extensions, and source-aware GUI overrides over immutable Nix defaults.
Source-aware overrides, structured control commands and the initial settings UI
are implemented, together with the first multi-output registry/inventory slice
and control-center composition. Settings/control-center layouts share spacious
oxiced controls while retaining the existing palette. Native audio/BlueZ reuse,
OSDs, dock/desktop widgets and live theme snapshots remain subsequent milestones.

The roadmap also identifies selective ReSet native audio/BlueZ and monitor
backend reuse. These are planned adapters within existing plugin/service
boundaries, preserving the Rust/Iced host and plugin API.

## Nix Packaging

- `inputs.crane` provides Cargo derivations for Nix builds.
- `flake.nix` creates one cleaned Cargo source, one vendored dependency set, and one `buildDepsOnly` workspace artifact derivation with `--workspace` package selection.
- `nix/default.nix` builds only the host package from those shared Cargo artifacts.
- `nix/plugin.nix` builds only the requested plugin package from the same shared Cargo artifacts and installs the resulting `lib*.so` under `$out/lib/`.
- This reduces rebuild time when multiple plugins are updated because dependency crates such as `iced`, `oxiced`, `zbus`, and `oxibar-plugin-api` are compiled once per dependency graph instead of once per plugin derivation. Final plugin crate compilation and linking still happen per plugin.
