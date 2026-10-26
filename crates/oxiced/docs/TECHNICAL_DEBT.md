# Technical Debt

- Existing general-purpose button helpers include small shadows by default, which is not ideal for every compact popup action.
- Some downstream apps still carry local themed popup row/card styles that can move into `oxiced::widgets::oxi_plugin` once patterns stabilize.
- The theme API exposes colors and spacing but only limited semantic text roles.
- `oxi_plugin` covers primary, muted, and accent text roles; title/disabled/accent-muted roles are still not formalized.
- Resolved: button/toggler/slider/picklist keyboard focus, caller-owned remappable
  navigation, nested menus and scrolling. Native text inputs/editors retain typing.
- Screen-reader semantics and keyboard adapters for unused legacy checkbox/radio
  widgets remain follow-up work; action widgets should compose the focusable helpers.
- Base16/Base24 automatic text contrast is checked on opaque semantic backgrounds.
  Explicit role overrides and transparent backdrops can intentionally defeat that
  guarantee; a GUI contrast report for custom roles remains a possible extension.
- Resolved: dim Catppuccin helper text, colored neutral hover, unreadable filled text
  and unrelated Base24 bright colors inherited by Base16-only schemes.
- Resolved: draw-preview child reconciliation across empty surfaces and optional
  preview transitions; regression tests and downstream pointer smoke cover it.
- Resolved: equal-sized replacement controls could retain another logical item's
  bounds cache. Optional capture identity invalidates it, with a widget-tree regression
  and downstream post-drag anchor verification.
