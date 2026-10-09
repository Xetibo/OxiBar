# Decisions

## Workspace integration (2026-10-09)

- Import the exact previously pinned upstream revision; preserve library license
  and public modules. Root Cargo/Nix tooling now owns dependency pinning and builds.
- Add generic selectable rows and settings-card chrome so launcher and settings
  UI use one shared style instead of application-local selection closures.
