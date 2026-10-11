# Decisions

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
