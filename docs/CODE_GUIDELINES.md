# Code Guidelines

## Rust Style

- Keep changes small and aligned with existing module boundaries.
- Prefer pure helper functions for parsing, state transitions, and config extraction so behavior is unit-testable without DBus, Wayland, Hyprland, or shell commands.
- Keep dynamic plugin ABI entry points thin. Delegate testable behavior to normal Rust functions where practical.
- Plugins that require optional hardware or system services should export `availability(config)` and keep the probe cheap; put the actual probe logic in `system.rs` so it can be unit-tested.
- Keep model-dependent popup sizing in pure helpers and expose it through optional `popup_metrics(model)` only when content height truly varies.
- Keep plugin `lib.rs` focused on ABI, model updates, and view composition. Move command/DBus integrations, parsers, and domain data into modules such as `system.rs` before files become god files.
- Use `Result<T, String>` for plugin-local external command/DBus errors when the error is surfaced through a plugin error queue.
- Avoid long-lived locks around UI building or external calls.
- Add comments only when code is not self-explanatory or unsafe/lifetime behavior needs documentation.
- Do not leave magic numbers in code. Use named constants for domain/protocol values, put user-tunable values in config, and use `oxiced` theme/helper tokens for UI spacing, padding, margins, border radii, and font sizes.
- When the same UI number repeats across plugins, add or use a shared `oxiced` token/helper before adding plugin-local constants.

## Testing Expectations

- Add unit tests near the helper functions they cover.
- Prefer deterministic tests that do not require live system services.
- Main loop tests should cover pure host logic: config parsing, message mapping, popup metrics, placement math, and state transitions that do not require layershell runtime.
- Plugin tests should cover each plugin's parsing, config, helper logic, and model update behavior where possible.

## Formatting And Checks

- Use `cargo fmt` for formatting.
- Use `cargo test --workspace` for test coverage.
- Use `cargo clippy --workspace --all-targets` when practical before larger refactors.

## Nix

- `flake.nix` exists. Prefer running commands from the flake/dev shell where practical.
