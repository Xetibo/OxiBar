# Oxibar Plugin UI

This file is the styling contract for Oxibar plugins. New plugins and UI changes must match these rules so the bar, popups, panels, and modals feel like one product.

## Source Of Truth

- All components must adhere to the `oxiced` theme. This is mandatory for bar items, popup content, panels, modals, cards, buttons, inputs, sliders, picklists, icons, text, and empty states.
- Use `oxiced::theme::theme_impl::OXITHEME` for colors, radius defaults, and derived iced theme behavior.
- Use shared `oxiced` widgets when available: `oxi_button`, `oxi_text_input`, `oxi_picklist`, and `oxi_slider`.
- Do not use raw numeric literals for spacing, padding, margins, border radii, or font sizes. Use `OXITHEME` tokens, `oxi_plugin` constants/helpers, or a named local constant for plugin-specific geometry.
- Do not introduce plugin-local palettes, random RGB values, or one-off control styles unless the shared theme cannot express the state.
- If `oxiced` cannot express a required component state, a local themed style is allowed only as temporary technical debt documented in `docs/TECHNICAL_DEBT.md`.
- If a style is needed in more than one plugin, prefer moving it to a shared helper before copying it again.

## Surfaces

- Main bar spans the compositor-selected active output width unless `[bar] width` overrides it. Its background uses `OXITHEME.mantle` unless `[bar] transparent = true` is set.
- Popup, panel, and modal host surfaces use `OXITHEME.mantle`. Plugin content should not repaint the full surface with another base color.
- Popup open/close animation uses quick `Easing::EASE_OUT` timing to align with notification-center layer opening.
- Popups are compact overlays. Keep content inside `Length::Fill`, with body padding based on `OXITHEME.padding_md`/`padding_lg` and vertical spacing from `OXITHEME.padding_sm`.
- Plugins with model-dependent popup content should export `popup_metrics(model)` and cap long content at the host max height with scrolling.
- Panels are full-height side surfaces. Use `OXITHEME.padding_md`, top-level spacing from `OXITHEME.padding_md`, and scrolling for long lists.
- Modals are focused task surfaces. The host provides themed radius, shadow, border, and `OXITHEME.padding_lg`; plugin modal content should use full width and not add another outer card.

## Bar Controls

- Bar plugin controls use `oxi_plugin::BAR_CONTROL_HEIGHT`.
- Use `oxi_plugin::bar_button` for standard horizontal padding and `Length::Shrink` width for text/icon buttons.
- The host bar keeps `OXITHEME.padding_sm` at the left and right window edges.
- Use `OXITHEME.font_md` for bar labels and icons.
- Align bar text to center on both axes.
- Text labels that combine icons with counts or values should use `OXITHEME.padding_xs` between the icon and text, not extra inner padding.
- Informational bar controls may show extra detail with `tooltip::Position::FollowCursor` and the same themed tooltip chrome as the calendar.
- Default bar button style is transparent background, `OXITHEME.primary` text, transparent border, themed radius, and no shadow.
- Hover background must be `OXITHEME.primary_bg_hover`.
- Pressed background must be `OXITHEME.primary_bg_active`.
- Active/idle status indicators may use filled pills, but must stay within `oxi_plugin::BAR_CONTROL_HEIGHT` and theme colors.

## Typography

- Bar text: `OXITHEME.font_md`.
- Surface title: `OXITHEME.font_lg`, color `OXITHEME.primary`.
- Section title: `OXITHEME.font_md`, color `OXITHEME.primary`.
- Primary row text: `OXITHEME.font_md`, color `OXITHEME.text`.
- Secondary/detail text: `OXITHEME.font_sm`, color `OXITHEME.text_muted`.
- Empty states: `OXITHEME.font_md`, color `OXITHEME.text_muted` unless the empty state is the only popup content, where `OXITHEME.primary` is acceptable.
- Plugins should inherit the bar font. If a plugin needs explicit weight, resolve `[bar] font` first and then set only the weight.
- Use semibold only for compact identity controls such as workspace pills.

## Color Roles

- `OXITHEME.primary`: accent text, section headings, idle icon buttons.
- `OXITHEME.primary_contrast`: text on filled primary backgrounds.
- `OXITHEME.text`: primary readable content.
- `OXITHEME.text_muted`: secondary content, metadata, helper text, empty states.
- `OXITHEME.mantle`: host surface background.
- `OXITHEME.mantle_hover`: neutral card/list-row background.
- `OXITHEME.primary_bg`: subtle accent background for secondary controls and selected cards.
- `OXITHEME.primary_bg_hover`: hover background for interactive controls and rows.
- `OXITHEME.primary_bg_active`: pressed background.
- `OXITHEME.primary`, `OXITHEME.primary_hover`, and `OXITHEME.primary_active`: filled primary controls such as active workspace pills.

## Buttons

- Prefer `oxi_button::button` variants for normal actions.
- Primary action: `ButtonVariant::Primary`, `OXITHEME.font_md`, padding from `OXITHEME.padding_sm`/`padding_md`.
- Secondary action: `ButtonVariant::SecondaryBg`, `OXITHEME.font_md`, padding from `OXITHEME.padding_xs`/`padding_sm` for compact controls or `padding_sm`/`padding_md` for modal actions.
- Row actions may use custom transparent button styles only when the row container already supplies the hover background.
- Disabled or busy controls should keep the same visual size. Change label to `Working...`, disable `on_press`, or show pending copy instead of resizing.
- Do not use shadows on plugin buttons.

## Lists And Cards

- Card/list-row background should be `OXITHEME.mantle_hover` at rest and `OXITHEME.primary_bg_hover` on hover.
- Accent/selected cards may use `OXITHEME.primary_bg` at rest.
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

- Every clickable bar item, card, and row must have a hover state.
- Notification cards are clickable surfaces: click invokes the primary notification action, or dismisses the notification when it has no action. Child controls such as close buttons, action buttons, and inline reply inputs must remain usable.
- Toast notifications must not take keyboard focus when they appear. Hover/click intent may enable keyboard focus for inline replies, and hover or inline reply focus/draft must prevent expiry while the user is interacting with them.
- Tray rows should keep one primary visible row action. Secondary actions and DBusMenu entries belong in the row context menu, opened by right-click.
- Pressed state should darken or strengthen the same color family, not switch palettes.
- Selected/current state should be visually distinct from hover. Use filled primary for active workspace-style pills, or `primary_bg`/`primary_bg_hover` for selected cards.
- Busy state should preserve layout and prevent duplicate actions.
- Empty state copy should be short and specific: `No connected devices`, `No notifications`, `No tray items`.

## Icons

- Plugins may use Nerd Font glyphs for compact bar indicators.
- Bar icon labels should show only the icon when count/value is zero or unknown, and `icon value` when useful.
- Keep icon language functional. Avoid decorative icons that do not communicate state or action.
- External image/svg icons should fit their box with `ContentFit::Contain` unless artwork intentionally uses `ContentFit::Cover`.

## Popup, Modal, And Panel Duties

- Plugins request host surfaces with `HOST_REQUEST_TOGGLE_POPUP`, `HOST_REQUEST_OPEN_MODAL`, `HOST_REQUEST_CLOSE_MODAL`, or `HOST_REQUEST_TOGGLE_PANEL`. Plugins with variable popup content can expose `popup_metrics(model)` so the host sizes visible chrome and input region from current model state.
- Plugins render only inner content. Host owns popup shape, modal chrome, panel chrome, layer placement, and input region.
- Popup content should be useful at the host default popup size. Larger plugins may rely on named popup-size constants or `[bar.popup_sizes]`, but must still scroll instead of overflowing.
- Panel content should assume the host `PANEL_WIDTH` and full height.
- Modal content should assume the host `MODAL_SIZE` and themed host padding.

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
