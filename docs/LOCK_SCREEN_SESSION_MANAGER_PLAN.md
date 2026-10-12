# Native Lock Screen and Kiosk Session Manager

**Status:** implementation in progress. Stages 0 (backend audit/state boundaries)
through 2 (owned protocol backend, private PAM worker and authenticated frontend)
are complete. `oxilock` requires an administrator-owned `/etc/pam.d/oxilock` policy;
`--check` probes protocol support without locking. Greeter, packaging/VM integration
and visual/suspend stages follow in separate commits.

## Product and process boundaries

Keep Rust, Iced and `oxiced`. Use Hyprlock's protocol-backed locking and Noctalia's
coherent visual language as references. Share theme tokens, fonts, input/card
components and read-only configuration parsing, not the running shell or plugin
loader.

Deliver two independently packaged executables (working names):

| Executable | Role | Surface backend | Authentication owner |
| --- | --- | --- | --- |
| `oxilock` | Lock the current user's existing Wayland session | `ext-session-lock-v1` | PAM for that user |
| `oxisession` | Login greeter and desktop-session chooser | Standard Iced `xdg-shell` window, fullscreen in a kiosk compositor | `greetd` |

Here, “session manager” means the login/session chooser and its handoff into the
selected desktop. `greetd` remains the privileged login manager: it opens the PAM
session, changes identity and starts the authenticated user's command. Oxibar's
existing session-action tiles remain the in-session logout/suspend/power UI.

Suggested workspace boundaries:

```text
crates/oxi-auth-ui/       shared prompt state and themed authentication views
crates/oxi-lock/          protocol state, output surfaces, locker frontend
crates/oxi-pam/           fixed-service PAM conversation worker for the locker
crates/oxi-session/       greeter frontend, greetd client, session catalog
nix/                     separate packages and NixOS/Home Manager integration
```

The visual crate must not depend on PAM, greetd, the shell daemon, dynamic plugins,
notification services or compositor-specific window-management APIs. Authentication
and surface lifecycle use separate typed state machines. No authentication state or
credentials travel through Oxibar's general-purpose control socket.

## 1. Protocol-backed lock screen

### Backend selection spike

Audited `iced_sessionlock` / `sessionlockev` 0.17.1, matching the existing Iced 0.14
family. The selected implementation is an owned `wayland-client` adapter with Iced
CPU rendering: the candidate does not expose lock-state events and treats generic
window close as unlock. See `AUTH_BACKEND_AUDIT.md`; no candidate dependency is added.

The spike must expose lock acquisition/readiness, exact configure/ack handling,
all output lifecycle events, authentication-gated unlock and final display sync.
Audit keyboard ownership, input methods, renderer failure and device removal,
including the pending-repeat lifecycle fix already used by Oxibar. If the wrapper
does not expose these requirements, extend its backend or implement the missing
protocol adapter with the existing `wayland-client` / `wayland-protocols` toolset.

### Lock lifecycle

1. Connect to the existing compositor and discover
   `ext_session_lock_manager_v1`. If unavailable or denied, report failure and do
   not claim to have locked. A layer-shell/fullscreen overlay is not a fallback lock.
2. Request a lock and immediately create one role-free `wl_surface` and one lock
   surface per advertised output. Cover newly added outputs during the lock too.
3. Do not attach or commit a buffer before the first configure. Acknowledge the
   latest configure serial once, then commit an opaque buffer matching its exact
   surface-local width/height. Respect buffer scale/viewport transforms; never
   commit a null buffer. Destroy surfaces for removed outputs.
4. Enter `Locked` only on the protocol's `locked` event. Only then signal readiness
   (`--ready-fd` / a scoped service notification). The compositor guarantees that
   all outputs already show locked content or its opaque blanking fallback and
   normal-client input is blocked. Suspend integration waits for this event.
5. One authentication transaction serves all output views. Switch the active
   prompt with keyboard/pointer focus without duplicating attempts or secrets.
6. After successful authentication, issue `unlock_and_destroy`, clean up lock
   surfaces, and wait for `wl_display.sync` before exiting. Escape cancels a prompt
   or clears input; it never unlocks.
7. Track whether `locked` was received when processing `finished`: use `destroy`
   before locking, or `unlock_and_destroy` after locking as required by the protocol.
   A compositor-authorized alternative unlock is distinct from a denied request.

Use explicit states such as `Connecting → Acquiring → Locked → Authenticating →
Unlocking → Finished`, with denial/disconnection represented separately. Rendering
and authentication failures must not issue an unlock. A crashing client must leave
the compositor locked; recovery or replacement-client takeover is compositor
policy and must not be promised universally.

### Authentication and UX

- Authenticate only the current session user, through a fixed, administrator-owned
  PAM service such as `oxilock`. Never load a user-selected PAM module/service.
- Run blocking PAM work outside Iced's event loop, preferably in a small same-user
  worker process with private, bounded IPC. PAM supplies account policy and
  conversation prompts, including visible/secret questions and MFA. Do not assume
  that every authentication exchange is a single password.
- Keep one in-flight attempt, use attempt IDs to reject late responses, clear
  submitted/cancelled secrets and audit Iced text-input/message copies. Masking a
  text field is only visual protection. Use zeroizing secret storage where the
  API allows it; exclude credentials from `Debug`, logs, config and crash reports.
- Initial UI: opaque background, clock/date, identity, current authentication
  prompt, layout/Caps Lock indicator and readable error feedback. Use static
  fallback rendering if wallpaper/image loading fails. Reduced motion applies.
- Start with password/PAM conversations and multi-output correctness. Add avatars,
  wallpaper/blur and optional media information after the lock lifecycle passes
  its tests. Notifications, clipboard paste/history and arbitrary plugins are
  disabled in the initial locker. Suspend/power shortcuts are optional explicit
  integrations, not part of authentication.

### Shell and suspend integration

Keep the existing configurable System lock command working. Once `oxilock` is
implemented, deployments can set that command to `oxilock`; no plugin ABI change
is required. Add a ready-aware lock-then-suspend path instead of treating process
spawn as proof of locking. Integrate logind's `PrepareForSleep` with a bounded delay
inhibitor: request locking first, release the inhibitor on confirmed readiness,
and report refusal/timeouts. Test resume on the same locked connection.

## 2. Kiosk greeter / session manager

### Frontend and session selection

Use an ordinary fullscreen Iced application so Cage does not need layer-shell or
session-lock support. Cage and Hyprland kiosk mode both host this same frontend.
The greeter is not a lock screen: it runs before the user's session exists.

Initial UI: user selection or username entry, selected Wayland desktop session,
PAM conversation prompts supplied by greetd, keyboard-layout/Caps Lock state,
bounded authentication errors and a clearly scoped shutdown/reboot menu. Keep
graphics/themes system-owned for this pre-login process. Separate development/demo
mode from actual login; launching the UI inside an existing user session cannot
change UID or create a privileged login session by itself.

Read trusted system `wayland-sessions/*.desktop` entries, including Nix-provided
directories. Parse `Name`, `TryExec`, `Exec` and desktop metadata into validated
argv, without `sh -c` or an arbitrary command text box. Handle supported desktop
Exec field codes deliberately and reject unsupported entries with diagnostics.
Keep session environment additions small and explicit (`XDG_SESSION_TYPE`,
desktop/session identity); let greetd/PAM own the base environment. X11 session
wrappers can be a later independently tested extension.

### greetd conversation and handoff

Use `GREETD_SOCK`, preferably through a pinned Rust greetd IPC client. The framing
is a native-byte-order 32-bit length followed by UTF-8 JSON; impose a size bound
and use complete reads/writes. Do not manually treat the socket as newline JSON.

```text
create_session(username)
  → auth_message(visible | secret | info | error), zero or more times
  → post_auth_message_response(response or no response for informational messages)
  → success
start_session(cmd: argv, env: KEY=value list)
  → success
terminate the greeter and its kiosk compositor
  → greetd starts the authenticated session on the released seat/VT
```

Cancel or changing users sends `cancel_session`; handle `auth_error` without
crashing. Do not impose a fixed number/type of PAM questions. Distinguish successful
authentication from successful `start_session`. Exit for handoff only after the
start request is acknowledged. Never launch the user compositor as a child of the
still-running greeter compositor.

### Deployment

Cage is the simplest initial deployment. An illustrative greetd configuration:

```toml
[terminal]
vt = 1

[default_session]
command = "cage -s -- oxisession"
user = "greeter"
```

The greeter user is unprivileged; greetd remains the privileged daemon. Cage exits
when its greeter child exits, releasing display/seat resources for handoff. Package
absolute command paths in the NixOS integration rather than relying on a login
shell's `PATH`.

For Hyprland, ship a separate minimal kiosk configuration and owned launch wrapper:
one fullscreen greeter, no bar/user startup hooks, no desktop-launch/terminal
bindings. On acknowledged session start, close the greeter and then terminate the
owned Hyprland instance. Verify greetd waits for the whole default-session command
to exit before starting the user session. On failure/crash, show an error or let
greetd restart its default session; do not leave an empty kiosk compositor running.
Do not reuse or modify the user's normal Hyprland configuration.

NixOS owns greetd, the greeter account, PAM configuration, kiosk wrapper and seat/VT
integration. Home Manager may configure/install the per-user locker and shell lock
command, but cannot supply the privileged display-manager/PAM setup. Keep both
executables independently installable, pinned and buildable for the current
`x86_64-linux` and `aarch64-linux` flake systems.

## Delivery sequence and acceptance gates

| Phase | Deliverable | Required gate |
| --- | --- | --- |
| 0 | Backend/protocol and greetd/Cage spikes; finalize binary/crate boundaries | Confirm Iced version compatibility, readiness hooks and kiosk teardown semantics |
| 1 | Minimal opaque protocol locker with fake authentication | Fake Wayland server validates configure/ack/dimensions, denied/finished states, display sync, hotplug and crash-while-locked behavior |
| 2 | Locker PAM worker and shared prompt view | Fake conversations cover visible/secret/info/error/MFA, cancellation, stale responses and failure; dedicated PAM integration test confirms real policy |
| 3 | Minimal kiosk greeter and greetd client | Fake Unix server covers framing, partial I/O, all prompts, cancel/retry/start rejection and successful handoff acknowledgement |
| 4 | Cage deployment, then owned Hyprland kiosk wrapper | NixOS VM verifies real greetd login, user/seat/VT handoff, logout-to-greeter, crash recovery and failed-auth retention |
| 5 | Polished themed UI and shell/suspend integration | Owned compositor screenshots/input checks, multi-output/fractional-scale coverage, confirmed lock-before-suspend and resume |

Nested compositor tests validate rendering and protocol behavior without touching
the developer's session. They cannot prove real seat/VT handoff: keep that gate in
an isolated NixOS VM. Use deterministic fake authentication by default; actual PAM
tests require an explicit disposable fixture user/service, never real credentials.
Add package/build/check targets only when their phase is implemented; this plan
does not claim new runtime capabilities or replace the existing lock command.

## References

- [Official ext-session-lock-v1 XML](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/main/staging/ext-session-lock/ext-session-lock-v1.xml)
  and [readable protocol reference](https://wayland.app/protocols/ext-session-lock-v1).
- [`iced_sessionlock` 0.17.1 API](https://docs.rs/iced_sessionlock/0.17.1/iced_sessionlock/)
  and [backend upstream](https://github.com/waycrate/exwlshelleventloop).
- [greetd overview](https://man.sr.ht/~kennylevinsen/greetd/)
  and [greetd IPC specification](https://man.archlinux.org/man/greetd-ipc.7.en).
- [Cage](https://github.com/cage-kiosk/cage),
  [Hyprlock](https://github.com/hyprwm/hyprlock),
  [Noctalia](https://github.com/noctalia-dev/noctalia-shell).
