# Testing

## Strategy

- Unit tests cover pure helper/style behavior, Base16 extension precedence,
  explicit color preservation and Mocha/Latte/default text contrast in all states.
- Widget-tree tests cover empty-root replacement and optional drag-preview child
  transitions, plus equal-geometry replacement with changed bounds identity;
  reveal tests preserve full child layout through animation.
- Oxibar's isolated GUI smoke exercises actual shared controls, pointer previews,
  SVG/raster application rendering, icon-only/live-label controls, session action
  tiles, theme changes and calendar layout.
- Downstream crates should run their own tests after adopting new helpers.

## Commands

- From the workspace root: `nix develop -c cargo fmt --all -- --check`.
- From the workspace root: `nix develop -c cargo test -p oxiced`.
- From the workspace root: `nix develop -c cargo clippy --workspace --all-targets -- -D warnings`.
