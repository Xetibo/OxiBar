# UI Guidelines

`UI.md` at the repository root is the full styling contract. This file exists in `docs/` because global agent rules require docs-local UI guidance.

## Rules

- Use `oxiced::theme::theme_impl::OXITHEME` for colors and theme-derived styling.
- Prefer shared `oxiced` widgets when available.
- Use `oxiced::widgets::oxi_plugin::bar_button` for standard plugin bar controls.
- Use `oxiced::widgets::oxi_plugin` text role helpers before adding local text style closures.
- Keep bar controls 22.5 units high, text size `14`, horizontal padding `[0, 8]`, and transparent idle background.
- Popup roots should use compact spacing, padding `[12, 14]`, `Length::Fill`, and scrolling for long content.
- Panel roots should use padding `[14, 14]`, full width/height, and scrolling for long lists.
- Modal content should rely on host-provided chrome, radius, shadow, border, and padding.
- Notification cards are interactive: card click invokes the notification's primary action or dismisses when no action exists; inline reply inputs must remain focusable in toast and panel layers; hover/focus prevents toast expiry.
- Tray rows use left-click activation and right-click context menus. Do not add separate per-row action buttons for secondary/context actions.
- Detached popup overlays may request a larger transparent input region than their visible popup body; keep visible chrome compact and use the extra region only for overlay hit-testing.
- Do not introduce arbitrary palettes or random RGB colors. If `oxiced` cannot express a needed style, track the exception in `docs/TECHNICAL_DEBT.md`.

## Current Surfaces

- Main bar background uses `OXITHEME.mantle` unless `[bar] transparent = true`.
- Popup, modal, and panel host surfaces use `OXITHEME.mantle`.
- Plugins request host surfaces through `HOST_REQUEST_TOGGLE_POPUP`, `HOST_REQUEST_OPEN_MODAL`, `HOST_REQUEST_CLOSE_MODAL`, and `HOST_REQUEST_TOGGLE_PANEL`.
