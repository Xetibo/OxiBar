# Decisions

This file records LLM-specific continuity decisions and low-impact choices that
are useful for future agents. Omitted from this file are high-level architecture
topics already covered by `ARCHITECTURE.md`.

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
