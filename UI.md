# Oxibar Plugin UI

This file is the styling contract for Oxibar plugins. New plugins and UI changes must match these rules so the bar, popups, panels, and modals feel like one product.

## Source Of Truth

- All components must adhere to the `oxiced` theme. This is mandatory for bar items, popup content, panels, modals, cards, buttons, inputs, sliders, picklists, icons, text, and empty states.
- Use `oxiced::theme::theme_impl::OXITHEME` for colors, radius defaults, and derived iced theme behavior.
- Use shared `oxiced` widgets when available: `oxi_button`, `oxi_text_input`, `oxi_picklist`, and `oxi_slider`.
- Do not introduce plugin-local palettes, random RGB values, or one-off control styles unless the shared theme cannot express the state.
- If `oxiced` cannot express a required component state, a local themed style is allowed only as temporary technical debt documented in `docs/TECHNICAL_DEBT.md`.
- If a style is needed in more than one plugin, prefer moving it to a shared helper before copying it again.

## Surfaces

- Main bar background uses `OXITHEME.mantle` unless `[bar] transparent = true` is set.
- Popup, panel, and modal host surfaces use `OXITHEME.mantle`. Plugin content should not repaint the full surface with another base color.
- Popups are compact overlays. Keep content inside `Length::Fill`, with body padding `[12, 14]` and vertical spacing `8` to `10`.
- Panels are full-height side surfaces. Use `padding([14, 14])`, top-level spacing `12`, and scrolling for long lists.
- Modals are focused task surfaces. The host provides radius `18`, shadow, border, and padding `16`; plugin modal content should use full width and not add another outer card.

## Bar Controls

- Bar plugin controls are 22.5 units high.
- Use horizontal padding `[0, 8]` and `Length::Shrink` width for text/icon buttons.
- Use text size `14` for bar labels and icons.
- Align bar text to center on both axes.
- Default bar button style is transparent background, `OXITHEME.primary` text, transparent border, radius `8`, and no shadow.
- Hover background must be `OXITHEME.primary_bg_hover`.
- Pressed background must be `OXITHEME.primary_bg_active`.
- Active/idle status indicators may use filled pills, but must stay within the same 22.5 height and theme colors.

## Typography

- Bar text: `14`.
- Surface title: `18`, color `OXITHEME.primary`.
- Section title: `12`, color `OXITHEME.primary`.
- Primary row text: `13`, color `OXITHEME.text`.
- Secondary/detail text: `10` or `11`, color `OXITHEME.text_muted`.
- Empty states: `12` or `13`, color `OXITHEME.text_muted` unless the empty state is the only popup content, where `OXITHEME.primary` is acceptable.
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
- Primary action: `ButtonVariant::Primary`, text size `13`, padding `[8, 12]`.
- Secondary action: `ButtonVariant::SecondaryBg`, text size `12` or `13`, padding `[5, 8]` for compact controls or `[8, 12]` for modal actions.
- Row actions may use custom transparent button styles only when the row container already supplies the hover background.
- Disabled or busy controls should keep the same visual size. Change label to `Working...`, disable `on_press`, or show pending copy instead of resizing.
- Do not use shadows on plugin buttons.

## Lists And Cards

- Card/list-row background should be `OXITHEME.mantle_hover` at rest and `OXITHEME.primary_bg_hover` on hover.
- Accent/selected cards may use `OXITHEME.primary_bg` at rest.
- Standard row/card radius is `9` or `10`; media/artwork cards may use `12`.
- Inner row content should use padding `[6, 8]` or `[8, 10]`.
- Outer animated row/card wrappers should use padding `[4, 5]` or `[5, 6]`.
- Row icon size should be `13` to `16`; keep icon column width stable when mixed with images.
- Use `AnimationBuilder` with `Motion::SMOOTH` for hover background transitions that track `mouse_area` hover state.
- Long popup lists must be wrapped in `scrollable(...).height(Length::Fill)`.

## Layout

- Prefer `Column::new().spacing(8).padding([12, 14]).width(Length::Fill)` for popup roots.
- Use `Row::new().align_y(Alignment::Center)` for action headers and list rows.
- Use `Space::new().width(Length::Fill)` to push actions to the right.
- Keep row gaps at `4`, `6`, or `8`; use `10` to `12` only between larger sections/cards.
- Use `Length::Fill` for popup/panel/modal body width so host sizing remains authoritative.
- Avoid fixed widths inside plugin content except stable affordances like artwork previews, icons, or equal-size workspace buttons.

## Interaction States

- Every clickable bar item, card, and row must have a hover state.
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

- Plugins request host surfaces with `HOST_REQUEST_TOGGLE_POPUP`, `HOST_REQUEST_OPEN_MODAL`, `HOST_REQUEST_CLOSE_MODAL`, or `HOST_REQUEST_TOGGLE_PANEL`.
- Plugins render only inner content. Host owns popup shape, modal chrome, panel chrome, layer placement, and input region.
- Popup content should be useful at 320x300. Larger plugins may rely on the host large popup size, but must still scroll instead of overflowing.
- Panel content should assume width `420` and full height.
- Modal content should assume size `460x300` and padding `16` supplied by the host.

## Checklist

- Every visible component adheres to the `oxiced` theme.
- Uses `OXITHEME` and shared `oxiced` widgets.
- Any temporary local themed style caused by an `oxiced` limitation is tracked in `docs/TECHNICAL_DEBT.md`.
- Bar control is 22.5 high with text size `14` and padding `[0, 8]`.
- Popup root uses `[12, 14]` padding and `Length::Fill` width.
- Text sizes follow `18`, `14`, `13`, `12`, `11`, `10` scale.
- Hover, pressed, selected, busy, and empty states are defined.
- Long content scrolls.
- No plugin-local palette or arbitrary hardcoded colors.
