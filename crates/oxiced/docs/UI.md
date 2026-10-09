# UI Guidelines

- Use `OXITHEME` for colors, radius, padding, and text sizes.
- Prefer small reusable helpers over copy-pasted local styles.
- Button helpers should expose active, hovered, pressed, and disabled states.
- Shared helpers should avoid application-specific names unless they are intentionally reusable patterns.
- Keep helper defaults conservative so downstream apps can still override padding, width, height, and actions.
- Compact plugin/status-bar controls should use `oxi_plugin::bar_button` or `oxi_plugin::bar_button_style` for 22.5 height, `[0, 8]` padding, transparent idle background, and shadowless hover/pressed states.
- Use `oxi_plugin::text_primary`, `text_muted`, and `text_accent` for common text roles instead of repeating local closures.
- Search and settings rows use `oxi_plugin::selectable_row`; neutral, selected,
  hovered and pressed states share theme tokens and remain shadowless.
- Settings field groups use `oxi_plugin::settings_card` for shared themed chrome.
- Multiline settings editors use `oxi_text_input::editor_style`, matching input
  focus, border, value, selection and disabled colors.
