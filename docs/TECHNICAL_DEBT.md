# Technical Debt

This file tracks accepted deviations from the current UI contract in `UI.md`. These are allowed only because `oxiced` does not yet provide the needed shared component or semantic token.

It also tracks non-UI runtime/testing limitations that affect future plugin work.

## Runtime Debt

### Hardcoded Bar Width

- Status: resolved.
- Current state: the main layer requests automatic width from the compositor-selected active output with layer-shell width `0` and left/right/top anchors. `OxiBar` stores the configured width from iced window events, and `[bar] width` / `[bar] height` provide config fallback/override values.

### Missing Dynamic Plugin Loading Integration Test

- Status: accepted temporary limitation.
- Affected code: `src/plugins.rs` and all dynamic plugin crates.
- Current state: the host supports optional `availability(config)` gates and unit-tests the pure plugin availability probes, but does not run an integration test that builds temporary dylibs and verifies skip-before-model behavior through `load_plugins()`.
- Target fix: add a small dynamic-loading integration test once plugin test fixtures can be built reproducibly through the workspace/Nix setup.

## UI Theme Debt

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
