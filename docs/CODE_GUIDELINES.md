# Code Guidelines

## Rust Style

- Keep changes small and aligned with existing module boundaries.
- Prefer pure helper functions for parsing, state transitions, and config extraction so behavior is unit-testable without DBus, Wayland, Hyprland, or shell commands.
- Keep dynamic plugin ABI entry points thin. Delegate testable behavior to normal Rust functions where practical.
- Plugins that require optional hardware or system services should export `availability(config)` and keep the probe cheap; put the actual probe logic in `system.rs` so it can be unit-tested.
- Reconnecting native services may stay loaded with empty/offline state. Do not
  gate them on currently present hardware when hotplug recovery is required.
- Keep model-dependent popup sizing in pure helpers and expose it through optional `popup_metrics(model)` only when content height truly varies.
- Keep plugin `lib.rs` focused on ABI, model updates, and view composition. Move command/DBus integrations, parsers, and domain data into modules such as `system.rs` before files become god files.
- Use `Result<T, String>` for plugin-local external command/DBus errors when the error is surfaced through a plugin error queue.
- Avoid long-lived locks around UI building or external calls.
- Add comments only when code is not self-explanatory or unsafe/lifetime behavior needs documentation.
- Do not leave magic numbers in code. Use named constants for domain/protocol values, put user-tunable values in config, and use `oxiced` theme/helper tokens for UI spacing, padding, margins, border radii, and font sizes.
- Keep bar/window dimensions behind `src/layout.rs` helpers. Do not reintroduce monitor-width constants in app, message, or surface code.
- Startup retry behavior is config-driven via the `[startup]` TOML table parsed into `config::StartupRetryPolicy`. Keep timing constants and retry knobs out of `src/app.rs`; add new user-tunable startup options there (see `DECISIONS.md`).
- Single-instance behavior is config-driven via the `[instance]` TOML table parsed into `config::InstancePolicy`. Keep lock acquisition in `src/single_instance.rs`; `src/app.rs` only acquires the guard at startup (see `DECISIONS.md`).
- Add active-output compositor integrations under `src/monitor/` as small backends returning an output name; keep toast/panel layer construction in `src/messages.rs` backend-agnostic.
- When the same UI number repeats across plugins, add or use a shared `oxiced` token/helper before adding plugin-local constants.
- Extend plugins with optional capability exports when existing signatures suffice. Preserve provider-local launch indexes when ranking search results globally; never broadcast a selected result's index to unrelated plugins.
- Keep blocking plugin work in runtime-independent workers with immutable snapshots. Dylibs have independent Tokio TLS; they must not assume the host's reactor is visible. Cancel/join long-lived workers when their owning stream is dropped, and generation-gate asynchronous search results.
- Read runtime settings from immutable `ConfigSnapshot`s. Route all GUI/IPC writes
  through the revision-checked config store; never write base/Nix configuration or
  expand defaults into the sparse override file.
- Declare full dotted plugin paths in optional `settings_schema` exports. Live
  fields require a compatible `config_changed` hook; hooks receive the full
  document and must ignore unrelated changes and retain restart-only running values.
- Add shared controls/styles to `crates/oxiced` and use the workspace dependency;
  do not restore per-plugin git/path forks of that library.
- Register every surface before creation; route view/events by its known ID and
  kind. Never infer that an unknown window is a bar. Surface tasks must use explicit
  target IDs for geometry/input updates.
- Normalize output inventory off the UI thread, preserve refresh-rate precision,
  and separate logical output geometry from pixel modes/native buffer scale.
- Keep plugin service ownership independent of output/view count. Preserve source
  context for UI-initiated asynchronous work without dropping model updates after
  the originating surface disappears.
- Embedded capability views reuse the existing model and return inner content;
  host surfaces own chrome and scrolling. Factor shared popup/embedded view logic
  without creating another subscription, backend or polling owner.
- Keep thread-affine native handles on their owning worker. Bound request queues
  and operation waits, coalesce replacement actions, reject expired/offline
  commands and resnapshot after reconnect rather than replaying stale actions.
- Keep independently refreshed domains separate in the model, and generation-gate
  action acknowledgements and queries so older work cannot overwrite newer state.
- Resident command replies report completion, not queue acceptance. Apply the
  ordinary model outcome first; capture output context before starting work.
  Bound concurrent IPC handlers so slow actions do not block fast commands.
- Use the SDK's serialized host-request helpers across dylibs, rather than relying
  on SDK struct `Any` identity across independently compiled dependency graphs.
- Non-interactive surfaces must not alter keyboard ownership. Route keyboard events
  by compositor Enter/Leave, independently of pointer focus or surface creation.
- Coalesce transient OSDs per output and abort replaced timers; expiry carries a
  generation. Initial service snapshots are quiet, failed actions do not show success.
- Scope DBus state/actions to the service's unique-owner epoch. Subscribe before
  taking snapshots, reconcile signal ordering and resnapshot invalidated properties.
- Interactive protocol requests need typed, validated replies and session-scoped
  cancellation. Old request timeouts must not cancel newer sessions. Export optional
  nonblocking `modal_closed` cleanup for operations whose lifetime includes a modal;
  cleanup tasks must survive the retired surface without reopening stale prompts.
- Keep schema-driven view construction separate from settings state/actions and
  persistence. Use shared oxiced surface buttons, badges and themed switches for
  spacious forms; preserve compact bar controls and the existing theme palette.

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
- Nix Rust packages use Crane. Keep host and plugin packages wired to the shared workspace `cargoArtifacts` in `flake.nix` instead of creating per-plugin dependency caches.
- Wire native libraries into shared dependency compilation and the owning plugin's
  final build/runtime search path; keep unrelated plugin runtime inputs minimal.
