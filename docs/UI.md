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
- Interactive protocol modals distinguish input, displayed codes and explicit
  Allow/Reject decisions. Validate without discarding the pending prompt, keep
  actions reachable by scrolling, and show progress until the operation finishes.
  Escape/host dismissal cancels through the optional `modal_closed` capability;
  submitting an answer must not prematurely close its ongoing protocol operation.
- Notification cards are interactive: card click invokes the notification's primary action or dismisses when no action exists; toast layers must not take keyboard focus when they appear; new toast layers should open on the focused Hyprland monitor when available; inline reply inputs become focusable after hover/click intent and remain focusable in toast and panel layers; hover/focus prevents toast expiry.
- Tray rows use left-click activation and right-click context menus. Do not add separate per-row action buttons for secondary/context actions.
- Detached popup overlays may request a larger transparent input region than their visible popup body through `popup_metrics(model)` or metadata; keep visible chrome compact and use the extra region only for overlay hit-testing.
- Do not introduce arbitrary palettes or random RGB colors. If `oxiced` cannot express a needed style, track the exception in `docs/TECHNICAL_DEBT.md`.
- Search/task overlays take exclusive keyboard focus only while visible. Scope navigation to the overlay window; Escape and activation dismiss the overlay rather than terminate the daemon. Selected rows must differ from hover, and long result lists must scroll.
- Settings fields display their effective value, base/default value, source and
  live/surface/restart classification, with individual reset actions. Invalid edits
  remain visible without losing the running configuration. Continuous text edits
  are drafts; Enter/Apply accepts them. Busy operations preserve layout.
- Use shared selectable rows, settings cards, switch/pick-list/input helpers and
  themed editor styling from the workspace `oxiced` library.
- Settings and control-center surfaces use Android-inspired grouped cards and
  generous spacing within the existing palette: `padding_xl` outer/card padding,
  `padding_lg`/`padding_xl` group gaps, `font_xxl` page titles, `font_lg` field labels
  and `font_md` descriptions. Technical paths may use `font_sm`.
- Use `oxi_plugin::surface_button` for normal settings/control-center actions,
  `oxi_plugin::badge` for source/application-mode chips, and
  `oxi_toggler::toggler` for themed boolean switches. Keep normal input padding;
  compact bar-button sizing belongs to the bar.
- Settings navigation uses a sidebar at desktop widths and a category pick-list
  at narrow widths. Control-center tiles collapse to one column when narrow;
  detail content scrolls inside host-owned chrome without duplicating service
  subscriptions or repainting the full host surface.
- Per-output bars may use independent layout, height, transparency and top/bottom
  placement. Popups and detached input overlays remain local to their source bar;
  all bars keep one shared model for each plugin.
- Monitor settings expose read-only inventory and logical/pixel geometry. Physical
  output changes remain compositor operations.
- Live service errors retain useful cached content and show themed inline status;
  recovery clears that status. Device mute/volume state and media playback remain
  distinct controls. Compact popups scroll when the available height is exceeded.

## Current Surfaces

- Each bar uses `OXITHEME.mantle` unless its effective transparency is enabled,
  and spans its selected output width unless common/profile width overrides it.
- Popup, modal, panel, settings and control-center host surfaces use `OXITHEME.mantle`.
- Notification panel host surface remains on the bar output; toast host surfaces target the active monitor independently.
- Popup open/close animation uses quick `Easing::EASE_OUT` timing to align with notification-center layer opening.
- Plugins request host surfaces through `HOST_REQUEST_TOGGLE_POPUP`, `HOST_REQUEST_OPEN_MODAL`, `HOST_REQUEST_CLOSE_MODAL`, and `HOST_REQUEST_TOGGLE_PANEL`.
- Control-center providers optionally export `control_center_view`; the host also
  supports existing popup/panel views. `HOST_REQUEST_TOGGLE_CONTROL_CENTER` opens
  the host surface, and settings/launcher/control-center dismiss each other.
