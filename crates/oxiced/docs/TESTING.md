# Testing

## Strategy

- Unit tests cover pure helper/style behavior.
- UI rendering remains manually verified in downstream applications.
- Downstream crates should run their own tests after adopting new helpers.

## Commands

- `cargo fmt --all`
- From the workspace root: `nix develop -c cargo test -p oxiced`.
- From the workspace root: `nix develop -c cargo clippy -p oxiced --all-targets`.
