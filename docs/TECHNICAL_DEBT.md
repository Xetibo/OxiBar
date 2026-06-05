# Technical Debt

This file tracks accepted deviations from the current UI contract in `UI.md`. These are allowed only because `oxiced` does not yet provide the needed shared component or semantic token.

## UI Theme Debt

### Missing Flat Button Variants In `oxiced`

- Status: accepted temporary exception.
- Affected code: `plugins/audio/src/lib.rs`, `plugins/bluetooth/src/lib.rs`, `plugins/network/src/lib.rs`, `plugins/notifications/src/lib.rs`, `plugins/tray/src/lib.rs`, `plugins/workspaces/src/lib.rs`.
- Current state: plugins either duplicate local flat button styles or use `oxi_button::button`, whose default style includes a small shadow.
- Why accepted: `oxiced::widgets::oxi_button` does not currently expose a shadowless compact/bar variant or flat row-action variant.
- Target fix: add shadowless `oxiced` button variants for bar controls, compact actions, and row actions; migrate plugins to those helpers.

### Duplicated Bar Button Styling

- Status: accepted temporary exception.
- Affected code: `plugins/audio/src/lib.rs`, `plugins/bluetooth/src/lib.rs`, `plugins/clock/src/lib.rs`, `plugins/network/src/lib.rs`, `plugins/notifications/src/lib.rs`, `plugins/tray/src/lib.rs`.
- Current state: each plugin defines a near-identical bar button style using `OXITHEME.primary`, `OXITHEME.primary_bg_hover`, and `OXITHEME.primary_bg_active`.
- Why accepted: `oxiced` does not currently provide an Oxibar bar control helper with 22.5 height, `[0, 8]` padding, transparent idle background, and theme hover/pressed states.
- Target fix: add an `oxiced` bar button helper or style function, then replace plugin-local copies.

### Missing Compact List Row/Card Helpers

- Status: accepted temporary exception.
- Affected code: `plugins/audio/src/lib.rs`, `plugins/bluetooth/src/lib.rs`, `plugins/network/src/lib.rs`, `plugins/notifications/src/lib.rs`, `plugins/tray/src/lib.rs`.
- Current state: plugins build compact row/card containers with local `container::Style` or `button::Style` closures while still using `OXITHEME` colors.
- Why accepted: `oxiced::widgets::oxi_card` is a general card abstraction, not a compact popup list-row/card primitive with the padding, hover, and radius rules from `UI.md`.
- Target fix: add compact row/card helpers to `oxiced` for popup lists, selectable rows, and accent cards.

### Missing Text Role Helpers

- Status: accepted temporary exception.
- Affected code: most plugin popup/panel/modal views.
- Current state: plugins repeat local text style closures for primary, muted, section, and title text.
- Why accepted: `oxiced` exposes theme colors but not reusable text role helpers for `primary`, `text`, and `text_muted` styling.
- Target fix: add `oxiced` text helpers or style functions for title, section, primary, muted, and empty-state text.

### Calendar Adjacent-Month Color

- Status: accepted temporary exception.
- Affected code: `plugins/clock/src/lib.rs`.
- Current state: adjacent-month days use `Color { a: 0.45, ..OXITHEME.primary }`.
- Why accepted: `oxiced` does not currently provide a semantic token for disabled accent text or adjacent-calendar text.
- Target fix: add a semantic disabled/accent-muted text color to `oxiced` or use a new calendar helper once available.

### Workspace Pill Styling

- Status: accepted temporary exception.
- Affected code: `plugins/workspaces/src/lib.rs`.
- Current state: workspace buttons use a custom 22.5x22.5 pill style with active/inactive theme colors and a small shadow.
- Why accepted: `oxiced` does not currently provide a compact workspace/pill/bar-item helper that supports active, hover, pressed, and fixed square sizing.
- Target fix: add a compact pill/bar-item helper to `oxiced`; remove local workspace style and shadow.
