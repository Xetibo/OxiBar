# UI Guidelines

- Use `OXITHEME` for colors, radius, padding, and text sizes.
- Install explicit immutable `ThemeSnapshot` values; the synchronized compatibility
  handle retains bounded immutable palette/font references. Built-in Catppuccin
  defaults use readable text/neutral surfaces; preserve explicit custom seed roles.
- `oxi_plugin::text` and shared inputs/pickers use the current font. Icon glyphs
  retain their explicit family; reduced/zero motion bypasses animation builders.
- Prefer small reusable helpers over copy-pasted local styles.
- Button helpers should expose active, hovered, pressed, and disabled states.
- Shared helpers should avoid application-specific names unless they are intentionally reusable patterns.
- Keep helper defaults conservative so downstream apps can still override padding, width, height, and actions.
- Compact plugin/status-bar controls should use `oxi_plugin::bar_button` or `oxi_plugin::bar_button_style` for 22.5 height, `[0, 8]` padding, transparent idle/hover background, and shadowless pressed feedback.
- `bar_button` centers its child vertically; `bar_action` pairs recolored SVG icons
  with ordinary font-aware text. `bar_action_with_label` makes that text optional;
  callers own the label preference and any informational tooltip. Semantic SVG actions also
  support large session tiles using the same palette. `navigation_row` keeps idle navigation flat and
  selects using `primary_bg`. `scroll_panel` reserves an embedded scrollbar gutter.
- Use `oxi_plugin::text_primary`, `text_muted`, and `text_accent` for common text roles instead of repeating local closures.
- Search and settings rows use `oxi_plugin::selectable_row`; neutral, selected,
  hovered and pressed states share theme tokens and remain shadowless.
- Spacious field groups use `oxi_plugin::settings_card` with `padding_xl` for
  shared `primary_bg` chrome. Normal surface actions use `oxi_plugin::surface_button`;
  source/mode chips use `oxi_plugin::badge`. Keep compact bar controls separate.
- Use `oxi_toggler::toggler` for a switch styled directly from `OXITHEME`, including
  hover/disabled states. The existing custom animated toggler remains available.
- Multiline settings editors use `oxi_text_input::editor_style`, matching input
  focus, border, value, selection and disabled colors.
- Base16/Base24 supports validated slots, partial inheritance, bright states and an
  optional final accent override. Standalone theme files accept flat or nested
  `[base24]`; host snapshots use their explicit scheme. Semantic roles remain the
  widget API. Derive missing Base16 extensions from supplied/inherited base colors;
  keep neutral hover neutral. Automatic text roles target 4.5:1 across opaque state
  backgrounds without rewriting scheme slots or explicit semantic overrides.
- `text_accent` and shared neutral button/navigation styles use a readable accent
  display color. `accent_selection_style` gives active items an accent fill with
  contrasting text while keeping inactive items accent-colored.
- `oxi_drag_handle` captures press points, drop-zone releases and actual child bounds
  without replacing normal button event handling. Keep placement/input decisions in
  the owning application rather than inside shared controls.
- `drag_preview` draws a noninteractive child beside the pointer, clamped to the
  viewport. Keep payload/revision state in the owning application.
- `capture_visible_bounds` reports clipped rectangles, including an empty rectangle
  when scrolled out, for consumers that must not retain hidden hit regions.
- Give capture wrappers `with_identity` when equal-sized logical items can replace
  one another; geometry alone cannot identify a new callback or action.
- `oxi_reveal::reveal` lays out its child at a stable full height while clipping the
  animated visible viewport for drawing/input. Use it for expensive popup content.
- `oxi_plugin::application_icon` selects SVG/raster rendering and a shared portable
  fallback; application icons should not become font-dependent initial letters.
