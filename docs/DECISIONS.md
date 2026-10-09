# Decisions

This file records LLM-specific continuity decisions and low-impact choices that
are useful for future agents. Omitted from this file are high-level architecture
topics already covered by `ARCHITECTURE.md`.

## Spacious settings and control-center composition (2026-10-10)

- Resume the approved shell roadmap after the multi-output slice. User requested
  Android-inspired spacing/components while retaining the oxiced theme. Replace
  cramped settings bar buttons with normal surface actions, xl card/outer padding,
  source/mode badges, readable titles/descriptions and theme-colored switches.
- Split settings views into `src/settings/view.rs`; state/actions and revisioned
  drafts stay in `mod.rs`, persistence/application stays in `runtime.rs`. Desktop
  navigation uses a sidebar, narrow navigation a category picker. Widget layout
  sections stack vertically. Preserve the first six category shortcuts; append
  Control Center as Alt+7.
- Add the host-owned centered control-center overlay and reserved `control-center`
  bar item. Default dimensions are 680x760; configured provider order defaults to
  Audio, Network, Bluetooth, Battery, Notifications and Clock. CLI/JSON commands
  use `control-center` and the existing version-1 protocol.
- Add optional `control_center_view` at ABI 6, using the existing view signature.
  Dedicated content takes precedence over legacy popup/panel fallbacks. Bundled
  control providers reuse existing models/actions; calendar remains a popup
  fallback. Do not initialize another backend or subscription per embedded view.
- Opening settings/launcher/control center dismisses the other overlay. Center
  source routing carries through async password/pairing modals; hiding a bar on
  a connected output retains the overlay, actual output removal dismisses it,
  and late plugin results still update the model.
- Viewing the notification inbox closes visible toasts and suppresses subsequent
  notification toasts without removing stored records. Selecting another provider
  restores later toasts. Preserve existing actions/replies/DND/panel behavior.
- Verification: 165 workspace tests/doctests and strict Clippy passed; Nix built
  the host and applications/audio/battery/Bluetooth/clock/network/notification
  libraries. Packaged settings/Nix-symlink regeneration smoke passed unchanged.
  Private-bus control-center smoke rendered all six providers, verified inbox/toast
  behavior, live preferences, disabled bars, overlay dismissal, scoped Escape and
  ten reopen cycles with the same seven loaded plugins.
- Inspect normal/narrow screenshots privately: theme/compositor translucency can
  reveal desktop content. Initial standard private-bus smoke autoactivated portal
  services and stalled; a bus config without service directories completed cleanly.
  No application change was inferred from that first test-environment failure.
- Native PulseAudio/BlueZ migration and brightness/power/session/OSD capabilities
  are the next milestone-4 work; this slice completes composition and visual
  consistency. Preserve staged multi-output work and the unrelated staged `result`
  deletion. No additional commit was requested during this continuation.

## Multi-output registry and inventory (2026-10-09)

- Commit the completed launcher/settings/local-oxiced work as `12d48a8`, preserving
  the pre-existing staged build-result symlink deletion outside that commit.
- Use `StartMode::Background` and explicit IDs for all bar creation, removing the
  first-unknown-window-as-main-bar assumption. Registry metadata owns kind, output
  and source-bar relationships; retired IDs return empty views/ignore resize.
- `bar.outputs = []` retains one selected output; `["*"]` targets all connected
  outputs. Named selections use configurable fallback, and `bar.enabled = false`
  leaves launcher/settings/IPC available. Per-output `bar.profiles` override common
  geometry/layout/transparency and top/bottom placement.
- ReSet reuse in this slice is domain-pattern adaptation: normalized output DTOs,
  scale/rotation/logical geometry and separate backend boundaries. Hyprland uses
  bounded read-only IPC; generic Wayland uses wl_output/xdg_output. Keep fractional
  scales/precise refresh rather than donor rounding/unsafe threading assumptions.
- Inventory refresh is one background subscription (default two seconds); unchanged
  snapshots are suppressed. A failed refresh preserves current bars/last inventory.
- Keep one shared plugin model/subscription. Optional `view_with_context` receives
  stable output/section/plugin-occurrence identity and native output scale. ABI 6
  required signatures remain unchanged.
- Source-window routing preserves popup/modal/panel ownership through asynchronous
  plugin tasks; late model updates survive output removal. Auxiliary registry
  ownership is normalized to its actual target output. Toast layout is per output.
- Deterministic multi-output tests and packaged Hyprland smoke verified three
  physical outputs, portrait widths, independent bottom profile, live selection,
  disabled bars with settings access, and temporary headless add/remove. Shared
  Applications/Clock models remained loaded. Serial Nix package builds avoided
  temporary storage exhaustion from parallel cache unpacking.
- Generic Wayland inventory was forced by removing the Hyprland session signature
  from the test daemon environment; the same three-output/profile/hotplug scenario
  passed. This is protocol fallback verification on Hyprland, not a separate
  Sway/Niri runtime claim. Existing settings/Nix-regeneration smoke and real runner
  dylib regression passed after the refactor.

## Settings foundation and GUI (2026-10-09)

- Implement the roadmap's first implementation slice: compiled defaults < base
  `config.toml` < sparse `overrides.toml`. Reset removes overrides, including
  parent pruning; explicit equal-to-base values retain GUI intent across rebuilds.
- The store performs validated, revision-checked transactions under a local
  mutex and cross-process file lock. Background workers keep writes off the Iced
  thread. Owner-only temporary writes are synced, renamed and directory-synced.
- Notify watches the config directory and resolved input files; cache watch paths
  rather than canonicalizing them on every ordinary UI update. Invalid external
  edits keep the last valid snapshot. Self writes do not advance revision again.
- Add version-1 JSON control requests/responses while retaining legacy launcher
  wire commands. The CLI uses JSON and validates actual command outcomes. The
  `control.socket` record preserves access while endpoint changes await restart.
- GUI categories are Appearance, Bar/Layout, Launcher, Plugins and Advanced.
  Global search, source/base values, reset actions, ordered widget controls,
  installed-library status, typed plugin fields and a sparse TOML editor are wired.
- Explicit empty GUI section arrays suppress the legacy all-empty fallback.
  Resetting those overrides reactivates fallback even when effective array values
  are unchanged; runtime application must consider provenance as well as values.
- Live bar geometry updates both layer size and exclusive zone and refreshes
  input regions. Settings receives initial focus once; resize does not steal
  focus from a field being edited. Launcher/settings dismiss each other's surfaces.
- Native plugin additions remain optional at ABI version 6. Current models remain
  alive. Application terminal/result-limit changes, clock text/bold/calendar
  commands, and notification timeout support live hooks; fonts/lifecycle/static
  poll timers show restart requirements. Hooks are idempotent for unrelated edits.
- Packaged Hyprland smoke verified keyboard-driven GUI edits, geometry, source
  badges through IPC, Nix symlink regeneration/reset, stale-revision rejection,
  invalid-input recovery, settings with launcher disabled, launcher activation and
  persistence across daemon restart. Private temporary fixtures were used.
- Final checks include workspace tests and strict Clippy, Nix host/application/
  clock/notification package builds, the real-dylib live-config regression, and
  Home Manager evaluation with a nonstandard XDG config home. The local Home
  Manager checkout emits a release-mismatch notice against the project's pinned
  Nixpkgs, but the relevant generated paths and override ownership evaluated correctly.

## Local oxiced workspace (2026-10-09)

- User authorized bringing shared UI code into this repository. Import exact
  upstream revision `735181be7340e5916795642efa95a11e08d8f32f` into `crates/oxiced`,
  preserving its LGPL-3.0-or-later license and recording provenance in `UPSTREAM.md`.
- Host and bundled plugins use `[workspace.dependencies] oxiced`; root lockfile
  and flake own builds. Remove the obsolete git-source vendor hash configuration.
- Shared selectable rows, settings-card chrome and editor styling now live in
  the workspace library. Root workspace tests/Clippy cover that library too.

## Resident OxiRun Integration (2026-10-09)

- Source reviewed: `/home/dashie/gits/OxiRun` (the requested lowercase `oxirun`
  path does not exist on this filesystem). Its existing application provider was
  the only local runner plugin. The source repository was not modified.
- Port application-provider behavior into `plugins/applications` using Oxibar's
  owned model/message API and optional `runner_query`/`runner_entries` exports.
  Legacy OxiRun libraries have different model/update/view signatures and must be
  ported; loading them as Oxibar dylibs is not compatible.
- Keep ABI version 6: existing signatures/layouts crossing the dylib boundary did
  not change, and the new capability symbols are optional.
- Add `oxibar launcher [toggle|open|close]` as a small Unix-socket client in the
  existing executable. Handle commands before tracing/font/GPU/daemon startup.
  The daemon acknowledges queueing a recognized request; this is not an assertion
  that the compositor has already presented the surface.
- Reserve `launcher` as a host-owned bar item. Put it before configured plugins
  only in the all-empty fallback layout; explicit layouts select it themselves.
- Keep the application index resident, search shared immutable snapshots in
  workers, bound the exported result list, cache unchanged queries, and discard
  out-of-order results. Provider-local indexes survive host global ranking.
- Activation closes the overlay immediately while the selected provider's task
  continues in the daemon; queued duplicate activation after closure is ignored.
- The first real Nix-package run exposed duplicated Tokio TLS across dylibs:
  plugin `tokio::task::spawn_blocking` and timers could not see the host reactor
  and aborted the process. Replaced those with thread/oneshot tasks and a
  cancellable/joined refresh timer; added a real-library regression test that runs
  without entering a host Tokio runtime.
- Verification: workspace tests, strict Clippy, Nix host/applications/clock builds,
  opt-in dylib regression, and isolated Hyprland smoke with both application and
  clock plugins. Typed a fixture app name and launched with Enter; the bar stayed
  alive through activation and 20 subsequent open/close cycles.
- Final release-package smoke on this machine: median CLI acknowledgement
  round trip 3.54 ms over 20 opens; median host-request-to-surface-configure 3.92 ms,
  max 11.92 ms over 21 opens. These are warm IPC/configuration measurements using
  one fixture entry, not first-frame latency or a standalone OxiRun comparison.
- `SHELL_ROADMAP.md` is the requested next-stage plan. Its first major slice is
  sparse GUI overrides above the immutable Nix/base TOML plus source/reset-aware
  settings UI; those capabilities are not implemented by the launcher change.

## ReSet Reuse Scope (2026-10-09)

- Follow-up request: review `/home/dashie/gits/ReSet` for useful audio, Bluetooth
  and monitor handling while retaining Oxibar's different architecture.
- Reviewed daemon native PulseAudio subscriptions/request handling, BlueZ
  ObjectManager/property signal logic, the Iced frontend's ID-keyed state, monitor
  compositor/protocol adapters and geometry tests, and mock Bluetooth endpoints.
- Roadmap now maps concrete source paths to incremental adapters behind existing
  Oxibar plugins. Native audio/BlueZ state and read-only monitor inventory are the
  first reuse candidates; optional display controls and advanced audio follow.
- ReSet's pairing agent has placeholder responses and its monitor implementation
  has protocol/threading/refresh-rounding assumptions. Port useful logic with
  fixtures and current protocol checks rather than copying those limitations.
- No ReSet runtime dependency or source transplant is part of the launcher
  integration. The planned service adapters preserve current UI/plugin behavior
  and do not require ReSet's daemon/plugin loader or GTK frontend.
- Local-path Nix imports can include ignored Cargo targets. A newly generated
  workspace `target/` made concurrent final source imports exceed free temporary
  space; final checks use a filtered `builtins.path` excluding `.git`, `.direnv`,
  `target` and `tmp`. Session-owned Cargo artifacts were also cleaned selectively.

## Clock Ticking Strategy (2026-08)

- Problem: a `format` that shows seconds previously required `tick_seconds = 1`,
  meaning the real system clock was polled every second.
- Decision: keep `tick_seconds` as the real-clock poll interval and derive a
  separate display interval from the format. When the format shows seconds the
  plugin emits a `Tick` every second, but each intermediate tick's timestamp is
  simulated as `anchor + monotonic elapsed`, only re-reading `Local::now()` once
  every `tick_seconds`. When the format shows no seconds the display interval is
  `min(tick_seconds, 60)`.
- Rationale: avoids a per-second system-clock read; restarts a freshness ceiling
  of one minute for minute/hour/date-only formats.
- Tradeoffs: simulated time drifts only within one poll interval and is
  re-anchored to the real clock each `tick_seconds`, so NTP and timezone changes
  are picked up at each poll. Sub-second formats are capped at 1 Hz (see
  `TECHNICAL_DEBT.md`).## Startup Retry Strategy (2026-08)

- Problem: launching oxibar from a compositor's startup commands (e.g. Hyprland
  `exec-once`) failed because the compositor socket was not yet accepting
  clients; `layershellev::WindowState::build()` calls
  `Connection::connect_to_env()?` and iced_layershell turns that failure into a
  `panic!` (`exit 101`) before the app's `Result` is ever returned.
- Decision: resolve in the host with a config-driven retry loop in `src/app.rs`
  `run()`, governed by a new `[startup]` TOML table parsed by
  `config::startup_retry_policy()` into `config::StartupRetryPolicy`
  (`enabled`, `initial_delay_ms`, `max_delay_ms`, `max_attempts`, `catch_panics`).
  Two mechanisms combine:
  1. A pre-flight probe using `wayland_client::Connection::connect_to_env()`
     that skips work while the compositor is not yet ready (cheap, and hits the
     exact same code path that currently panics).
  2. `std::panic::catch_unwind` around `run_bar()` as a safety net for any other
     startup panic.
  Backoff doubles per attempt starting at `initial_delay_ms` and caps at
  `max_delay_ms`. `max_attempts = None` (default) retries indefinitely until the
  compositor is ready.
- Rationale: keeps the fix entirely in the host, needs no compositor-side
  `exec-once` sleep and no dependency patch.
- Tradeoffs: retrying indefinitely means a permanently absent compositor will
  wait forever unless `max_attempts` is set; `catch_unwind` swallows panics that
  are not startup-transient unless they come from within the daemon run.
- Alternatives rejected: compositor-side `exec-once+sleep` (fragile, WM-specific)
  and patching/pinning a fixed `iced_layershell`/`layershellev` (see
  `TECHNICAL_DEBT.md`).

## Startup Retry Follow-up: Empty `WAYLAND_DISPLAY` (2026-09)

- Problem: the pre-flight probe still waited forever on real setups. Root cause
  was a stale/empty `WAYLAND_DISPLAY` (Hyprland `exec-once` fires before the
  variable is populated): `Connection::connect_to_env()` treats `""` as a
  relative name, joins it onto `$XDG_RUNTIME_DIR` (a directory), and fails, so
  `compositor_ready()` never became true. The waiter also held the
  single-instance `flock`, so a manual restart with a correct environment died
  with `AlreadyRunning` - the bar "no longer opened" with no visible error.
- Decision: `connect_wayland()` in `src/app.rs` tries `connect_to_env()`
  first, then falls back to scanning `$XDG_RUNTIME_DIR` for a connectable
  `wayland-*` socket (`UnixStream::connect` + `Connection::from_socket`). The
  winning connection is passed explicitly via
  `Settings::with_connection = Some(WithConnection::Value(..))`, which the
  iced_layershell daemon path forwards to `layershellev::WindowState::build`,
  so surface creation no longer depends on the inherited environment.
- The probe now requires only `wl_compositor` + `zwlr_layer_shell_v1`
  (previously also `wl_output`, which the backend never hard-requires - a
  false negative here waits forever, while a false positive only costs one
  retry attempt).
- Verified live on Hyprland: `WAYLAND_DISPLAY=` empty starts and shows a
  layer surface; second copy still exits 1 with the `allow_multiple_instances`
  hint. `src/single_instance.rs` needed no change (flock logic was correct).

## Single-Instance Guard (2026-08)

- Problem: rerunning oxibar (e.g. after a `nixos-rebuild switch` regenerates the
  config) spawned a second identical bar instead of replacing the first.
- Decision: enforce a config-driven single-instance lock in the host. The
  `[instance]` TOML table is parsed by `config::instance_policy()` into
  `config::InstancePolicy` (`allow_multiple_instances`, optional `lock_path`). A
  new `src/single_instance.rs` module takes an exclusive `flock` on a lock file
  (default `$XDG_RUNTIME_DIR/oxibar.lock`, falling back to
  `get_oxirun_dir()/oxibar.lock`). The guard is acquired once at the top of
  `src/app.rs` `run()` before the startup retry loop and held for the process
  lifetime; the RAII `SingleInstanceGuard` releases the lock on drop, so crashes
  and core dumps never leave a stale lock.
- Behavior when a second instance is denied: it prints an error to stderr
  mentioning `[instance] allow_multiple_instances = true` and exits non-zero
  (`std::process::exit(1)`), matching the existing `Message::Exit` exit pattern.
- `allow_multiple_instances = true` skips the lock entirely - the documented way
  to run different bars on different monitors. A distinct `lock_path` lets two
  separate configs run concurrently while still guarding each of them from
  duplicates.
- Rationale: flock is kernel-managed (auto-released on process death, no PID
  liveness probing), needs no new vendored crate beyond already-locked `rustix`,
  and keeps the fix entirely in the host.
- Tradeoffs: the guard is best-effort protection, not authorization; it only
  coordinates oxibar processes that share a runtime dir. If the lock file cannot
  be opened (e.g. unwritable runtime dir), the host logs a warning and continues
  without a guard rather than refusing to start.

## Input Region Boot Race (2026-09)

- Problem: the bar surface is oversized by design (`bar height +
  POPUP_MAX_HEIGHT`, 451px) with clicks restricted via `SetPopupInputRegion`.
  The boot-time region task (`initial_input_region_task`, 50ms delay) races
  layer-surface creation, and iced_layershell 0.17 silently drops
  `SetInputRegion` while no surface is ready (early `return`, only a warning
  when the `wl_region` object itself is missing). Losing the race leaves the
  Wayland default input region (whole surface), so the invisible popup-reserve
  strip swallows clicks meant for windows below the bar (e.g. browser tabs).
  Slow startups (debug builds, post-restart reconnects) lose reliably, and
  `update_bar_size` previously skipped re-applying whenever the reported size
  matched the config default.
- Decision: `update_bar_size` in `src/app.rs` now always emits
  `current_input_region_task()` on the first size event for the main window
  (`Window::Opened` also maps to `LayerSurfaceResized`, so the surface is
  guaranteed to exist), via a tested `should_refresh_input_region()` predicate.
  The 50ms boot task stays as an early attempt; duplicates are harmless.
- Rationale: converge the region from an event that implies surface existence
  instead of a blind timer; host-side only, no dependency change.
- Tradeoffs: a pathological ordering inside iced_layershell (action processed
  before its `UpdateInputRegion` event) could still drop one application, but
  two independent chances (boot task + first-ready event) make that negligible.
