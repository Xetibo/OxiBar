# Technical Debt

This file tracks accepted deviations from the current UI contract in `UI.md`. These are allowed only because `oxiced` does not yet provide the needed shared component or semantic token.

It also tracks non-UI runtime/testing limitations that affect future plugin work.

## Runtime Debt

### Static Theme and Remaining Restart-only Settings

- Status: settings foundation resolved; theme/lifecycle extensions remain planned.
- Current state: source-aware snapshots, sparse GUI overrides, atomic persistence,
  filesystem reload and optional plugin config hooks are implemented. Fonts,
  plugin lifecycle and existing poll timers remain restart-only; `OXITHEME` is
  independently static in host/plugin libraries.
- Target fix: cancellable plugin lifecycle and immutable theme snapshots as
  sequenced in `SHELL_ROADMAP.md`. Add live hooks to further bundled settings as
  their timer/backend ownership supports them.

### Launcher Index and Desktop-entry Coverage

- Status: accepted initial limitations.
- Current state: application/icon indexing runs off the UI thread at startup and every configured refresh interval; reopening does not scan. The provider supports XDG precedence, hidden-entry masking, normal Exec fields, terminal commands and desktop visibility filters, but default-language names, hicolor/Adwaita/pixmap icon lookup, and Exec launching remain the supported subset.
- Gaps: locale-aware names, icon-theme inheritance, DBusActivatable-only applications, desktop actions, persistent/mtime-based caches and filesystem-driven refresh are not implemented. Search results discard stale generations but old computations are not coalesced/cancelled; large configured result lists do not automatically scroll the keyboard selection into view.
- Target fix: extend the resident provider and overlay navigation incrementally; do not reintroduce per-invocation scans or full application-vector cloning per query.

### Native System Backend Migration

- Status: planned, with existing integrations retained as migration baselines.
- Current state: audio and Bluetooth still use `pactl`/`bluetoothctl` for substantial device interaction. Reviewed ReSet backends provide native PulseAudio subscriptions, BlueZ object/property events and broader monitor adapters suitable for selective adaptation.
- Migration requirements: cancellable/reconnecting backend owners, state/action parity, complete pairing-agent responses and protocol/version-aware display capabilities. The donor's placeholders, blocking waits, unsafe thread markers and coarse refresh rounding must be addressed rather than imported.
- Target fix: the ReSet reuse workstream in `SHELL_ROADMAP.md`; keep existing plugin capabilities and themed Iced views throughout migration.

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

- Status: accepted temporary limitation.
- Affected code: `src/plugins.rs` and all dynamic plugin crates.
- Current state: the host supports optional `availability(config)` gates and unit-tests the pure plugin availability probes, but does not run an integration test that builds temporary dylibs and verifies skip-before-model behavior through `load_plugins()`.
- Target fix: add a small dynamic-loading integration test once plugin test fixtures can be built reproducibly through the workspace/Nix setup.

## UI Theme Debt

### Settings Control Accessibility

- Status: follow-up for the initial settings UI.
- Current state: keyboard search, text-field traversal, Enter/Apply, category
  shortcuts and window-scoped Escape work. Generic Iced button/checkbox keyboard
  focus and screen-reader semantics are not yet uniformly exposed by shared controls.
- Target fix: reusable focusable/accessible controls in `crates/oxiced`, with
  full form navigation and runtime checks as the settings pages expand.

### Selectable Search Result Row

- Status: resolved.
- Current state: launcher and settings navigation use the workspace `oxiced`
  selectable-row helper, sharing neutral/selected/hover/pressed states.

### Missing Additional Flat Button Variants In `oxiced`

- Status: partially resolved.
- Affected code: popup row actions in `plugins/bluetooth/src/lib.rs`, `plugins/network/src/lib.rs`, `plugins/notifications/src/lib.rs`, `plugins/tray/src/lib.rs`, plus workspace pills in `plugins/workspaces/src/lib.rs`.
- Current state: `oxiced::widgets::oxi_plugin` provides shadowless bar buttons and flat background styles. Some popup row/action buttons still use local styles or `oxi_button::button`.
- Target fix: migrate remaining compact popup row/action buttons to `oxi_plugin` helpers or add more helper variants as needed.

### Duplicated Bar Button Styling

- Status: resolved.
- Current state: `plugins/audio/src/lib.rs`, `plugins/bluetooth/src/lib.rs`, `plugins/clock/src/lib.rs`, `plugins/network/src/lib.rs`, `plugins/notifications/src/lib.rs`, and `plugins/tray/src/lib.rs` use `oxiced::widgets::oxi_plugin::bar_button` or `bar_button_style`.

### Missing Compact List Row/Card Helpers

- Status: accepted temporary exception.
- Affected code: `plugins/audio/src/lib.rs`, `plugins/bluetooth/src/lib.rs`, `plugins/network/src/lib.rs`, `plugins/notifications/src/lib.rs`, `plugins/tray/src/lib.rs`.
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

### Workspace Pill Styling

- Status: accepted temporary exception.
- Affected code: `plugins/workspaces/src/lib.rs`.
- Current state: workspace buttons use a custom pill style sized from `oxi_plugin::BAR_CONTROL_HEIGHT`, active/inactive theme colors, and a small named shadow.
- Why accepted: `oxiced` does not currently provide a compact workspace/pill/bar-item helper that supports active, hover, pressed, and fixed square sizing.
- Target fix: add a compact pill/bar-item helper to `oxiced`; remove local workspace style and shadow.

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
