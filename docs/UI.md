# UI Guidelines

`UI.md` at the repository root is the full styling contract. This file exists in `docs/` because global agent rules require docs-local UI guidance.

## Rules

- Use `oxiced::theme::theme_impl::OXITHEME` for colors and theme-derived styling.
- Prefer shared `oxiced` widgets when available.
- Use `oxiced::widgets::oxi_plugin::bar_button` for standard plugin bar controls.
- Use `oxiced::widgets::oxi_plugin` text role helpers before adding local text style closures.
- Keep bar controls at `oxi_plugin::BAR_CONTROL_HEIGHT`, use `OXITHEME.font_md` text, `OXITHEME.padding_sm` horizontal padding, and transparent idle background. Bar edge padding should use `OXITHEME.padding_sm`; bar labels that combine icons with counts or values should use `OXITHEME.padding_xs` between icon and text.
- Informational bar controls that need details on hover should use the same follow-cursor tooltip pattern as the clock calendar.
- Popup roots should use compact `OXITHEME.padding_sm` spacing, `OXITHEME.padding_md`/`padding_lg` padding, `Length::Fill`, and scrolling for long content.
- Plugins with model-dependent popup content should export `popup_metrics(model)` and cap long content at the host max height with scrolling.
- Panel roots should use `OXITHEME.padding_md` padding, full width/height, and scrolling for long lists.
- Modal content should rely on host-provided chrome, radius, shadow, border, and padding.
- Notification cards are interactive: card click invokes the notification's primary action or dismisses when no action exists; toast layers must not take keyboard focus when they appear; new toast layers should open on the focused Hyprland monitor when available; inline reply inputs become focusable after hover/click intent and remain focusable in toast and panel layers; hover/focus prevents toast expiry.
- Tray rows use left-click activation and right-click context menus. Do not add separate per-row action buttons for secondary/context actions.
- Detached popup overlays may request a larger transparent input region than their visible popup body through `popup_metrics(model)` or metadata; keep visible chrome compact and use the extra region only for overlay hit-testing.
- Do not introduce arbitrary palettes or random RGB colors. If `oxiced` cannot express a needed style, track the exception in `docs/TECHNICAL_DEBT.md`.

## Current Surfaces

- Main bar background uses `OXITHEME.mantle` unless `[bar] transparent = true`; the bar spans the compositor-selected active output width unless `[bar] width` overrides it.
- Popup, modal, and panel host surfaces use `OXITHEME.mantle`.
- Notification panel host surface remains on the bar output; toast host surfaces target the active monitor independently.
- Popup open/close animation uses quick `Easing::EASE_OUT` timing to align with notification-center layer opening.
- Plugins request host surfaces through `HOST_REQUEST_TOGGLE_POPUP`, `HOST_REQUEST_OPEN_MODAL`, `HOST_REQUEST_CLOSE_MODAL`, and `HOST_REQUEST_TOGGLE_PANEL`.
