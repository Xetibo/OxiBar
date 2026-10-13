# UI Guidelines

- Installed plugin catalog entries show enabled/running state, ABI and optional
  capabilities using the existing themed typography/cards. Inspection must not
  start disabled models or background subscriptions.
- Confirmation modals retain unsubmitted intent until an explicit action; Escape,
  replacement and outside dismissal use the same scoped cleanup. App handoff
  lifetime is independent of the transient UI surface.

`UI.md` at the repository root is the full styling contract. This file exists in `docs/` because global agent rules require docs-local UI guidance.

## Rules

- Authentication surfaces share one prompt transaction across outputs. Render
  secret responses as masks and collect them outside ordinary text-input widget
  state; explicitly visible authentication prompts may echo their response. Escape
  clears/cancels, and only authenticated completion unlocks. The locker displays a
  compositor-themed pointer/text/button cursors through the cursor-shape protocol,
  with a visible fallback only when that protocol is absent. Clicking the response
  focuses it before subsequent native keys are handled. Tab/Shift+Tab cycle enabled
  response and action controls with a theme accent ring. Enter activates the focused
  action (or submits from the response), and Space activates focused buttons. New
  response prompts focus the input automatically; typing only edits that input.
  Use opaque themed backgrounds and centered bounded cards with explicit progress
  and failure feedback.
- Greeters offer a trusted session picker and username field before authentication,
  then reuse masked multi-prompt interaction with the same explicit response/action
  focus as the locker. Response clicks and Tab/Shift+Tab must reach the input and
  enabled buttons; captured keyboard events must not disable password editing.
  New prompts focus the response, and cancellation restores username focus.
  Preserve failure feedback and session
  choice on rejected starts; show explicit starting state until the login manager
  acknowledges handoff. Generic window-close events cannot accept a login.
- Authentication views use clock/date, optional bounded assets and centered cards
  with a scroll fallback for small outputs or long prompts. Image/configuration
  failures retain an opaque fallback and readable feedback. Keyboard indicators
  distinguish known state from unknown initial Caps Lock state. Pre-login power
  actions require a visible confirmation and permission checks; dismissal restores
  username focus without accepting a login. Static composition respects reduced motion.

- Use `oxiced::theme::theme_impl::OXITHEME` for colors and theme-derived styling.
- `OXITHEME` is the safe compatibility handle for the explicitly delivered
  immutable snapshot. Use readable built-in Catppuccin roles and preserve explicit
  custom seed colors; live preferences
  change shared tokens rather than introducing a second palette.
- Use `oxi_plugin::text` and shared font-aware inputs/pickers for ordinary text.
  Glyph icons retain their icon font. Reduced motion and zero animation duration
  render final layout directly.
- Prefer shared `oxiced` widgets when available.
- Use `oxiced::widgets::oxi_plugin::bar_button` for standard plugin bar controls.
- Use `oxiced::widgets::oxi_plugin` text role helpers before adding local text style closures.
- Keep bar controls at `oxi_plugin::BAR_CONTROL_HEIGHT`, use `OXITHEME.font_md` text, `OXITHEME.padding_sm` horizontal padding, and transparent idle/hover background. Bar edge padding should use `OXITHEME.padding_sm`; bar labels that combine icons with counts or values should use `OXITHEME.padding_xs` between icon and text.
- Informational bar controls that need details on hover should use the same follow-cursor tooltip pattern as the clock calendar.
- Bars default to icons without names/status labels. `bar.show_labels` applies live
  and may differ by output profile; preserve clock time and workspace numbers as
  functional content. Host action icons do not show name tooltips; provider details
  remain in popups and existing informational tooltips.
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
- Revealed content accepts clicks and hover only within its visible, parent-clipped
  region. Hidden children must let pointer events reach neighboring controls.
- Do not introduce arbitrary palettes or random RGB colors. If `oxiced` cannot express a needed style, track the exception in `docs/TECHNICAL_DEBT.md`.
- Search/task overlays take exclusive keyboard focus only while visible. Scope navigation to the overlay window; Escape and activation dismiss the overlay rather than terminate the daemon. Selected rows must differ from hover, and long result lists must scroll.
- Command-driven provider pickers center on the selected output and clamp their
  size to its logical bounds. Focus search on native configure, keep shared form
  traversal and configured close hints, and dismiss mutually exclusive overlays.
  Reuse provider content with host chrome; bar placement is not a prerequisite.
- Empty asynchronous searches do not flash a no-results message. Retain explicit
  indexing and missing/unavailable-provider status.
- Distinguish backend startup/unavailability, a healthy empty history and an
  unmatched search. Healthy empty history explains how copying adds items; backend
  errors must remain visible alongside useful cached content.
- Show provider prefixes, indexing/unavailable status and separate favorite actions.
  Terminal/PATH command results are prefix-scoped by default; merged application
  search is an explicit preference rather than command noise hiding desktop entries.
  Exact application names/desktop IDs stay ahead of helper-alias matches, including
  usage-boosted or favorite game shortcuts and desktop actions.
  A row or input submission captures its displayed provider/local identity; a later
  result reorder must not change what that activation executes. Ephemeral launcher
  clipboard history is explicitly opt-in and clears when disabled; calculator copy
  does not need it. Persistent picker history survives provider disable.
- Settings fields display their effective value, base/default value, source and
  live/surface/restart classification, with individual reset actions. Invalid edits
  remain visible without losing the running configuration. Continuous text edits
  are drafts; Enter/Apply accepts them. Busy operations preserve widget geometry
  and the form viewport. Keep save/reload diagnostics in a compact fixed-height
  footer; long feedback scrolls without taking space from the form. Put global
  reset immediately before Reload and Close in the header.
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
- Settings navigation uses expandable, indented sidebar groups at desktop widths
  and a full-path page pick-list at narrow widths. Branches offer child-page cards;
  breadcrumbs return to ancestors. Keep the selected route and drafts on saves,
  and keep global search independent of page selection. Animate group reveal with
  full-sized child layout and live motion preferences; reduced/zero motion is direct.
  Use flat themed navigation and neutral `primary_bg` cards. `scroll_panel` reserves
  its scrollbar gutter, so cards and sidebar labels never sit beneath the scroller.
- Control-center navigation stays in a fixed left menu. Long selected detail
  scrolls; stretch content receives the full bounded body height. Reaching another
  provider never requires scrolling past its content.
  The default 1440×960 settings surface provides room for forms and layout editing.
- Per-output bars may use independent layout, height, transparency and top/bottom
  placement and label preference. Popups and detached input overlays remain local to their source bar;
  all bars keep one shared model for each plugin.
- Optional edge reveal hides bar content on pointer departure, uses a small input
  strip at its configured edge and reserves no desktop space. Open dropdowns pin
  visibility until dismissal; preserve the native surface and provider state.
- Monitor settings show a draggable logical-layout diagram and native mode/refresh,
  scale, rotation, position, enablement and supported VRR controls. Show temporary
  preview/Keep and Save/Revert state clearly; expiry rolls back without a UI timer.
  Dragging snaps nearby logical edges with alignment guides; overlapping drops
  restore the gesture's starting position. Exact numeric positions remain unsnapped.
  Native output operations remain delegated to the compositor.
- Live service errors retain useful cached content and show themed inline status;
  recovery clears that status. Device mute/volume state and media playback remain
  distinct controls. Compact popups scroll when the available height is exceeded.
- OSDs are read-only mantle surfaces: shared `oxi_plugin::osd_content` and
  `level_meter`, normal theme roles/padding/radius, no buttons or focus. They are
  pointer-transparent, have zero exclusive zone and never redirect keyboard input.
- Music and notifications share configurable output-local toast placement and
  spacing. Align their chosen edge or center and restack live without overlap;
  device-volume/brightness feedback retains ordinary OSD placement.
- Session actions open a task confirmation; dismissal clears unsubmitted intent.
  Locking invokes the configured dedicated locker, not a visual shell overlay.
- Bar controls center icon/text vertically and use shared SVG action icons plus
  theme-derived spacing between entries. Capture each actual clicked button's
  bounds with logical identity; equal-sized replacement controls must republish them.
  Open dropdowns underneath/above the button and clamp to output edges.
- Control center and notification inbox use bar dropdowns by
  default. Clock opens the configured center/calendar entry; all provider views
  retain one model. Closing a dropdown restores bar input/keyboard policy.
- Every dropdown requests keyboard focus while open and closes with scoped Escape,
  including ordinary provider popups and standalone inbox panels.
- Clipboard pickers distinguish row-click Paste from a separate Copy action that
  retains the picker. Copy must acknowledge success before dismissal/injection;
  paste releases native keyboard ownership first. Keep action failures visible
  across healthy background refreshes and preserve exact data outside previews.
- Session uses a centered, output-bounded OxiShut-style overlay: three equal columns
  of large icon-only tiles, neutral theme fills, accent selection, disabled capability
  styling, hover descriptions and a selected-action label. Native confirmation
  retains the originating output after the menu closes.
- Bar/Layout uses three equal Left/Center/Right columns of draggable cards with a
  noninteractive pointer preview and up/down alternatives. A stale gesture reports
  conflict instead of silently reordering.
- Base16/Base24 slots and the accent preference feed the same snapshot used by launcher,
  bar, forms and menus. Explicit semantic overrides remain available; reset reveals
  the underlying scheme/legacy seed. Automatic text roles target 4.5:1 across opaque
  interaction states; use shared readable accent helpers, not raw syntax slots.
- Shortcuts exposes all host key bindings, live edits, disable/reset and validation
  errors. Surface context determines navigation; footers reflect effective bindings.
- Missing-provider states offer an actionable settings route. Catalog controls
  distinguish not installed, disabled, loading and running providers.

## Dock and Desktop Interaction

- Reuse themed buttons, cards, icons and focus accents for docks and
  widgets. Preserve the shared palette and rounded mantle chrome.
- Space dock entries with theme tokens; focused application uses an accent hue
  instead of a dot below its icon. Inactive accent controls retain readable accent text.
- Render application SVGs as vectors and raster icons with aspect containment.
  Missing icons use the shared portable application SVG rather than an initial letter.
- Keep normal desktop input inside interactive widget bounds; read-only content
  passes input through. Explicit edit mode may cover the output and show roomy
  placement controls, move handles and resize corners.
- Keep persistent output-relative placement when an output disappears; rendered
  fallback must not silently rewrite the user's layout.
- Capture pointer press positions on handles. Suppress inner provider actions in
  edit previews, retain dirty drafts on conflicts, and provide Save/Cancel.
- Outside clicks dismiss keyboard-focused overlays using their actual bounds,
  releasing compositor grabs while retaining window-scoped keyboard behavior.

## Current Surfaces

- Each bar uses `OXITHEME.mantle` unless its effective transparency is enabled,
  and spans its selected output width unless common/profile width overrides it.
- Popup, modal, panel, settings, control-center and OSD host surfaces use `OXITHEME.mantle`.
- Notification inbox stays inside its source bar by default; the optional standalone
  panel remains available. Toast surfaces target the active monitor independently.
- Popup open/close animation uses theme-controlled `Easing::EASE_OUT` duration,
  revealing a fixed full-size child layout. Use a direct final-layout path for
  reduced motion or zero duration; external calendar work must not block opening.
- Plugins request host surfaces through `HOST_REQUEST_TOGGLE_POPUP`, `HOST_REQUEST_OPEN_MODAL`, `HOST_REQUEST_CLOSE_MODAL`, and `HOST_REQUEST_TOGGLE_PANEL`.
- Control-center providers optionally export `control_center_view`; the host also
  supports existing popup/panel views. `HOST_REQUEST_TOGGLE_CONTROL_CENTER` opens
  the host surface, and settings/launcher/control-center dismiss each other.

## Window Overview

- Show actual per-window content, aspect-contained in themed cards, with title,
  application identity and a clear selected state. Unsupported/denied/minimized
  sources show an honest status; an application icon is not a content preview.
- Use large centered output-bounded modals, grouped by native output membership.
  Tab/Shift+Tab cycles windows within that monitor; h/j/k/l moves spatially between
  monitors. Arrows/Home/End select, Enter activates and Escape closes by default.
  All keys are configurable; keep selected cards visible and only one keyboard owner.
- Animate selection with existing snapshot duration; reduced/zero motion renders
  directly. Frame changes do not cross-fade another native window's old content.
- Capture visible card rectangles for redirected cross-monitor pointer events;
  clipped/scrolled-out cards cannot receive them. Release native overview grabs
  before delegating activation, retaining no overview input/capture owner after close.
