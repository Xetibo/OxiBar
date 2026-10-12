# Decisions

## 2026-10-12 — Flat bar-icon hover

- Keep shared bar controls transparent on hover and retain pressed feedback.
  `navigation_row` explicitly keeps its ordinary themed hover fill; selected
  accent/pill controls retain their semantic selection styles.
- Host action icons no longer add name tooltips. Informational provider/calendar
  tooltips remain caller-owned; no shared API or dependency change.
- Shared regression and downstream workspace tests pass. The isolated native shell
  smoke verifies bars, dropdowns, forms and provider interactions with the same
  rebuilt host/DLL graph.

## 2026-10-11 — Optional bar labels and logical bounds identity

- Add portable reboot/sleep/hibernate/lock/logout SVG actions and
  `bar_action_with_label`; keep the existing `bar_action` signature and behavior.
  Output-local preferences belong to callers, not the shared theme/model.
- Equal-width icon controls exposed a recycled-tree anchor cache: geometry did not
  change when the logical item did. Optional `DragHandle::with_identity` clears the
  cache on identity replacement. Host bar slots and overview windows supply identity.
- The new regression preserves cached geometry for unchanged identity and clears it
  for a replacement. Thirteen shared-library tests pass; two final downstream GUI
  runs pass icon-only/live labels, centered tiles, dragging and post-drag anchoring.

## 2026-10-11 — Contrast-aware scheme adapter and pointer drag previews

- Keep Base16/Base24 as compatibility input for Nix theme ecosystems and semantic
  roles as the GUI API. Base16-only schemes derive missing extensions; raw slots
  and explicit semantic overrides retain their values. No new dependency or ABI.
- Automatic normal/muted and filled text targets 4.5:1 across opaque interaction
  backgrounds. Shared accent-text helpers adjust only a display color; built-in
  Catppuccin now has readable helper text and neutral hover/border surfaces.
- Add shared accent selection for focused/pill controls and a draw-only pointer
  preview. A unique wrapper tree tag plus `diff_children` fixes empty-surface and
  optional-child transitions without resetting the underlying input state.
- Real downstream three-column dragging initially exposed both empty-tree access
  and a fixed height overwritten by a `Fill` alignment helper; regression tests
  and the complete isolated GUI smoke now pass with a visible dragged card.

## 2026-10-11 — Stable reveal layout and portable application icons

- Added `oxi_reveal::reveal`: expensive child content keeps its full layout while
  animated viewport drawing/input is clipped. Calendar forms no longer reshape at
  every intermediate dropdown height; reduced/zero motion keeps the direct path.
- Added SVG/raster-aware `application_icon` with a portable application fallback,
  rather than font-dependent letters or passing vectors to a raster renderer.
- `capture_visible_bounds` reports clipped/empty child rectangles without consuming
  ordinary button events. Consumers can remove hidden hit regions rather than use
  stale full-card bounds; existing bar capture semantics remain unchanged.
- Workspace checks, real native preview/dock GUI and blocked-calendar actual-
  presentation benchmarks verify these shared helpers through the shell.

## 2026-10-11 — Shared Base24 and form/bar geometry

- Added all 24 slots and optional accent to immutable snapshots. Standalone files
  accept flat or nested schemes; legacy colors and explicit roles remain compatible.
  Semantic role overrides follow mapping; final accent and contrast are shared.
- Center bar children explicitly, use shared SVG action icons/spacing, neutral form
  cards and flat navigation. Embedded scrollbar spacing reserves a real gutter.
- Shared event wrappers report actual child bounds/presses and capture drop releases;
  application-owned slot identity/revision remains responsible for mutations/placement.
- New Base24 regression tests, full workspace checks and private shell GUI verify
  consistent bar/launcher/settings/control themes without changing the default seed.

## Immutable live snapshots (2026-10-11)

- Preserve exact seed colors and derived states; make palette/font/density/motion
  changes explicit snapshots delivered by the host to upgraded consumers.
- A synchronized immutable intern pool keeps existing `OXITHEME` field access safe
  without mutating previously borrowed tokens. Bound distinct palette/font values
  to 128; reuse values for metadata/revision-only changes.
- Ordinary text and shared inputs/pickers inherit the live family, while icons keep
  their glyph font. Upload font bytes before switching family and bypass reduced/
  zero-duration animations. Legacy consumers retain fallback behavior.
- Workspace/unit checks and private packaged host/provider GUI verified exact seed,
  shared revisions, font/density/colors, invalid-input retention and restart/reset.

## Spacious surface controls (2026-10-10)

- Add normal-sized flat surface buttons, badges and a directly theme-styled Iced
  switch for settings/control-center layouts. Increase settings-card padding to
  `padding_xl`; keep compact bar controls unchanged. Use theme radius in shared
  flat/card styles instead of duplicated numeric radii.
- Use the builtin Iced switch for the new helper, preserving the existing custom
  animated toggler API. Host workspace checks and packaged visual smoke cover the
  shared library through its actual consumers.

## Workspace integration (2026-10-09)

- Import the exact previously pinned upstream revision; preserve library license
  and public modules. Root Cargo/Nix tooling now owns dependency pinning and builds.
- Add generic selectable rows and settings-card chrome so launcher and settings
  UI use one shared style instead of application-local selection closures.
