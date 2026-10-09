# Decisions

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
