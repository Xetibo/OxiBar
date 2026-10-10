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
- `plugins/*/src/system.rs`: command/DBus/sysfs integration, parsing, and domain
  data extracted from plugin ABI/UI files. Audio additionally separates its native
  PulseAudio owner in `native.rs` and runtime-independent job deadlines in `worker.rs`.
- Bluetooth separates cached DTO reduction in `system.rs`, native DBus ownership
  in `native.rs`, the pairing agent in `agent.rs`, model transitions in `runtime.rs`
  and themed content in `views.rs`.
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
- Modal retirement invokes an optional provider `modal_closed` hook once for that
  instance, including Escape, replacement, compositor closure and output removal.
  Nonblocking cleanup tasks still update the model after their window disappears.
  Modal Escape and text-field traversal are scoped to the current modal ID.
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

## Native Audio Ownership

The Audio plugin defaults to one native `libpulse-binding` context/mainloop for
PulseAudio or PipeWire's PulseAudio-compatible server. The subscription owns the
worker; dropping it wakes and joins the worker. Thread-affine handles stay on
that thread, and a bounded request queue plus Unix wake socket bridges Iced tasks
to it without host Tokio TLS or unsafe thread markers.

Sink/source/server events coalesce into device snapshots queried through the same
context. Cached indexes and channel volumes support default-device, volume and
mute actions; volume scaling preserves channel balance. Monitor sources are
filtered by protocol metadata. Normal device refresh does not invoke `pactl` or
run a periodic device scan. Commands are bounded, expire while queued, and are
rejected offline rather than replayed after reconnect. Errors retain the previous
snapshot; reconnect resubscribes and queries authoritative state.

Device snapshots and MPRIS state are independent model domains. Media properties
still refresh at `audio.poll_seconds`, using bounded asynchronous zbus calls and a
cancellable async-io timer. Per-domain action generations and media query generations
reject stale results; a device action cannot suppress a completed media refresh.
All bar, popup and control-center views share this model and subscription. Explicit
`audio.backend = "pactl"` retains the compatibility adapter on background workers;
backend and refresh-interval changes require restart. Card profiles, per-stream
routing and event-driven MPRIS remain follow-ups.

## Native Bluetooth Ownership

The Bluetooth plugin owns one asynchronous zbus system-bus connection on a
subscription-owned worker. A weak client bridges the no-model-argument subscription
ABI; dropping the stream cancels and joins the owner without host Tokio TLS.
ObjectManager supplies initial state; interface and property signals update the
object-path cache. Invalidated properties resnapshot. Reply sequence watermarks
avoid replaying pre-snapshot events, and BlueZ's unique owner identifies each epoch.
Commands target that owner, expire while queued and are rejected offline. Reconnect
registers a fresh agent and resnapshots; cached content remains visible meanwhile.

Adapter power/selection, connect/disconnect, trust/remove and bounded discovery
share this owner. Discovery tracks only sessions started by this connection;
timeout/cleanup releases those sessions without stopping another client's scan.
Configured plugins stay loaded with empty/offline state so service and hardware
hotplug can recover. Normal inventory/actions do not run `bluetoothctl` or poll.

The `KeyboardDisplay` Agent1 handles PIN/passkey input and display, numeric
confirmation, authorization and service authorization. Calls must come from the
current BlueZ owner for the active user-initiated device. Typed replies wait for
the UI, reject invalid/stale answers, and never fabricate codes or auto-accept.
The plugin registers its own agent without taking global default-agent ownership.

Pairing events travel in the originating task stream, preserving modal output
routing. Pair, trust and connect form one deadline-bound operation. Closing the
modal cancels the pending reply and protocol operation; cancellation during Connect
also disconnects that new connection. Session IDs prevent old agent requests or
queued prompts from cancelling/reopening a newer operation. Discovery and pairing
limits are restart-only settings. General incoming pairing and additional adapter
capabilities remain follow-ups.

## Plugin ABI

- Required symbols: `abi_version`, `name`, `model`, `update`, `launch`, `view`, `errors`, and `subscription`.
- Optional symbols: `availability`, `metadata`, `popup_metrics`, `popup_view`,
  `modal_view`, `panel_view`, `toast_view`, `runner_query`, `runner_entries`,
  `settings_schema`, `config_changed`, `view_with_context`, and
  `control_center_view`, and `modal_closed`.
- `availability(config)` lets a plugin skip loading before model initialization, init tasks, and subscriptions when required hardware or system services are unavailable.
- `ABI_VERSION` in `oxibar-plugin-api` gates host/plugin compatibility.
- Plugin models are `Arc<RwLock<Box<dyn OxiAny>>>`; plugin messages are `Arc<dyn OxiAny>`.
- Plugin subscriptions return a raw pointer to a boxed stream. The host rebuilds it while keeping the dynamic library alive for function-pointer validity.
- `control_center_view(model)` has the ordinary `ViewFn` signature and returns
  inner content without a second full-height scroll surface. The capability is
  optional at ABI 6; existing popup/panel exports remain compatible fallbacks.
- `modal_closed(model)` optionally returns a cleanup task. It is idempotent and
  nonblocking; returned work must remain valid after surface retirement. Required
  exports/signatures and ABI 6 remain unchanged.

## Current Plugins

- `audio`: native PulseAudio/PipeWire device/default/volume/mute events and actions,
  asynchronous MPRIS controls, and explicit `pactl` compatibility; shared bar,
  scrollable popup and control-center content.
- `battery`: Linux power-supply sysfs percentage, charging/draining state, and calendar-style hover tooltip with duration estimates.
- `bluetooth`: native BlueZ adapter/device events, owned discovery, power,
  connect/disconnect, trust/remove and complete cancellable Agent1 pairing; shared
  bar, popup, control-center and task-modal content.
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
- Dynamic plugins cannot assume the host's Tokio runtime is visible through their
  own copy of Tokio TLS. Runner workers and native audio/BlueZ owners are cancelled
  and joined by their owning streams; asynchronous DBus/media uses async-io/zbus.

## Shell Expansion

`SHELL_ROADMAP.md` defines the proposed Rust/Iced widget shell, compatible plugin
capability extensions, and source-aware GUI overrides over immutable Nix defaults.
Source-aware overrides, structured control commands and the initial settings UI
are implemented, together with the first multi-output registry/inventory slice
and control-center composition. Native audio and BlueZ integration are implemented;
settings/control-center layouts share spacious oxiced controls while retaining
the existing palette. Additional audio/adapter capabilities, OSDs, dock/desktop
widgets and live theme snapshots remain subsequent milestones.

The roadmap identifies selective ReSet audio/BlueZ and monitor backend patterns.
Audio, BlueZ and read-only monitor adapters follow these boundaries.
No donor daemon/GTK architecture is required by the Rust/Iced host or plugin API.

## Nix Packaging

- `inputs.crane` provides Cargo derivations for Nix builds.
- `flake.nix` creates one cleaned Cargo source, one vendored dependency set, and one `buildDepsOnly` workspace artifact derivation with `--workspace` package selection.
- `nix/default.nix` builds only the host package from those shared Cargo artifacts.
- `nix/plugin.nix` builds only the requested plugin package from the same shared Cargo artifacts and installs the resulting `lib*.so` under `$out/lib/`.
- Native dependencies must be available to shared dependency compilation and the
  owning final plugin. `libpulseaudio` is in the Audio library's runtime search
  path; other plugins do not gain that plugin-specific runtime dependency.
- This reduces rebuild time when multiple plugins are updated because dependency crates such as `iced`, `oxiced`, `zbus`, and `oxibar-plugin-api` are compiled once per dependency graph instead of once per plugin derivation. Final plugin crate compilation and linking still happen per plugin.
