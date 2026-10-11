# Technical Debt

## Dock and Desktop Extensions

- Initial dock/widgets are implemented and verified. Running-window activation is
  currently a Hyprland adapter; other compositors retain pinned launches and
  output-aware widgets but need a window/focus capability adapter.
- Dock auto-hide and weather are optional extensions, not enabled implementations.
- Generic output inventory still has its documented fallback polling/focus limits.
  The shared Hyprland window/workspace broker is event-driven.
- Legacy providers default to resident subscriptions. Clock/Stats opt into owned
  visible-only timers; Network/Battery use owned SDK intervals. SDK, loader, private
  app lifetime, compositor smoke and release measurements are now verified.

This file tracks accepted deviations from the current UI contract in `UI.md`. These are allowed only because `oxiced` does not yet provide the needed shared component or semantic token.

It also tracks non-UI runtime/testing limitations that affect future plugin work.

## Runtime Debt

### Theme Compatibility and Remaining Restart-only Settings

- Status: live immutable snapshots and bundled theme hooks implemented.
- Palette/font/density/motion preferences apply live; legacy `bar.font`, plugin
  lifecycle and existing backend timers keep explicit restart classification.
  Old providers without the optional hook retain their seed palette.
- Compatibility field access interns at most 128 immutable palette/font values
  per library; exhausting that budget requires restart rather than unbounded leaks.
  Wallpaper-derived colors remain an optional extension.
- Further live timer hooks depend on owned resource lifecycles; the release SDK
  gate documents those rules rather than attempting unsafe dylib unloading.

### Launcher Index and Desktop-entry Coverage

- Status: localized fields/actions, inherited icons, filesystem refresh, usage/
  favorites and coalesced cancellable searches implemented.
- Remaining gaps: DBusActivatable-only entries and a persistent mtime index cache.
  Large configured result lists do not yet scroll keyboard selection automatically.
  Tools' file/command candidates skip non-UTF8 paths rather than lossy retargeting.
- Clipboard text is bounded, opt-in and ephemeral; non-text/oversized selections
  are not captured. Compositors without data-control keep other Tools providers
  usable but cannot provide native clipboard copy/history.
- Preserve shared indexes and stable immutable actions when extending coverage.

### Native System Backend Migration

- Status: native audio device and BlueZ integration implemented.
- Current state: audio defaults to one native PulseAudio/PipeWire context with
  device/default/volume/mute events and actions, bounded commands, reconnect and
  stream-owned cancellation. Explicit `pactl` compatibility remains available.
  Bluetooth uses one native ObjectManager/property owner with complete typed,
  cancellable pairing and bounded discovery. Control-center views reuse these owners.
  Reviewed ReSet backends provide BlueZ object/property events and broader audio/
  monitor capabilities suitable for selective adaptation.
- Remaining requirements: richer capabilities and protocol/version-aware display
  settings. Keep cancellable/reconnecting ownership and proven state/action parity;
  donor placeholders, blocking waits and unsafe thread markers are not reused.
- Target fix: the ReSet reuse workstream in `SHELL_ROADMAP.md`; keep existing plugin capabilities and themed Iced views throughout migration.

### Remaining Audio Capabilities

- Status: accepted initial native-backend limits.
- Current state: sink/source/server events trigger coalesced full device snapshots,
  not incremental per-object queries. MPRIS uses bounded asynchronous DBus calls
  and cancellable periodic refresh. The compatibility adapter still runs `pactl`.
- Gaps: card profiles/ports, per-application stream routing and event-driven media
  properties are not implemented. PulseAudio protocol parity was verified against
  a private server; a separate PipeWire server fixture remains a follow-up.
- Target fix: extend the existing owner with typed card/stream capabilities and
  media signals when needed; avoid adding a service/poller for each surface.

### Remaining Bluetooth Capabilities and Hardware Verification

- Status: user-initiated native pairing implemented and privately verified.
- Current state: all Agent1 request/display/confirmation/authorization methods have
  typed responses, owner/device validation, deadlines and full-flow cancellation.
  The plugin registers its own agent without claiming the global default role.
- Gaps: unsolicited incoming pairing/default-agent policy, discoverability settings
  and further adapter/device capabilities remain extensions. Private mock-BlueZ
  tests cover multiple adapters, property invalidation, hotplug and reconnect;
  real hardware/BlueZ-version and system permission-policy checks remain manual.
- Target fix: extend the same owner and explicit UI policy rather than introducing
  another connection/agent per view or importing placeholder replies.

### Remaining Control-center Services and OSDs

- Status: initial milestone implemented.
- Current state: host-owned control-center selection, live preferences, source
  routing and existing audio/network/Bluetooth/battery/notification/calendar views
  work with one shared model/subscription per provider. System adds backlight,
  power-profile and verified session controls; host OSDs reuse cached provider state.
- Remaining extensions: additional driver-specific brightness devices, profile
  holds and richer native Audio/BlueZ capabilities. Actual hardware/system policy
  checks remain manual; private fixtures exercise ordinary files and mock services.
- Backlight refresh uses one watcher plus a bounded fallback poll because sysfs
  notification support varies. Remote profiles/session state remains event-driven.
- Keep domain health independent and avoid one backend/poller per surface.

### Layer-shell Keyboard Ownership

- Status: keyboard-stealing bug resolved locally.
- Published layershellev 0.17.1 routed keyboard events through its newest/pointer
  surface. Creating a non-interactive OSD redirected typing from existing settings.
- The local patch separates compositor keyboard Enter/Leave from pointer/creation
  state. Packaged checks prove typing and applying settings through an OSD.
- Repeated seat capabilities retain resources rather than rebinding/synthesizing
  unfocus; removed devices release resources and retired keyboard events are ignored.
  Private provider typing/copy/reopen checks passed with virtual-device startup
  delays in the fixture, without changing the user's input devices.
- Follow-up: upstream the correction and evaluate a released replacement before
  removing the local dependency; startup connection error propagation remains separate.

### Output Inventory Refresh and Remaining Shell Capabilities

- Status: initial multi-output slice implemented.
- Current state: explicit surface registry, per-output profiles, hotplug
  reconciliation and read-only Hyprland/Wayland inventory work. Inventory is
  refreshed on background workers at `monitor.refresh_seconds` (default 2), and
  unchanged snapshots do not redraw the UI.
- Gaps: fully event-driven inventory and further compositor focus adapters remain
  follow-ups. Generic Wayland output names/logical geometry do not provide global
  focus, VRR/HDR state or persistent display configuration. Compositors without
  output names retain compositor-selected placement.
- Target fix: event-driven output service and optional display-settings backends;
  do not conflate shell placement with display-mode/compositor ownership.

### Home Manager Plugin Path

- Status: resolved.
- Current state: base configuration and plugin libraries both use `xdg.configFile`;
  GUI overrides are excluded from Home Manager ownership. Nonstandard config homes
  were checked using Home Manager module evaluation.

### iced_layershell Startup Connection Panic

- Status: mitigated, not root-fixed.
- Affected code: `src/app.rs` `run()` / `run_bar()`, dependent on iced_layershell 0.17.1 and layershellev 0.17.1.
- Current state: when the compositor is not yet accepting clients, `layershellev::WindowState::build()` calls `Connection::connect_to_env()?` and iced_layershell -0.17.1 `multi_window.rs` turns that into `.expect("Cannot create layershell")`, a panic. The host works around this with a config-driven retry loop (pre-flight `connect_to_env()` probe plus `catch_unwind` around `run_bar()`), so transient startup failures retry with exponential backoff instead of exiting.
- Why accepted: a real fix requires surfacing connection errors through iced_layershell's `Error` enum instead of panicking.
- Target fix: patch or update `iced_layershell`/`layershellev` so `WindowState::build()` connection failures return an `Error` variant rather than panicking; then the probe/`catch_unwind` workaround can be simplified or dropped. Tracked as a future option pending an upstream or vendored dependency update.

### Compositor-Specific Active Toast Output

- Status: accepted temporary limitation.
- Affected code: `src/monitor/` and `src/messages.rs`.
- Current state: toast layer creation asks the `src/monitor/` backend registry for an active output name. The only backend currently queries Hyprland's focused monitor name and passes it to layer-shell as `OutputOption::OutputName`; if no backend returns an output name, toasts fall back to `OutputOption::LastOutput`.
- Why accepted: the available `wayland-protocols` output protocols describe/configure outputs and surfaces, but do not provide a compositor-neutral global focused-monitor query for layer-shell clients.
- Target fix: switch to a compositor-neutral focused-output source if `iced_layershell`, layer-shell integration, or a shared workspace/monitor service exposes one.

### Hardcoded Bar Width

- Status: resolved.
- Current state: the main layer requests automatic width from the compositor-selected active output with layer-shell width `0` and left/right/top anchors. `OxiBar` stores the configured width from iced window events, and `[bar] width` / `[bar] height` provide config fallback/override values.

### Missing Dynamic Plugin Loading Integration Test

- Status: resolved with the Nix-built SDK example.
- Actual loader tests verify metadata inspection does not initialize models,
  availability skips before factories and duplicate names instantiate once.
  The real example contract exercises 25 shared-model/task/view/stream/theme/config/
  modal cycles without a host Tokio reactor; initialized code remains process-resident.

### Release and Application Lifetime Tradeoffs

- Named Sway/headless release gates passed actual presentation, bounded local query,
  idle CPU/draws and reopen RSS/workers. The record is workload-specific; all-thread
  voluntary context switches are a wakeup proxy, and RSS uses a documented 2 MiB
  noise budget. See `RELEASE_CHECKS.md` and `performance-reference.json`.
- Nested Hyprland 0.51.1 did not advertise its presentation clock and returned stale
  feedback; it is used for owned UI smoke, not the presentation reference.
- Successful plugin code is retained until process exit for opaque/deferred Rust
  vtable safety. Existing notification/tray bus servers remain process-lifetime
  providers; safe generic in-process unloading is not part of ABI 6.
- Native user-manager app handoff escapes the shell termination cgroup where
  supported. Direct fallback creates a reaped process group but cannot escape an
  existing systemd cgroup without a user manager.

## UI Theme Debt

### Settings and Control-center Accessibility

- Status: follow-up for the initial settings UI.
- Current state: keyboard search, text-field traversal, Enter/Apply, category/tab
  shortcuts and window-scoped Escape work. Generic Iced button/switch keyboard
  focus and screen-reader semantics are not yet uniformly exposed by shared controls.
- Target fix: reusable focusable/accessible controls in `crates/oxiced`, with
  full form navigation and runtime checks as the settings pages expand.

### Selectable Search Result Row

- Status: resolved.
- Current state: launcher and settings navigation use the workspace `oxiced`
  selectable-row helper, sharing neutral/selected/hover/pressed states.

### Missing Additional Flat Button Variants In `oxiced`

- Status: partially resolved.
- Affected code: popup row actions in `plugins/network/src/lib.rs`, `plugins/notifications/src/lib.rs`, `plugins/tray/src/lib.rs`, plus workspace pills in `plugins/workspaces/src/lib.rs`.
- Current state: `oxiced::widgets::oxi_plugin` provides shadowless bar buttons,
  normal-sized surface buttons, source/mode badges and flat background styles;
  themed switches are shared through `oxi_toggler`. Settings/control-center actions
  use these helpers. Some compact popup row/action buttons still use local styles
  or `oxi_button::button`.
- Target fix: migrate remaining compact popup row/action buttons to `oxi_plugin` helpers or add more helper variants as needed.

### Duplicated Bar Button Styling

- Status: resolved.
- Current state: `plugins/audio/src/lib.rs`, `plugins/bluetooth/src/views.rs`, `plugins/clock/src/lib.rs`, `plugins/network/src/lib.rs`, `plugins/notifications/src/lib.rs`, and `plugins/tray/src/lib.rs` use `oxiced::widgets::oxi_plugin::bar_button` or `bar_button_style`.

### Missing Compact List Row/Card Helpers

- Status: accepted temporary exception.
- Affected code: `plugins/audio/src/lib.rs`, `plugins/bluetooth/src/views.rs`, `plugins/network/src/lib.rs`, `plugins/notifications/src/lib.rs`, `plugins/tray/src/lib.rs`.
- Current state: plugins build compact row/card containers with local `container::Style` or `button::Style` closures while still using `OXITHEME` colors and spacing/font/radius tokens where available. `audio` media/device cards now use `oxi_plugin::compact_card`.
- Current state: `oxiced::widgets::oxi_plugin` includes `compact_card` and `compact_card_style`, but remaining plugin-local list-row styles have not all been migrated yet.
- Target fix: migrate popup list-row/card wrappers to `oxi_plugin` helpers.

### Missing Text Role Helpers

- Status: partially resolved.
- Affected code: most plugin popup/panel/modal views.
- Current state: `oxiced::widgets::oxi_plugin` provides `text_primary`, `text_muted`, and `text_accent`; several plugin-local title/section closures remain.
- Target fix: migrate remaining section/title text closures and add more semantic text helpers only where needed.

### Calendar Adjacent-Month Color

- Status: accepted temporary exception.
- Affected code: `plugins/clock/src/lib.rs`.
- Current state: adjacent-month days use a named local alpha constant with `OXITHEME.primary`.
- Why accepted: `oxiced` does not currently provide a semantic token for disabled accent text or adjacent-calendar text.
- Target fix: add a semantic disabled/accent-muted text color to `oxiced` or use a new calendar helper once available.

### Missing Shared Tooltip Chrome Helper

- Status: accepted temporary exception.
- Affected code: `plugins/clock/src/lib.rs`, `plugins/battery/src/lib.rs`.
- Current state: clock calendar event days and the battery bar item use local themed tooltip container styles so hover details share the same chrome.
- Why accepted: `oxiced` does not currently provide a shared plugin tooltip helper.
- Target fix: add an `oxi_plugin` tooltip chrome helper and migrate local tooltip styles to it.

### Tray DBusMenu Completeness

- Status: accepted temporary limitation.
- Affected code: `plugins/tray/src/lib.rs`, `plugins/tray/src/system.rs`.
- Current state: tray rows render `com.canonical.dbusmenu` `GetLayout` entries in a right-click context menu and dispatch selected entries through `Event(id, "clicked", ...)`. Items without DBusMenu fall back to StatusNotifier `Activate`, `SecondaryActivate`, and `ContextMenu` actions. StatusNotifier registration returns immediately with a fallback item and refreshes metadata asynchronously.
- Limitation: advanced DBusMenu properties such as icons, keyboard shortcuts, toggle/check/radio state, and lazy submenu refresh are not rendered yet.
- Target fix: extend DBusMenu rendering to cover icons, shortcuts, toggles, and `AboutToShow` submenu refresh behavior.

### Clock CalDAV Transport

- Status: accepted temporary implementation detail.
- Affected code: `plugins/clock/src/caldav.rs`.
- Current state: CalDAV sync shells out to `curl`, requires `https://`, forces HTTPS protocol use, and stores credentials in a temporary `0600` curl config rather than command-line arguments.
- Target fix: consider a native minimal HTTP/TLS client if the workspace adopts a stable HTTP dependency.

### Clock Fractional-Second Display

- Status: accepted temporary limitation.
- Affected code: `plugins/clock/src/lib.rs`.
- Current state: the display granularity floor is one second. Formats that show sub-second precision (`%f`, `%.3f`) still update at 1 Hz, so fractional digits are anchored to the last re-read of the real clock rather than tracked continuously.
- Target fix: add sub-second simulation granularity or an explicit precision config if fractional-second display is ever required.
