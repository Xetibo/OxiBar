# Authentication backend audit

## Selected boundaries

- `oxi-auth-ui` owns bounded response storage, attempt epochs and reusable prompt
  interaction. It neither authenticates users nor loads shell plugins.
- `oxi-lock` owns `ext-session-lock-v1`, output surfaces, readiness and the only
  unlock request. The application may request unlock only after its current PAM
  transaction succeeds. Ordinary close, Escape and renderer failure do not unlock.
- `oxi-pam` will own a fixed-service PAM conversation in a cancellable child
  process. Its private inherited channel carries bounded, redacted credentials.
- `oxi-session` owns greetd conversation and validated session selection. It exits
  for handoff only after `start_session` is acknowledged; greetd owns user/seat/VT
  transitions after the entire kiosk default command exits.

## Iced session-lock candidate

Audited upstream `iced_sessionlock` / `sessionlockev` **0.17.1**, matching the
repository's Iced 0.14 family. It is not selected as the locker backend:

- `sessionlockev` delegates `ExtSessionLockV1` events to a no-op handler. `locked`
  readiness and `finished` denial/external-unlock are not exposed.
- `iced_sessionlock::multi_window::run_action` maps window Close to unlock, and
  application action conversion also bypasses the application update handler.
- Output removal requires a dead `wl_surface` as well as a removed output, which
  does not correctly represent normal output hot-unplug.
- Keyboard lifecycle retains pending repeat work after device loss, similar to
  the defect already fixed in this repository's layer-shell backend.

The owned adapter uses `wayland-client` / staging `wayland-protocols` directly and
Iced's CPU renderer over opaque shared-memory buffers. This avoids adding another
unpatched window/event backend or depending on GPU initialization for coverage.
It preserves Iced/oxiced styling and ordinary Iced xdg-shell for the Cage greeter.

## Verification boundaries

Protocol tests use an owned Wayland server, never the desktop connection.
Authentication tests use owned conversations and a disposable PAM fixture.
Greeter tests use private Unix sockets with partial framing and rejected starts.
Real user, DRM/seat and VT handoff require the dedicated NixOS VM test; nested
compositor success alone is not evidence of a working display-manager handoff.

Sources: [lock protocol](https://wayland.app/protocols/ext-session-lock-v1),
[candidate event backend](https://docs.rs/sessionlockev/0.17.1/src/sessionlockev/lib.rs.html),
[candidate Iced runtime](https://docs.rs/iced_sessionlock/0.17.1/src/iced_sessionlock/multi_window.rs.html),
[greetd IPC](https://man.archlinux.org/man/greetd-ipc.7.en).
