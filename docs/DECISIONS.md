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
