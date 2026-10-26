# Oxibar Plugin UI

This file is the styling contract for Oxibar plugins and shell surfaces. New plugins and UI changes must match these rules so the bar, popups, panels, modals, settings and control center feel like one product.

## Source Of Truth

- All components must adhere to the `oxiced` theme. This is mandatory for bar items, popup content, panels, modals, cards, buttons, inputs, sliders, picklists, icons, text, and empty states.
- Use `oxiced::theme::theme_impl::OXITHEME` for colors, radius defaults, and derived iced theme behavior.
- Upgraded libraries receive immutable snapshots explicitly. The safe `OXITHEME`
  handle exposes their current tokens; legacy fallback preserves the existing seed.
- Base24 (`base00`–`base17`) and an optional accent map to those same semantic roles.
  Base16-only inputs derive missing extensions. Automatic text roles target 4.5:1
  on opaque state backgrounds; explicit role overrides remain authoritative and
  accent selection is final. Scheme slots are inputs, not widget color APIs.
- Use shared `oxiced` widgets when available: `oxi_button`, `oxi_text_input`, `oxi_picklist`, `oxi_slider`, `oxi_toggler`, and `oxi_plugin` surface helpers.
- Do not use raw numeric literals for spacing, padding, margins, border radii, or font sizes. Use `OXITHEME` tokens, `oxi_plugin` constants/helpers, or a named local constant for plugin-specific geometry.
- Do not introduce plugin-local palettes, random RGB values, or one-off control styles unless the shared theme cannot express the state.
- If `oxiced` cannot express a required component state, a local themed style is allowed only as temporary technical debt documented in `docs/TECHNICAL_DEBT.md`.
- If a style is needed in more than one plugin, prefer moving it to a shared helper before copying it again.

## Surfaces

- Each bar spans its selected output width unless common/profile `width` overrides it. Its background uses `OXITHEME.mantle` unless its effective `transparent = true` is set.
- Popup, panel, and modal host surfaces use `OXITHEME.mantle`. Plugin content should not repaint the full surface with another base color.
- Popup open/close uses theme-controlled `Easing::EASE_OUT` duration. Reduced motion
  and zero duration return final content directly, without a zero-time animation.
- Popups are compact overlays. Keep content inside `Length::Fill`, with body padding based on `OXITHEME.padding_md`/`padding_lg` and vertical spacing from `OXITHEME.padding_sm`.
- Plugins with model-dependent popup content should export `popup_metrics(model)` and cap long content at the host max height with scrolling.
- Panels are full-height side surfaces. Use `OXITHEME.padding_md`, top-level spacing from `OXITHEME.padding_md`, and scrolling for long lists.
- Modals are focused task surfaces. The host provides themed radius, shadow, border, and `OXITHEME.padding_lg`; plugin modal content should use full width and not add another outer card.
- Search overlays reuse host theme tokens and controls, take keyboard focus while visible, and close on Escape or activation without terminating the shell. Keep result selection distinct from hover and make long lists scrollable.
- Settings forms use shared `oxi_plugin::settings_card`, selectable rows and
  themed inputs/editors. Show value provenance and reset controls; preserve drafts
  on revision conflicts and show restart-only changes explicitly.
- Settings and control center use spacious, Android-inspired grouping with the
  same oxiced palette, radius and components. Use `padding_xl` outer/card padding,
  `padding_lg`/`padding_xl` gaps, readable descriptions and normal-sized actions.
  Boolean fields use themed switches rather than a separate cramped checkbox row.
- Desktop settings use animated expandable sidebar groups, nested pages and
  breadcrumbs; narrow settings use a full-path page pick-list. Reveal full-sized
  children without reshaping text, and honor reduced/zero motion. Keep drafts and
  navigation stable across saves and global search independent of the selected page.
  Place global reset before Reload/Close in the header; keep feedback in a compact
  fixed-height scrolling footer. Use flat `navigation_row` controls and reserved
  `scroll_panel` gutters. Control
  center uses a fixed left provider menu; long detail content scrolls, while bounded
  stretch content fills available height. Layout editing uses three equal columns
  of small draggable cards with a pointer-following preview and up/down actions.
- Display arrangement snaps nearby logical edges and shows alignment guides;
  overlapping drops restore the original position. Numeric position controls stay
  exact, and native preview/confirmation remains a separate operation.
- Multiple output bars retain independent geometry and input regions. Attached
  popups appear below top bars or above bottom bars, on the originating output.
  Removing an output dismisses its surfaces without discarding plugin state.
- Dropdowns reuse their owning bar, anchor to actual pressed child bounds and clamp
  to the output. Drawing, input regions and outside dismissal use the same offset;
  layout changes invalidate moved/removed slot identities.
- Every dropdown takes keyboard focus while open and closes with scoped Escape,
  restoring the bar policy on dismissal. Edge-revealed bars remain visible while
  their dropdown is open and otherwise hide on pointer departure.
- OSDs reuse `oxi_plugin::osd_content` and `level_meter` inside mantle chrome with
  `padding_lg` and theme radius. They are read-only, pointer-transparent and have
  no keyboard interactivity or exclusive zone. Coalesce updates per output and
  keep existing text input usable while they appear.
- Media feedback aligns with the configured notification toast edge/center and
  joins its output-local stack. Preserve spacing and live restacking when other
  entries resize or close; device-volume/brightness OSD placement stays independent.
- Power/logout actions use a confirmation task modal; dismissal clears pending
  intent. Lock UI launches a dedicated locker rather than imitating a lock surface.

## Bar Controls

- Bar plugin controls use `oxi_plugin::BAR_CONTROL_HEIGHT`.
- Use `oxi_plugin::bar_button` for standard horizontal padding and `Length::Shrink` width for text/icon buttons.
- Its child container centers content within the fixed control height. Host action
  controls use `bar_action_with_label` SVG icons and optional text; section rows use `padding_sm`
  between entries. Do not reduce spacing to make settings controls mimic bar height.
- The host bar keeps `OXITHEME.padding_sm` at the left and right window edges.
- Use `OXITHEME.font_md` for bar labels and icons.
- Align bar text to center on both axes.
- Text labels that combine icons with counts or values should use `OXITHEME.padding_xs` between the icon and text, not extra inner padding.
- Informational bar controls may show extra detail with `tooltip::Position::FollowCursor` and the same themed tooltip chrome as the calendar.
- `bar.show_labels` defaults to false and applies per output without changing shared
  provider state. Host action icons do not show name tooltips; clock time and workspace
  numbers are functional content and remain visible.
- Default bar button style is transparent background, contrast-aware accent text,
  transparent border, themed radius, and no shadow. Use shared helpers so light
  schemes do not put an unreadable raw accent on neutral surfaces.
- Bar-icon hover stays transparent. Navigation, cards and ordinary surface actions
  retain themed hover feedback.
- Pressed background must be `OXITHEME.primary_bg_active`.
- Active/idle status indicators may use filled pills, but must stay within `oxi_plugin::BAR_CONTROL_HEIGHT` and theme colors.

## Typography

- Bar text: `OXITHEME.font_md`.
- Surface title: `OXITHEME.font_lg`, color `OXITHEME.primary`.
- Section title: `OXITHEME.font_md`, color `OXITHEME.primary`.
- Primary row text: `OXITHEME.font_md`, color `OXITHEME.text`.
- Secondary/detail text: `OXITHEME.font_sm`, color `OXITHEME.text_muted`.
- Empty states: `OXITHEME.font_md`, color `OXITHEME.text_muted` unless the empty state is the only popup content, where `OXITHEME.primary` is acceptable.
- Ordinary text uses `oxi_plugin::text` and font-aware shared inputs/pickers. Use
  the current snapshot font before overriding weight; glyph icons keep their font.
  The empty live family inherits the startup bar font.
- Use semibold only for compact identity controls such as workspace pills.
- Spacious settings/control-center typography uses `font_xxl`/`text` for page
  titles, `font_xl` for group headings, `font_lg` for field/tile labels and
  `font_md` for descriptions/actions. Reserve `font_sm` for technical metadata.

## Color Roles

- `OXITHEME.primary`: accent fills and icons; use `text_accent` for readable accent text.
- `OXITHEME.primary_contrast`: text on filled primary backgrounds.
- `OXITHEME.text`: primary readable content.
- `OXITHEME.text_muted`: secondary content, metadata, helper text, empty states.
- `OXITHEME.mantle`: host surface background.
- `OXITHEME.mantle_hover`: neutral card/list-row background.
- `OXITHEME.primary_bg`: shared neutral form-card/control background, and selected navigation.
- `OXITHEME.primary_bg_hover`: hover background for interactive controls and rows.
- `OXITHEME.primary_bg_active`: pressed background.
- `OXITHEME.primary`, `OXITHEME.primary_hover`, and `OXITHEME.primary_active`: filled primary controls such as active workspace pills.

## Buttons

- Prefer `oxi_button::button` variants for normal actions.
- Settings/control-center actions use shadowless `oxi_plugin::surface_button`
  with `padding_md`/`padding_lg`; reserve `bar_button` height for compact bar items.
  Source/mode chips use `oxi_plugin::badge`, and switches use
  `oxi_toggler::toggler`, all styled directly from `OXITHEME`.
- Primary action: `ButtonVariant::Primary`, `OXITHEME.font_md`, padding from `OXITHEME.padding_sm`/`padding_md`.
- Secondary action: `ButtonVariant::SecondaryBg`, `OXITHEME.font_md`, padding from `OXITHEME.padding_xs`/`padding_sm` for compact controls or `padding_sm`/`padding_md` for modal actions.
- Row actions may use custom transparent button styles only when the row container already supplies the hover background.
- Disabled or busy controls should keep the same visual size. Change label to `Working...`, disable `on_press`, or show pending copy instead of resizing.
- Do not use shadows on plugin buttons.

## Lists And Cards

- Card/list-row background should be `OXITHEME.mantle_hover` at rest and `OXITHEME.primary_bg_hover` on hover.
- Accent/selected cards may use `OXITHEME.primary_bg` at rest.
- Settings groups use that same `primary_bg` role, rather than a settings-specific
  palette or permanently opaque navigation tiles.
- Standard row/card radius should come from `OXITHEME.border_radius` or `oxi_plugin::compact_card`.
- Inner row content should use `OXITHEME.padding_xs`/`padding_sm`; modal actions use `padding_sm`/`padding_md`.
- Outer animated row/card wrappers should use `OXITHEME.padding_xs` unless a named plugin geometry constant is needed.
- Row icon size should use `OXITHEME.font_md` or `font_lg`; keep icon column width stable when mixed with images.
- Use `AnimationBuilder` with `Motion::SMOOTH` for hover background transitions that track `mouse_area` hover state.
- Long popup lists must be wrapped in `scrollable(...).height(Length::Fill)`.

## Layout

- Prefer `Column::new().spacing(OXITHEME.padding_sm).padding([OXITHEME.padding_md, OXITHEME.padding_lg]).width(Length::Fill)` for popup roots.
- Use `Row::new().align_y(Alignment::Center)` for action headers and list rows.
- Use `Space::new().width(Length::Fill)` to push actions to the right.
- Keep row gaps on `OXITHEME.padding_xs`, `padding_sm`, or `padding_md` depending on density.
- Use `Length::Fill` for popup/panel/modal body width so host sizing remains authoritative.
- Avoid fixed widths inside plugin content except stable affordances like artwork previews, icons, or equal-size workspace buttons.

## Interaction States

- Cards, rows and surface actions have themed hover feedback; bar icons stay flat.
- Notification cards are clickable surfaces: click invokes the primary notification action, or dismisses the notification when it has no action. Child controls such as close buttons, action buttons, and inline reply inputs must remain usable.
- Toast notifications must not take keyboard focus when they appear. Hover/click intent may enable keyboard focus for inline replies, and hover or inline reply focus/draft must prevent expiry while the user is interacting with them.
- Tray rows should keep one primary visible row action. Secondary actions and DBusMenu entries belong in the row context menu, opened by right-click.
- Pressed state should darken or strengthen the same color family, not switch palettes.
- Selected/current state should be visually distinct from hover. Use filled primary for active workspace-style pills, or `primary_bg`/`primary_bg_hover` for selected cards.
- Busy state should preserve layout and prevent duplicate actions.
- Reserve bounded save/reload status space so settings feedback does not resize
  the form viewport. Disabled controls retain their widget geometry.
- Empty state copy should be short and specific: `No connected devices`, `No notifications`, `No tray items`.
- Asynchronous launcher searches leave an empty result area quiet; show indexing
  or provider errors rather than transient no-results copy.

## Icons

- Plugins may use Nerd Font glyphs for compact bar indicators.
- Bar indicators default to icons only. When labels are enabled, show an optional
  name or useful status value; unknown/zero values may remain icon-only.
- Keep icon language functional. Avoid decorative icons that do not communicate state or action.
- External image/svg icons should fit their box with `ContentFit::Contain` unless artwork intentionally uses `ContentFit::Cover`.

## Popup, Modal, And Panel Duties

- Plugins request host surfaces with `HOST_REQUEST_TOGGLE_POPUP`, `HOST_REQUEST_OPEN_MODAL`, `HOST_REQUEST_CLOSE_MODAL`, or `HOST_REQUEST_TOGGLE_PANEL`. Plugins with variable popup content can expose `popup_metrics(model)` so the host sizes visible chrome and input region from current model state.
- Plugins render only inner content. Host owns popup shape, modal chrome, panel chrome, layer placement, and input region.
- Popup content should be useful at the host default popup size. Larger plugins may rely on named popup-size constants or `[bar.popup_sizes]`, but must still scroll instead of overflowing.
- Panel content should assume the host `PANEL_WIDTH` and full height.
- Modal content should assume the host `MODAL_SIZE` and themed host padding.
- Scroll longer task prompts so their actions remain reachable. Distinguish typed
  input from displayed codes and explicit Allow/Reject decisions; invalid answers
  keep the prompt available. Enter may submit input, while Escape is window-scoped.
- Optional `modal_closed(model)` performs nonblocking, idempotent operation cleanup
  on host dismissal/replacement/output retirement. Keep a protocol modal alive
  after submitting its answer until completion or the next prompt; suppress queued
  prompts after cancellation. Cleanup tasks remain valid after the window closes.
- Optional `control_center_view(model)` returns inner content for the existing
  model. The host owns scrolling/cards/chrome; avoid nested full-height scrolling,
  duplicated outer padding or new subscriptions for an embedded view. Popup/panel
  content is the compatibility fallback. All content messages retain source output
  routing, including asynchronous password/pairing requests.
- Clock/Controls open attached control-center content by default. Notifications
  supplies an embedded inbox with its own header/padding; separate-panel mode is
  optional. Session uses a centered OxiShut-style tile overlay, with three equal
  columns of large labeled action icons, neutral themed fills, shared keyboard focus
  rings and muted unavailable capabilities. Tiles have no hover highlight or tooltip.
  Entries delegate to cached commands and task confirmations;
  capture output context before retiring the menu for that handoff.
- Popup, panel and modal actions use shared focusable controls and one host-owned
  navigation policy. Tab/arrows/Vim traversal stays inside the active content,
  scrolls focused controls into view and skips collapsed/disabled actions. Text
  editors retain editing keys. Nested menus inherit bindings and dismiss before
  their parent on Escape. Focus rings use theme accent without changing geometry.

## Dock and Desktop Surfaces

- Dock buttons have theme-derived gaps and use an accent hue for the focused
  application's background; do not add a dot beneath each icon.
  SVG and raster application icons use their matching renderer; missing icons use
  a portable application SVG. Window management stays with the compositor.
- Normal widgets use mantle/radius tokens and only their own bounds. Read-only
  widgets are pointer-transparent; explicit input overrides remain available.
- Full-output edit mode uses the same palette and spacious controls, suppresses
  provider actions in previews and offers Save/Cancel, grid move/resize and reset.
- Use `oxi_drag_handle` for event-local press coordinates. Keep configured logical
  positions intact when rendering a clamped fallback on another output.
- Dismiss exclusive overlays on scoped outside clicks; do not reroute/replay those
  clicks as another provider action.

## Window Overview

- Use real native window pixels in aspect-contained, theme-colored cards. Keep
  title/application labels, selection, error and unavailable states readable.
- Large centered modals per monitor share a visible-owned capture model and are
  bounded to the output. Only one owns keyboard focus; arrows/Home/End select,
  Tab/Shift+Tab cycles windows, h/j/k/l moves spatially between monitors and
  Enter/click activates the exact current window. All keys live in Shortcuts.
- Animate selection without animating old pixels into a replacement identity.
  Snapshot reduced motion/zero duration bypasses the animation builder.
- Use clipped visible card bounds for cross-monitor pointer-grab redirection and
  embedded scrollbar gutters. Wait for native overview retirement before activation.
- Dropdown reveal animations retain full child layout while clipping the visible
  viewport; do not continuously reshape calendar/form content as the popup opens.

## Checklist

- Every visible component adheres to the `oxiced` theme.
- Uses `OXITHEME` and shared `oxiced` widgets.
- Any temporary local themed style caused by an `oxiced` limitation is tracked in `docs/TECHNICAL_DEBT.md`.
- Bar control uses `oxi_plugin::BAR_CONTROL_HEIGHT`, `OXITHEME.font_md`, and `oxi_plugin::bar_button` padding.
- Popup root uses `OXITHEME` padding tokens and `Length::Fill` width.
- Text sizes use `OXITHEME.font_*` tokens or named plugin-specific constants.
- Hover, pressed, selected, busy, and empty states are defined.
- Long content scrolls.
- No plugin-local palette or arbitrary hardcoded colors.
- No raw UI magic numbers for spacing, padding, margins, radii, or font sizes.
