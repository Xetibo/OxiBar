# Technical Debt

- Existing general-purpose button helpers include small shadows by default, which is not ideal for every compact popup action.
- Some downstream apps still carry local themed popup row/card styles that can move into `oxiced::widgets::oxi_plugin` once patterns stabilize.
- The theme API exposes colors and spacing but only limited semantic text roles.
- `oxi_plugin` covers primary, muted, and accent text roles; title/disabled/accent-muted roles are still not formalized.
- Generic keyboard focus/screen-reader behavior for button and checkbox wrappers
  remains a follow-up for complete form accessibility; text-input/editor traversal
  is supported by the initial settings UI.
