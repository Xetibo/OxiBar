# Decisions

## 2026-10-13: Greeter prompt focus and Hyprland kiosk default

- The greeter still used the legacy unfocused prompt view, ignored response-focus
  messages and subscribed to only ignored key events. Wire the shared explicit
  input/submit/cancel controller into its frontend, consume captured prompt keys,
  and fall back to modified character keys when native text is missing. Preserve
  ordinary username-widget capture, redacted keyboard messages and zeroized secrets.
- New visible/secret prompts focus the response; clicking it restores focus.
  Tab/Shift+Tab cycle enabled controls, focused buttons do not edit the response,
  Enter/Space activate actions, and cancellation restores the username field.
  Root-peer validation, serialized cancellation and acknowledged-only handoff remain.
- Default both the NixOS module and no-argument kiosk CLI to Hyprland. Retain Cage
  as an explicit choice and keep the owned minimal compositor configuration and
  PID/signature-bound teardown. Module assertions cover both generated commands.
- Two private greetd frontend fixtures exercise captured/missing-text keys,
  multi-prompt focus, a real Iced masked-field click and clear without authentication.
  All 36 normal authentication tests and strict all-target Clippy pass. The Hyprland
  VM leaves the backend option unset and passes real PAM failure/retry, Tab/Shift+Tab
  editing isolation, accepted login, seat handoff, lock/unlock and crash recovery.
- Use the focused prompt's Tab/focus hint for OCR readiness: its password label was
  present in failed-run screenshots but missed by OCR. Authentication success still
  depends on PAM, acknowledged handoff and process/seat assertions.

## 2026-10-13: Deployment ABI, Wayland endpoint and themed lock input follow-up

- The installed bar inherited empty `WAYLAND_DISPLAY`, recovered a socket for its
  own renderer, then forwarded the empty value to applications. New retained stderr
  showed actual Nheko/Qt and OpenCode/Electron Wayland connection refusal. Normalize
  the daemon environment before threads start and share bounded live-socket recovery
  with child handoff. Preserve valid explicit displays, including absolute paths.
  Transient `UnsetEnvironment` prevents manager-side FD/absent override inheritance;
  empty GBM/LIBVA/GLX driver selectors are discarded. No duplicate uncertain handoff.
- Installed auth binaries used glibc 2.40 while system PAM/Mesa required
  `GLIBC_2.43`; read-only module loads reproduced both failures. PAM could not reach
  a password prompt, and CPU greeter selection did not fix the kiosk's EGL loader.
  Factor `lib.mkAuthenticationPackages pkgs` / `nix/auth-packages.nix` and use the
  consuming package set for NixOS/Home Manager defaults and their compositor graph.
  Keep standalone packages pinned and shell/plugin ABI builds together. Cage <0.3
  retains the fullscreen/xdg-shell patch; newer Cage uses upstream support.
- Use compositor cursor-shape with Enter serials and Iced default/text/button
  interaction; retain immutable SHM only as a protocol fallback. Dispatch widget
  focus and native keys in arrival order before rendering, preserving click-then-type
  in one batch. Native-XKB shared-password and themed-cursor fixtures pass without
  moving secrets into ordinary widgets or changing authenticated-only unlock.
- Caller-package-set module assertions, all fourteen Home Manager checks and modern
  authentication builds/tests pass.
  Actual system PAM/Mesa libraries load with the matched runtime. Owned Cage GLES2
  context creation passes against the real system Mesa vendor after clearing the
  fixture's pinned graphics overrides; the pixman sleep VM was not EGL coverage.
- Actual installed Nheko and OpenCode open via the packaged launcher in a private
  Sway/HOME/bus with intentionally empty incoming display. Packaged Hyprland UI smoke
  also passes desktop/command endpoint and FD assertions plus native clipboard copy.
  Separate nested runtime directories prevent recovery from selecting the parent.
- Workspace tests/strict Clippy and native authentication checks pass. Cage VM
  passes real login, seat handoff, password lock/unlock, hotplug/repeat, both S3 paths
  and crash recovery. One run hit asynchronous explicit-suspend inhibitor retirement;
  retain timeout inhibitor diagnostics and the successful rerun without weakening
  readiness/authentication gates or changing production suspend policy.

## 2026-10-13: Launcher command isolation and native authentication input/rendering

- Extract PATH command indexing/search/terminal wrapping from Tools into optional
  `plugins/commands` / `libcommands.so` / `oxibar-commands`. Keep named configured
  actions in Tools and `Terminal=true` desktop handling in Applications. Commands
  defaults to `>` only; `commands.search_unprefixed` explicitly restores merged
  suggestions. Include it in the Home Manager, launcher-default switch and fourteen-
  library development installer. Move command preferences to `[commands]`.
- The installed Nheko and OpenCode desktop entries are valid and reachable through
  Nix profile data roots. Default PATH suggestions used higher raw scores and could
  crowd desktop results out of the global seven rows. Search executable/desktop-ID/
  window-class aliases with comparable exact/prefix/partial/fuzzy scores and verify
  both desktop entries through a symlinked-profile fixture.
- Preserve original incoming library/font/Vulkan settings before Nix wrapper/dev-
  shell overrides; restore them through the shared SDK for transient and direct app
  handoffs. Keep actual user overrides and the no-duplicate uncertain-submission
  rule. Journal transient stderr and inherit direct stderr for post-exec failures.
  OpenCode opens in an owned Sway/private HOME/bus with inherited and restored
  development graphics; earlier user-service Electron SIGTRAPs are present in the
  journal but their original stderr was discarded and the crash is not reproduced.
- The locker already received pointer events but never published a cursor and its
  masked response had no focus target. Add a per-seat immutable ARGB cursor and
  explicit shared response/submit/cancel focus. Tab/Shift+Tab skip disabled controls;
  mouse response focus, Enter/Space button activation and focused-only editing retain
  bounded zeroized secrets. New prompts automatically focus the response. Only PAM
  success remains an unlock capability.
- Select the greeter's `tiny-skia` backend at binary entry before any worker threads
  or GPU adapter initialization, overriding inherited wgpu preferences. Authentication
  Nix packages use RPATH for runtime libraries and fonts without forcing a Vulkan ICD
  on Cage/Hyprland. Keep root-peer greetd validation and acknowledged seat handoff.
- Normal workspace tests, strict Clippy, targeted provider/auth checks, the private
  user-manager lifetime/no-duplicate fixture, all twelve Home Manager checks and
  fourteen-library installer preflight/custom-root checks pass. Nix host, Applications,
  Commands, Tools and authentication packages build. The real Cage VM passes with
  invalid Vulkan ICD paths and inherited `ICED_BACKEND=wgpu`, including login, seat
  transfer, lock/unlock, two suspend/resume paths and crash recovery.
- The packaged private Hyprland UI smoke passes actual separate command-provider
  activation with literal quoted arguments alongside desktop application activation
  and the retained UI flows. Preserve existing command subsequence matching when
  extracting the provider.

## 2026-10-13: Plugin-owned persistent clipboard backend

- Empty history was caused by an enabled view-only provider without a running
  external daemon. Make OxiPaste self-contained: its enabled `Always` subscription
  starts the backend with Oxibar, captures with views/bar hidden and owns cleanup
  on disable. This supersedes the earlier separately configured daemon requirement.
- Keep the backend inside `liboxipaste.so`, already bundled by the shared module.
  Split compatible atomic history storage, native Wayland ownership and D-Bus
  compatibility into `service/history.rs`, `service/native.rs` and `service/mod.rs`.
  Reuse the existing locked Wayland/rustix/tempfile graph; no extra daemon binary,
  module service, host ABI capability or flake input is needed.
- Preserve `$XDG_CONFIG_HOME/oxipaste/items`, binary/legacy formats, mode `0600`,
  exact bytes/MIME and bounded history. Reject corrupt files without overwriting,
  roll back failed mutations and add restart-only `oxipaste.max_items` (default 100).
- Prefer ext-data-control with wlr fallback. Bound queues, offers, sources and
  pipe transfers; use wakeable deadlines and joined cancellation. A display sync
  acknowledges native clipboard ownership before host focus handoff. An owner
  marker prevents self-copy from reordering or reinserting deleted history.
- Own the existing `org.Xetibo.OxiPasteDaemon` API for standalone compatibility.
  An existing updated external owner remains authoritative and uses bounded
  polling. Duplicate streams wait for the previous owner; cancellation releases
  native/D-Bus resources before ownership can restart, including retained models.
  Legacy `ShutDown` cannot terminate Oxibar or bypass plugin enablement.
- Native snapshots update the shared model immediately and supersede stale manual
  refreshes. Unavailable snapshots retain cached data; a confirmed empty snapshot
  clears it. Views distinguish backend startup/unavailability from empty history
  and retain action failures across healthy capture updates.
- Extend the isolated runner with private HOME/XDG config/data/state as well as
  bus/compositor/runtime/cache. The real dylib now auto-captures and must never use
  inherited user history or clipboard. Extend the clipboard smoke with a daemon-free
  default scenario while retaining external-daemon/standalone compatibility mode.
- Verified normal workspace tests and strict Clippy; private D-Bus acknowledgement,
  real dylib cancellation/replacement without host Tokio, and native daemon-free
  capture/copy/paste/persistence/disable/re-enable checks pass. The external-daemon
  and standalone compatibility smoke also passes in the owned compositor/session.
- Final Nix host/provider builds use the filtered working source so new service
  modules are included. Both native smoke modes pass against the packaged binaries
  without focus tracing, and the packaged dylib passes owned cancellation/replacement
  with no host Tokio reactor. Formatting and diff whitespace checks pass.

## 2026-10-12: Bundle the clipboard provider in module defaults

- The shell clipboard command exposed a packaging gap: the shared Home Manager
  module, also exported as `nixosModules.home-manager`, omitted `oxibar-oxipaste`
  from its default enabled packages. Add it so enabling the module installs
  `liboxipaste.so` in the configured XDG plugin directory and enables it in the
  generated immutable base. This supersedes the earlier default-set exclusion.
- Explicit `programs.oxibar.config.plugins` lists remain authoritative;
  `extraPlugins` continues to install disabled catalog entries. The separately
  packaged OxiPaste daemon and its opt-in service remain configured by its own
  module. No shell/daemon dependency or automatic session startup is introduced.
- Expand `nix/hm-test.nix` to exercise default packages through the NixOS alias,
  generated clipboard enablement, package installation and custom XDG library
  source/target alongside the existing explicit-list and service checks. The new
  regression failed before the default-list fix and passes afterward.
- Verified all ten module checks with Home Manager release 25.11 aligned to the
  pinned nixpkgs, pinned Alejandra formatting, and Nix host/provider package builds.
  Packaged CLI help includes the clipboard command; no activation was required.

## 2026-10-12: Shell clipboard picker command

- Add `oxibar oxipaste [open|close|toggle]`; a bare command toggles the running
  shell's picker, using the same versioned window IPC as other exclusive overlays.
  Close is idempotent; missing/disabled providers or absent popup capability return
  an actionable error before dismissing any existing surface.
- Host-owned `src/oxipaste.rs` centers a bounded exclusive layer on the selected
  output and renders the existing provider popup body. Retain the one shared
  model, history/search state and visible-only subscription; no command ABI export
  or independent daemon/clipboard owner is needed. Layout placement is optional.
- Mirror the stable `oxibar-oxipaste-search` widget ID in the host and optional
  provider so native configure focuses the search. Shared form traversal and close
  bindings stay surface-scoped. Host chrome adds a configured close hint/footer.
- Register before native creation, retain auxiliary ownership when a bar is
  hidden, retire on provider disable/output removal/native closure and dismiss
  alongside other exclusive overlays. Include this surface in the acknowledged
  paste path's native focus-release barrier.
- Add focused host lifecycle/IPC regressions and extend the owned clipboard smoke
  with the real shell CLI, keyboard search, centered geometry, bar-independent
  Copy/Paste and provider-disable handling.
- Verified Rust formatting, all normal workspace tests, strict workspace Clippy
  and host/provider builds through the flake development shell. The extended
  native smoke passes with the real dylib and separately Nix-built OxiPaste daemon
  and UI in an owned compositor/session bus, including all clipboard command and
  hidden-bar Copy/Paste checks.

## 2026-10-12: OxiPaste widget and acknowledged native paste

- Add optional `plugins/oxipaste` / `liboxipaste.so`, packaged as `oxibar-oxipaste`.
  Share the updated external daemon's persistent text/image history across the bar
  popup, control center and interactive desktop widget. Keep the daemon opt-in and
  separate from Tools' ephemeral launcher clipboard owner.
- Use exact bytes/MIME in the daemon's new payload-addressed CopyContent/DeleteContent
  methods. Await real clipboard completion; separate Copy retains the picker and
  row-click Paste asks the host to hand off focus. Healthy polls retain action errors.
- Add additive ABI 6 string request/result helpers for restricted Ctrl+V/Ctrl+Shift+V.
  The host dismisses interactive surfaces and waits for native Unfocused/Closed,
  with bounded generation-scoped timeouts and cancellation on renewed shell focus.
  The Nix host includes wtype. No fixed-delay focus heuristic is used.
- Route bar clicks through a plugin-local Open message returning the host request
  from its update task; direct bar strings bypass emission translation.
- Fix layershellev keyboard capability loss: clear the focused surface and emit
  Unfocus when a temporary keyboard disappears without Leave. This prevents stale
  host focus state and paste timeouts; native Leave remains the ordinary barrier.
- Extend the Cargo installer to thirteen libraries while leaving OxiPaste outside
  Home Manager's default enabled set. Document catalog/base enablement and daemon
  startup, plus restart-only shortcut and visible refresh configuration.
- Verified normal workspace tests, strict workspace Clippy/builds, delayed private
  D-Bus success/failure and the real dylib without host Tokio TLS. The owned native
  smoke passes original watcher bytes/newlines, reordering-safe Copy, picker retention,
  actual application Ctrl+V/Ctrl+Shift+V output, failed-copy history preservation,
  standalone Copy/Enter and private binary persistence/restart. The real session's
  clipboard/input was never used. The Nix host/plugin/OxiPaste packages build and
  pass the same native suite, including keyboard-device removal without Leave.
  Daemon opt-in/custom target/null-package and catalog option fixtures pass; all
  three package derivations evaluate on x86-64 and AArch64 (native execution is
  x86-64). The thirteen-library installer passes both missing-library gates and
  custom XDG/absolute-relative target paths. Preserve concurrent authentication edits.
- Rust formatting and OxiPaste's Nix formatting pass. Full-file Alejandra reports
  existing authentication-block formatting in Oxibar's flake; the clipboard flake
  diff consists of the formatted helper and package entries.

## 2026-10-12: Authentication rebuild — private lease ownership

- Reproduce the logind lease `WouldBlock` failure with parallel locker library
  tests pinned to one CPU. The isolated lease test passes repeatedly; scheduling
  and concurrent process spawning expose the FD lifetime race.
- Audit authentication fixtures during an active-Oxi rebuild. Locker protocol,
  logind and greetd tests use private socket pairs; PAM uses a test-only service
  directory/module and declared compiler/library inputs. No live desktop service
  or current-session resource is needed by these Cargo tests.
- Retire unrelated child descriptors with `close_range(CLOSE_RANGE_CLOEXEC)` after
  installing readiness FD 3. D-Bus SCM_RIGHTS reply copies can lack close-on-exec
  even when the returned lease has it. Marking rather than immediately closing
  preserves Rust's exec-error pipe; any failure aborts spawning with native errno.
- Observe inhibitor EOF with a bounded one-second wait, including the private
  Suspend fixture. Reply tasks and concurrent forks can briefly retain copies;
  release must still happen while the locker is alive. Readiness/refusal gates
  and production lease-release timing remain unchanged.
- Add a deterministic non-close-on-exec inhibitor regression and an exec-failure
  regression. The FD test fails against the old spawn path even with the bounded
  EOF wait, detecting actual inheritance rather than only an immediate-read race.
- Verification passes: 30 authentication tests in the Nix sandbox, strict all-targets
  authentication Clippy, both packages, formatting and 100 parallel nine-test locker
  library runs pinned to one CPU. Use a filtered working-source flake snapshot to
  include the concurrent untracked workspace member while excluding build artifacts.
- Verified outputs under `/nix/store/`: locker `ryfcwk6nba73f1jrhncf7dq9r6ybls8h`,
  greeter `mbj5cgfr20f24qfl411qy14jbzmkz09n`, unit check
  `xqpxwpp01sdxrb531fzmp244bjnj6f9m` and Clippy `j94633y65795fyri1dp47hnfjzay9lrk`.

## 2026-10-12: Authentication stage 5 — visuals, input and ready-aware sleep

- Complete the isolated Rust/Iced authentication delivery. Shared read-only settings
  install existing theme tokens, clock/date, bounded PNG/JPEG/WebP assets and opaque
  fallback composition. System greeter assets retain lexical/canonical ownership
  checks; user locker appearance remains separate from shell/plugin configuration.
- Keep avatars beside the clock and bounded scrolling outside the centered content
  width. Empty feedback does not consume vertical rows; power controls remain visible
  at 800p. Greeter Ctrl+Shift+P/R opens permission-gated confirmation without typing
  into Username, and Escape/mouse cancellation restores username focus.
- Enforce the Nix kiosk layout through the greeter command environment: greetd clears
  service-level environment. Locker indicators use native XKB; generic Iced greeter
  Caps starts unknown and derives only observed state. Repeat stores physical keys,
  retires on focus/keymap/device loss and regenerates text from current XKB state.
- Honor per-output Iced redraw deadlines. Correct CPU BGRA-to-native-XRGB packing;
  a real rendered-frame corner check catches red/blue inversion, and final native
  avatar/after-resume screenshots show the requested blue palette.
- Add `oxilock-sleep --watch` and `--suspend`. Delay inhibition starts after signal
  subscription, remains held through exact live-child protocol readiness, is bounded
  by logind policy and is reacquired on resume. Explicit suspend refuses readiness
  failure. Watcher KillMode=process preserves active locker/PAM on unit stop; PAM
  worker parent-death handling retires it when the authentication frontend crashes.
  Pre-exec failures return native errno without allocating after fork.
- Standard polkit is supplied by the NixOS integration; no custom authorization
  bypass. Explicit suspend must run in the active desktop's policy context. HM
  remains user-only. Add independent all-auth `authentication-unit` flake checks.
- Native S3 diagnostics located guest virtio-GPU fence/logind stalls. Cage's sleep
  fixture uses legacy VGA/pixman and an EROFS store with protected sticky overlay;
  Hyprland keeps virtio-GPU. No production auth policy changes for VM failures.
- Verification passes: 28 auth tests, strict all-targets Clippy, both packages and
  real Cage/Hyprland VM gates for power-dialog cancellation/focus, PAM failure/retry,
  UID/seat0/VT1 handoff, retired kiosk processes, fractional/hotplug locker input,
  held Backspace, crash protection, logout and fresh greeter recovery. Cage also
  verifies watcher and explicit S3, actual visible locked resume, reacquired delay
  leases, watcher-stop child preservation and authenticated unlock after resume.
- Final checks/images: unit `c5mjnk3sw94894i19c7y7pb427f0clpn`, Clippy
  `9q3b6y5wmx7kj19s13ya5kgv4mfq1nrc`, Cage VM `g51ql6gbqyh0v502kkbr8xh2jy1bfhgx`,
  Hyprland VM `saqyw2xb1x6ggcmxbfcikwlwq9v67ggv` under `/nix/store/`.
- Both architecture package/check derivations evaluate. The actual pinned Home
  Manager module evaluates appearance generation and the process-preserving watcher
  unit; activation stays outside the test. Native VM execution remains x86-64.
- Preserve concurrent staged settings-sidebar work outside the authentication commit.

## 2026-10-12: Authentication stage 4 — packages and real kiosk handoff

- Package locker/greeter separately from the host/DLL artifact graph, with pinned
  runtime/font/PAM paths also supplied during sandbox tests. Export root NixOS
  authentication policy and a user-only Home Manager locker module.
- Own compositor process groups and nonce-protected runtime directories. Cage runs
  only the greeter; Hyprland uses a minimal private config and PID/signature-bound
  exit. Accepted frontend status plus successful compositor exit gates handoff.
- Patch pinned Cage's xdg-shell global to version 6 and defer initial fullscreen
  requests until the surface is initialized. Native VM startup located the upstream
  pre-commit configure assertion; replaying requested state fixes fullscreen startup.
- Enter-to-retry reconnects to a freshly root-peer-validated greetd connection after
  a rejected cancellation of an already-ended PAM attempt. No failed auth or generic
  window exit can start the selected user session.
- Both real KVM/NixOS fixtures pass private wrong-password/retry, authenticated UID
  1000 and seat0/VT1 handoff, old greeter/compositor retirement, logout and SIGKILL
  recovery with a different PID. Use raw initial OCR and actual PAM/process/seat
  gates; default variant OCR preprocessing was too slow and small prompt OCR unreliable.
- Packaged Cargo tests and strict all-auth-crate Clippy pass. Verified VM outputs:
  `/nix/store/mrpnz3knnnadig13rrp7vbnhn0z4a6p4-vm-test-run-oxisession-cage-handoff`
  and `/nix/store/z1a9h9jfaqrcq6cyndxgl9388arm7096-vm-test-run-oxisession-hyprland-handoff`.
- Keep concurrent settings-sidebar changes out of this authentication stage commit.

## 2026-10-12: Settings sidebar pointer clipping

- Sidebar actions already carry stable page IDs. The shared full-layout reveal
  passed an available cursor to clipped submenu buttons; Iced buttons hit-test their
  full layout bounds, so invisible entries intercepted later roots such as Dock.
- Mask the child cursor outside the reveal/parent viewport intersection for update,
  hover and draw. Continue forwarding releases with an unavailable cursor so a press
  canceled outside the reveal cannot activate later. Keep full-size child layout.
- Three actual-widget regressions reproduced the wrong submenu action, clipped hits
  and release bug before the fix. The owned compositor smoke now clicks every root
  and Dock children under normal, reduced and zero-duration motion.
- Verification passes in Nix: 16 oxiced tests, 76 host unit tests and six settings-flow
  tests; host build, workspace formatting and strict host/oxiced Clippy. The complete
  isolated UI smoke passes, including all sidebar pointer selections.
- Local Cargo smoke binaries need the outer development shell's dynamic library
  paths. Preserve daemon logs on timeout even if status is unavailable, and refresh
  the fixture's retained monitor draft before editing after unrelated config writes.

## 2026-10-12: Authentication stage 3 — fullscreen greetd greeter

- Added the standalone `oxisession` xdg-shell Iced frontend, trusted system Wayland
  session catalog and root-peer-validated greetd client. No locker/PAM worker or
  shell plugin is loaded for greeter authentication; greetd owns user privileges.
- The client uses native-endian length framing, bounded full/partial I/O and one
  pending operation. Password responses serialize from borrowed zeroizing storage
  directly into preallocated zeroized frames, with no secret request Debug/Clone.
- Conversation state handles visible/secret/info/error/MFA prompts. Cancellation
  invalidates the attempt and drains its reply before cancelling/retrying. Rejected
  starts retain the authenticated session choice; only an acknowledged start exits.
- Session entries/executables resolve through administrator-owned system paths;
  direct argv expansion supplies explicit XDG session metadata. No arbitrary launch
  command textbox or child user compositor runs inside the active kiosk.
- Username focus, raw masked prompt input and redacted keyboard messages provide
  keyboard interaction. Normal window-close events cannot report successful login.
- Private fragmented-socket fixtures cover MFA, cancellation, changed identity,
  authentication failure, malformed frames and rejected/accepted starts. Actual
  kiosk teardown and seat/UID handoff are the following integration stage.
- Session tests, strict all-auth-crate Clippy and the standalone greeter build pass
  in the pinned Nix shell. Catalog checks include lexical symlink ancestors so an
  untrusted link cannot borrow ownership from an administrator-owned target.

## 2026-10-12: Authentication stage 2 — PAM worker and masked lock interaction

- Add fixed-service current-UID PAM authentication and account checks in a private
  same-image subprocess. Validate dedicated root-owned service configuration before
  locking, avoiding PAM's missing-service fallback. Reject PAM identity remapping.
- Transfer a private same-UID socket on FD 3, close it across helper exec, bound and
  zeroize binary frames, and cancel/reap the worker process group. Disable core dumps
  and dumpability; preserve only locale/development library lookup in worker environment.
- Preallocate response storage to avoid password-bearing reallocations. Shared
  themed views render masks, while raw key events edit the zeroizing buffer; no
  password-bearing Iced message or editable widget stores full credentials.
- Start one authentication attempt only after protocol Locked. Enter submits,
  Backspace/Ctrl+U edit, Escape/clear cancel without unlock; only a current worker's
  successful authentication and account result requests authenticated unlock.
- Production exposes `oxilock [--ready-fd FD]`, `--check`, and a private worker entry;
  no fake-authentication or service override. Real-library tests use an owned module
  and test-only confdir to cover MFA, failure, account policy, remapping and cancellation.
- Pinned-Nix auth-crate tests, strict all-targets Clippy and the standalone locker
  build pass. Protocol fixtures continue to verify opaque frames and final unlock
  sync; real kiosk/seat and visual checks follow in the integration stages.

## 2026-10-12: Authentication stage 1 — owned lock protocol and CPU frames

- Implement `oxi-lock` with direct staging Wayland lock roles, configure coalescing,
  explicit lifecycle/readiness and final display sync. No generic close/unlock path.
- Give each output two release-aware XRGB8888 SHM buffers and an Iced tiny-skia
  renderer/cache. Keep integer/fractional geometry exact; retire hot-unplug input.
- Translate native seat keyboard/pointer events with device identity guards and
  a null clipboard. Input repeats/render wake policy follow in the polish stage.
- Gate the executable to `--check` until the PAM frontend stage; production does
  not expose a fake-authentication or button-to-unlock mode.
- Private Wayland tests render real opaque frames and verify latest ack, scaling,
  hotplug/removal, denied/external finish, authenticated sync, malformed configure,
  missing protocol and compositor-owned lock after a simulated client crash.

## 2026-10-12: Authentication stage 0 — boundaries and backend audit

- Audited Iced session-lock 0.17.1; rejected its no-op lock lifecycle and generic
  Close-to-unlock behavior. Use an owned Wayland adapter with Iced CPU rendering.
- Added reusable redacted, bounded response and attempt state plus explicit lock
  readiness/destructor and greetd start-acknowledgment state machines.
- Keep PAM in a cancellable private worker and greetd as the privileged login
  manager. No authentication secrets enter shell plugins or general shell IPC.

## 2026-10-12 — Native lock/session implementation authorization

- User requested committing the completed shell/UI work, then implementing all
  phases of `LOCK_SCREEN_SESSION_MANAGER_PLAN.md`, with a verified commit after
  each phase. Preserve separate locker, authentication and greeter ownership.
- Exclude generated Python bytecode from source commits and ignore future caches.

## 2026-10-12 — Nested settings, monitor snapping and native auth plan

- User requested consistent or removed control-panel session content, animated
  expandable settings submenus, ReSet-style monitor snapping, header reset/compact
  footer, and a Rust/Iced Wayland lock-screen plus kiosk session-manager plan.
  Earlier work is user-staged; keep it intact. No commit or delegation requested.
- Choose the allowed removal option for embedded System session actions. Factor
  `controls(model, include_session)` so the ordinary System popup retains session
  actions while control-center content shows brightness and power profiles. Keep
  the dedicated host OxiShut tiles, provider model, confirmations and ABI unchanged.
- Add pure `src/settings/navigation.rs`: stable route IDs, exhaustive unique field
  assignment, curated nested pages and dynamically grouped plugin namespaces.
  Retain Category/root shortcut order. Select branches/children via sidebar, landing
  cards, breadcrumbs or narrow full-path picker; opening a root folds other roots
  while remembering nested expansion. Keep drafts and route/expansion on saves,
  and reconcile a removed provider page to its nearest valid ancestor.
- Sidebar width is 288; branches use reversible EASE_OUT with the current theme
  duration and full-child `oxi_reveal` bounds. Reduced/zero motion bypasses the
  animation builder. Global search spans the schema and structured-editor results
  route to dedicated pages rather than offering raw duplicate fields.
- Put Reset all overrides immediately before Reload/Close. Replace the previous
  multi-line footer with one fixed-height revision/status row and scroll long
  diagnostics. Move config directory/editor to Advanced/GUI overrides. Busy field
  trees, provenance and sparse revision-checked persistence remain intact.
- Add pure logical snapping in `src/monitor/snapping.rs`, informed by local ReSet
  `ReSet-Plugins/monitors/src/frontend/handlers.rs` and its geometry fixtures.
  Use 150 logical units, enabled transformed/scaled bounds, deterministic nearest
  valid axis candidates and overlap rollback to the captured gesture origin.
  Canvas previews show alignment guides/overlap feedback; exact numeric edits and
  native Preview/Keep/Revert/controller rollback policy are unchanged.
- Capture settings page/control and monitor diagram bounds for native diagnostics.
  During animated layout, cached sidebar bounds can differ from the drawn frame;
  use actual child-page card bounds for navigation and intermediate/final screenshots
  to verify motion. Keep temporary devices/compositors and config fixtures owned.
- Write `docs/LOCK_SCREEN_SESSION_MANAGER_PLAN.md`; no locker/greeter code or new
  dependency added. Working binaries `oxilock`/`oxisession` share Rust/Iced/oxiced
  visuals, with separate protocol/PAM and greetd state. Locker requires
  ext-session-lock-v1 readiness/hotplug/configure/ack/sync and authentication-gated
  unlock; candidate iced_sessionlock 0.17.1 needs a version/API audit. Greeter uses
  ordinary fullscreen xdg-shell for Cage and a dedicated Hyprland kiosk wrapper;
  greetd owns privileged PAM/session identity and starts the user only after kiosk
  teardown. Gate real seat/VT handoff in an isolated NixOS VM.
- Full workspace tests/doctests, strict all-targets Clippy and same-graph host/all
  thirteen DLL builds pass through the pinned filtered Nix shell. Use one link job,
  stripped root artifacts and only session-target Cargo cleanup under limited disk.
  The complete isolated two-output Hyprland/Sway GUI flow passes hierarchy, responsive
  navigation, header reset/footer stability, real edge snapping/overlap restore,
  native preview/save/rollback and all retained menu/provider/music/reveal checks.
  Intermediate/final sidebar images show actual motion; evidence stays under
  `/tmp/opencode/oxibar-nested-settings`.

## 2026-10-12 — Shared toast placement, stable menus and edge reveal

- User requested aligned/configurable music and notification toasts, stable control
  titles, enough audio height, vertically centered calendar days, flat bar-icon hover,
  quiet empty launcher searches, Escape for every dropdown, stable settings saves,
  pointer-revealed bottom bars and compositor commands for launcher/session. Preserve
  all earlier staged/unstaged work; no commit or delegation requested.
- Add live `[toasts]` position (six edge/center choices), margin 16 and spacing 12.
  Notifications stack newest-first per output; Audio `media`/`media-volume` feedback
  occupies the next slot, aligning the same edge/center. Reuse the existing per-output
  OSD ID, size, abortable timer and generation policy; device/brightness topics retain
  ordinary OSD placement. Notification resize/close and live config restack music.
- Default controls are 960×960 with the same `font_xxl` page title in both modes.
  Audio popup metadata and host headroom default are 460×640; clamp visible/input
  metrics to the source output and retain scrolling on smaller outputs. Calendar
  day text fills its cell before centering. Shared bar hover stays transparent;
  navigation keeps themed hover. Remove host name tooltips and launcher no-matches
  copy while retaining informational tooltips and explicit indexing/provider errors.
- Every ordinary/attached dropdown and optional inbox panel receives exclusive
  keyboard focus. The central scoped Close binding handles captured Escape and
  restores normal bar policy without removing its ID. Save/reload feedback reserves
  fixed footer space; layout drag rows keep the same widget tree while busy.
- Add live common/profile `bar.auto_hide` (false) and `bar.reveal_zone` (8, 1..128).
  Keep the native bar/headroom and all models resident; hidden input is only the edge
  strip, exclusive zone is zero, pointer departure hides content and an owned dropdown
  pins visibility. Leave events are distinct from Enter; per-output visibility appears
  in status. Regression/native tests prove unknown IDs do not reveal bars and geometry
  changes do not replace models or native IDs.
- Add resident `oxibar session [open|close|toggle]` and version-1 session requests.
  Bare commands toggle; repeated open preserves its surface. Launcher already supports
  the same verbs. Document Hyprland/Sway binds and retain System capability/confirmation
  behavior instead of executing session actions from menu commands.
- Native smoke found a pending-repeat unwrap after temporary keyboard removal in the
  vendored backend. Clear queued repeats on release/Enter/Leave/removal and install a
  timer only with a live keyboard and matching repeat key. This is independent of the
  earlier anchor-cache fix; an older lost-location panic cannot be identified exactly.
- Workspace tests/doctests and strict all-targets Clippy pass. Build the host and all
  thirteen DLLs together through the pinned Nix shell, using only session-owned target
  cleanup, one link job and stripped root artifacts due limited disk space. The complete
  isolated Hyprland 0.51.1/Sway GUI flow passes real null-input/output audio, MPRIS/music
  stack placement, live settings/provider changes, Escape/session commands, monitor
  preview/rollback and pointer reveal/pinning. Before/after settings screenshots show
  unchanged form geometry. Evidence: `/tmp/opencode/oxibar-toast-reveal`.

## 2026-10-11 — Actual dock catalog, OxiShut session tiles and optional bar names

- Follow-up request: applications still showed the generic icon, Session should look
  like OxiShut, and plugin names should be hidden or toggleable. Preserve all earlier
  user/agent staged and unstaged work, including user edits to the installer. No
  commit, real desktop activation or delegation was requested.
- Concrete local icon cause: effective config enabled `libapplications.so`, but the
  XDG plugin directory did not contain it. `scripts/install-plugins.sh` omitted
  Applications and repeated Tray. Include all twelve production providers, preflight
  every file before installation, and respect `CARGO_TARGET_DIR`/`XDG_CONFIG_HOME`.
  Private fixtures prove missing Applications causes no partial install, correct
  content/mode for all twelve and absolute/relative build roots.
- Library search now prefers user XDG files, then colon-separated
  `OXIBAR_PLUGIN_DIR` directories. The Nix host wrapper defaults that fallback to its
  pinned Applications package, without implicitly enabling providers. Catalog and
  live loading share this resolution. Dock matching permits lower-ranked executable
  fallback despite an alternate StartupWMClass, preserves Steam game identity, and
  prefers an icon-bearing catalog entry when identity scores tie. Reuse the one
  resident application catalog; do not introduce per-output scans.
- Read local `/home/dashie/gits/OxiShut` for visual reference. Session is now a centered
  host `SurfaceKind::Session`, up to 800×480 and 90% of the output, with three columns
  of large portable action icons, neutral fills, accent selection, capability-disabled
  tiles, hover descriptions and selected-action text. Preserve configured action
  order/default six actions; `power-off`, `reboot`, `suspend` makes a three-tile preset.
  Reuse System's cached menu/command/native confirmations, not OxiShut's shell commands
  or another logind owner. Capture source output/owner before retiring the menu;
  `OnCommandOutput` retains native confirmation placement. Stale IDs, menu disable,
  competing overlays and output retirement must not remove the bar.
- `bar.show_labels` defaults false, applies live and supports per-output profiles.
  Host action tooltips remain discoverable; clock time/workspace numbers remain
  functional content. Add optional ABI-6 `BarViewFn(model, PluginViewContext, bool)`;
  prefer `bar_view`, then `view_with_context`, then required `view`. Existing context
  layout and required signatures stay unchanged. Built-in status providers and the
  example implement the hook; older opaque third-party views keep legacy behavior.
  Output-local rendering must not mutate shared provider state.
- Equal-width icons exposed bounds reuse after a layout drag: a recycled widget tree
  retained another item's identical rectangle after host anchors were pruned. Add
  optional `DragHandle::with_identity`; changed identity clears cached bounds while
  stable identity preserves them. Bar slot/provider and overview window identities
  now republish the correct callback geometry. Regression and post-drag GUI checks
  prove this independently of any backend panic.
- Verification passed: full workspace tests/doctests, strict workspace all-targets
  Clippy and formatting; all fourteen pinned Nix packages; installer fixture checks;
  thirteen current oxiced tests, including equal-geometry identity and drag-preview
  transitions. Rebuilt the host after the identity fix, with unchanged provider
  exports/ABI. Current host:
  `/nix/store/xkrcsxkbzm2vgp83p5sv14ly37s526qn-oxibar-0.1.0/bin/oxibar`.
- Real private Hyprland overview proof deliberately omits Applications from XDG and
  loads it via fallback: actual green SVG dock pixels, real window previews, centered
  modal/navigation/remap, retirement activation, hotplug and ten close cycles pass.
  Two final complete UI runs pass icon-only/live host+Stats labels, 800×480 session
  tiles/native confirmation, live Applications indexing/owned fixture launch,
  calculator copy, drag preview/move, post-drag anchors and native display rollback.
  Screenshots stay under `/tmp/opencode/oxibar-refinements`; no user HOME installation,
  configuration change, real application launch or session action was performed.
- An earlier isolated run caught an unlocated `Option::unwrap()` panic; its short log
  buffer lost the location. No root cause established; the two final runs did not
  reproduce it. Smoke now enables backtraces, keeps 500 log lines and fails on caught
  panics rather than accepting a restarted daemon. Track this diagnostic limitation
  in technical debt; do not claim the anchor fix proves its cause.

## 2026-10-11 — Configurable shortcuts, live providers and readable presentation

- User requested a Shortcuts settings page; a large modal overview with Tab cycling
  windows and h/j/k/l moving between monitors; dock spacing/real icons/focus hue;
  accent workspace text; faster first control-center opening; three-column layout
  drag with visible dragged content; a text-only clock/full-height calendar; contrast
  review and Base24 compatibility research; actionable session enablement and working
  Applications launcher providers. Preserve the extensive pre-existing staged and
  unstaged implementation, including the unrelated staged `result` deletion. No
  commit, activation or delegation was requested.
- `src/shortcuts.rs` is the single host key dispatcher and schema. Live text settings
  accept comma-separated alternatives, Ctrl/Alt/Shift/Super modifiers and empty
  disablement. Reject malformed or overlapping-context conflicts before adopting
  configuration. Resolve the exact receiving surface before dispatch; respect
  captured form input and use current bindings for visible hints. Compositor-level
  global toggles remain compositor configuration. Shortcuts is category 11, default
  Alt+s, with existing category shortcuts preserved.
- Overview remains one visible-owned native capture client with per-output surfaces
  and one keyboard owner. Surfaces are centered, default maximum 1400×900 and bounded
  to 90% of logical output size. Tab/Shift+Tab wraps windows within the active monitor;
  h/j/k/l selects spatial neighbors using current output geometry. Pointer redirect
  uses actual centered-modal origins. Exact native identity, stale-generation guards
  and retirement of ALL overview surfaces before activation remain unchanged.
- Provider enablement is live, with one serialized background loader and queued
  reconciliation. `src/plugins/runtime.rs` retains disabled models and DLL code,
  removes their consumers/subscriptions, accepts pending model data and rejects their
  host actions. Provider identity survives in `PluginEmission` until active-state
  checking and translation. Re-enable reuses one model/stable IDs and delivers current
  config/theme/compositor hooks. Completion uses current allowed membership so stale
  initialization cannot resurrect a disabled provider. DLL code replacement still
  requires restart; retained service workers may live until process exit under ABI 6.
- Settings always exposes Applications, Tools and System with enabled/disabled/loading/
  missing states. Missing packages have installation diagnostics; Session has an
  actionable Plugin settings fallback. Individual launcher provider toggles and the
  default-provider group take effect without restarting. Applications indexes normal
  XDG roots first and appends Nix profile roots even with nonempty/incomplete XDG paths;
  empty indexes identify their data directories. The owned GUI fixture proves real
  live indexing, search and activation rather than only saved configuration.
- Dock uses catalog SVG/raster icons, token spacing and accent focus fill without
  dots; inactive workspace labels use readable accent text. Clock bar rendering is
  time text only. Calendar gets bounded full-height space and evenly stretching week
  rows, with adjacent-month dates using the shared muted role. Existing off-executor
  calendar workers and stable child-layout reveal are retained and measured.
- Layout settings uses equal Left/Center/Right columns of compact cards. Existing
  revision-bound atomic drag payloads are unchanged. Shared draw-only pointer preview
  never intercepts drop input. Real GUI testing found two bugs absent from compile-
  only checks: empty surface trees needed a unique preview tag/defensive child diff,
  and `center_y(Fill)` overrode explicit drag-handle height inside unbounded scrolling.
  A widget-tree transition regression and actual three-column drag now pass.
- Keep Base16/Base24 as ecosystem adapters, not the widget color API. Stylix exposes
  Base16; absent Base24 extensions derive from supplied Base16 roles rather than
  unrelated seeded brights. Semantic neutral hover stays neutral. Automatic normal,
  muted and filled-control text meets 4.5:1 across opaque states; factory Catppuccin
  surfaces/text are adjusted accordingly. Explicit legacy/semantic overrides and raw
  input/accent colors remain authoritative. No new dependency or native ABI change.
  Mapping, sources and override/transparency boundaries are in `configuration.md`.
- Full normal workspace tests/doctests passed, plus final formatting, strict workspace
  all-target Clippy, host lib/bar-dropdown/overview/settings/theme regressions and
  all 12 shared-library unit tests. All fourteen pinned Nix outputs built; final
  host-only drag/layout corrections were rebuilt and tested with the verified DLLs.
  Current host: `/nix/store/r1laip9j77yrkl2zg8if66shcv3i22x1-oxibar-0.1.0/bin/oxibar`;
  Applications: `/nix/store/q6pj6wsyhwa8cgb81j22gng1kb7i5haj-applications-0.1.0/lib/libapplications.so`;
  Clock: `/nix/store/gpzmkpnjnzanym72vkm4qipzkdkxk8ni-clock-0.1.0/lib/libclock.so`.
- Complete refined UI and real-pixel overview smokes pass on owned private Hyprland
  fixtures. Overview validates 1400×900 centered bounds, Tab monitor retention,
  spatial keys/live Ctrl+h remap, SVG pixels, retirement/activation, hotplug and ten
  closes with zero capture workers and stable thread count. Private Sway presentation
  measurements during a blocked two-second CalDAV fixture are first 31.25 ms, warm
  p95 6.38 ms and concurrent status 4.77 ms, all under 50 ms. Preserve the preceding
  performance record and add `control-center-refinements-reference.json`; no causal
  percentage improvement is inferred. Screenshots stay under `/tmp/opencode/oxibar-refinements`.

## 2026-10-11 — Real window overview, dock icons and first-open responsiveness

- User approved the overview plan: "plan is good, proceed with implementation";
  also requested fixing bottom-dock application letters and investigating first
  control-center load lag. This is a new task after the completed `0fd8bfe` roadmap
  and preceding UI refinements. No new commit/push/activation/delegation was requested.
  Preserve all known externally staged work and unrelated staged deletion of `result`.
- Overview is a host capability, not another model/plugin per monitor. One visible-
  owned native thread serves per-output grids; only one surface owns keyboard focus.
  Arrow/Home/End selection scrolls into view, Tab switches monitor groups, and
  click/Enter uses the exact displayed native handle. Existing source/generation
  guards and hotplug registry ownership remain authoritative.
- Hyprland export v2 uses the exact wlr handle. Modern ext capture bridges an exact
  ext resource to an exact wlr activation/output resource through the advertised
  full-64-bit mapping protocol, once per window. Old handles never rebind after
  address reuse. No title/app-ID guess, truncated address or output crop substitutes
  for native window identity/content. Vendored protocol XML retains BSD notices and
  pins upstream `4c27ba9ce104782bf718aabf59df3f503afe7caa`.
- Capture owns proxies, memfds/mappings and frame callbacks. Checked SHM formats,
  stride/orientation/Y inversion/premultiplied alpha, any-order constraint batches,
  exact current frame object IDs and monotonic local identities prevent stale pixels.
  Limits are 256 windows, two global/one per-window jobs, 64 MiB staging and 96 MiB
  thumbnail image data, with adaptive downsample and configurable maximum 8 Hz.
  Modern frames can wait for damage; legacy export uses bounded fresh captures.
  Pixels never persist; denial/stopped/closed sources clear caches.
- Fixed a coalesced-wake stream bug: an extra wake with an empty latest mailbox must
  keep waiting, not end the subscription during rapid window moves. Offline commands
  reject, native activation callbacks identify their exact display sync, and dropping
  the stream cancels/wakes/joins its owner with no closed capture worker.
- Native activation must wait for ALL overview Closed events, not only queued
  RemoveSurface messages. A generation-scoped abort-on-drop retirement timer handles
  failure without delayed stale wakeups. Exclusive cross-monitor pointer redirection
  converts actual logical coordinates and tests visible card bounds; scrolled-out
  zero bounds erase hit regions. No speculative native pointer backend rewrite.
- Dock bug: SVG paths used the raster renderer and missing icons became letters.
  Shared SVG/raster/application fallback, automatic GTK4/GTK3/KDE themes, Nix data
  paths when XDG paths are unset, filename aliases and ranked class/desktop matching
  fix it. Steam URI-derived classes beat shared generic executable matches. Final
  review also prevents an explicit game class from falling back to the shared
  launcher's executable and claiming Steam's own window in a pinned group.
- Opening lag had blocking calendar refresh/launch calls in async executor work and
  animation-sized child layout. Bounded off-executor clock jobs/curl waits plus shared
  stable full-layout reveal avoid both. Selection animations retain the theme and
  bypass reduced/zero duration; source images do not cross-fade another identity.
- Full source verification passes 268 normal tests/doctests, 14 opt-in tests ignored
  by default and strict workspace Clippy. Normal hermetic modern Wayland tests execute
  real SHM pixels, formats-before-size, resize/denial and recycled-address activation.
  Actual owned Hyprland 0.51.1 tests prove fresh changed occluded/other-workspace
  content, identical-title per-monitor previews, exact keyboard/cross-monitor mouse
  activation, hotplug and ten closes with zero capture workers/cache and stable
  closed thread counts (64 in the final run). An attempted actual 0.56.2 nested
  startup failed before connection; its cause was not proved and it is not modern
  desktop runtime proof.
- All fourteen pinned Nix outputs build. Clean-source filtering retains required
  protocol XML; host preCheck supplies already-declared Wayland library paths for
  dynamic native client loading. The modern test now diagnoses environment errors
  immediately instead of masking them behind reconnect retries. Final host:
  `/nix/store/l6qpfnvzmv69cxi1y5qpmnpk8if7fzaz-oxibar-0.1.0/bin/oxibar`;
  Applications: `/nix/store/9b49s0dci9x0waj161fd6l6iazsjhwa7-applications-0.1.0/lib/libapplications.so`;
  Clock: `/nix/store/mwynwpg4g7gl35hd05vl5mlvav0g8731-clock-0.1.0/lib/libclock.so`.
- The final explicit-class/executable dock guard passed host 68 unit tests, eight
  dock/desktop flows, strict workspace Clippy, a fresh host Nix build and the complete
  real-pixel overview smoke again. That run has zero closed capture workers, 64
  threads across every cycle and about +1.5 MiB first/last-five median RSS change,
  within the 2 MiB noise budget. Performance paths/workloads are unchanged by this
  matching-only review fix, so presentation references are retained without repetition.
- Final repeatable `overview-smoke.py`, `ui-smoke.py` and retained `smoke.py` all pass
  on owned private compositors/buses/XDG fixtures. Actual green SVG/fallback, changed
  native preview pixels, exact activation, menus/Base24/native display save/revert,
  OSD input, CalDAV/password/Tray/notification reply-DND and widget/hotplug flows remain
  correct. All owned child processes/files/compositors are reaped; actual desktop,
  hardware/session/input/clipboard/calendar/network are untouched. Images stay private.
- Named Sway 1.11/CLOCK_MONOTONIC references separate UI performance from unsupported
  Sway content capture. Two native windows/30 opens: first actual overview presentation
  31.725396 ms, warm p95 37.245715 ms, closed CPU 0.399998814% of one core, zero closed
  draws/capture workers, +138 KiB median reopen RSS/no thread growth. All seven gates
  pass. First control-center actual presentation 31.359486 ms, warm p95 4.939352 ms,
  concurrent status 3.849148 ms while a private two-second CalDAV sync remains blocked;
  all three gates pass. No configure-time substitute or old-binary percentage claim.
  Machine-readable reports are `overview-performance-reference.json` and
  `control-center-performance-reference.json`; voluntary switches are a wakeup proxy.
- Required ABI 6 remains unchanged. `overview` CLI/config/reserved bar entry needs
  no provider enablement; supported capture/activation capabilities are explicit.
  `WINDOW_OVERVIEW_PLAN.md` now records implemented status and platform extensions.
  Development used filtered pinned Nix, serial builds and only the agent-owned
  `/tmp/opencode/oxibar-target` (debug=0/incremental=0); no global GC or user-target cleanup.

## 2026-10-11 — Bar-integrated menus, Base24, native displays and button anchors

- Scope is the user's new UI/settings/theme/monitor request after the completed
  `0fd8bfe` roadmap, plus the later request to open dropdowns at their own buttons.
  This task does not include another requested commit or implementing the overview;
  its next-feature plan is `WINDOW_OVERVIEW_PLAN.md`. Preserve the unrelated staged
  deletion of `result` and known externally staged task changes.
- Shared bar controls now center their children vertically, use themed SVG action
  icons and space entries. Settings defaults to 1440×960, reserves scrollbar gutters
  and uses flat navigation/neutral `primary_bg` cards consistent with launcher/bar.
  The center uses left navigation with independently scrolling selected content.
- Control center defaults to 960×760 attached presentation and clock entry selection;
  configurable provider ordering/inclusion remains shared-model. Notifications opens
  its inbox inside the same bar by default, with replies/DND/records retained and
  visible-inbox toast suppression. Optional standalone center/panel modes still work.
  Attached close paths never remove the native bar or leave its keyboard grab active.
- Session dropdown adapts OxiShut's keyboard/action UX, but delegates cached actions
  to the existing native System owner. Lock uses the configured locker; destructive
  actions still open normal confirmation and validate native policy/session identity.
  The optional ABI6 `session_menu` capability changes no required signatures.
- Bar-layout drag/drop captures actual press/release and source slot/revision,
  atomically saves all three arrays and rejects stale gestures. Keyboard reorder
  alternatives remain. Default launcher providers are a separate sparse filename
  list, unioned/deduplicated with ordinary plugins; disabling one never promotes
  other defaults into the ordinary list. Provider lifecycle stays restart-only.
- Base24 carries all slots and optional final accent through explicit immutable
  snapshots. Legacy seed-role overrides are retained unless a new host scheme
  supersedes them. Standalone flat/nested schemes and explicit role/accent precedence
  are validated; bright states use the additional slots. The inspected dashnix donor
  currently has Base16 plus a `base0D` accent override, so its accent convention was
  inspiration, not a claim that the donor already implements Base24. Donors untouched.
- ReSet inspired normalized native display boundaries. New bounded Hyprland
  legacy/Lua IPC and wlr-output-management owners preserve precise modes/zero
  compositor-defined refresh, advertised proxies/serials, test/apply and rollback.
  No donor unsafe thread markers, global head state, fabricated serials or rounded
  refresh identity were copied. Numeric fields and drag diagram share logical drafts.
- Native previews hold a controller-owned 20-second lease. Keep saves sparse profiles
  with the starting store revision before releasing rollback; failure, expiry,
  Revert and orderly drop restore connected baseline/preserve new outputs. The lease
  is not an external crash/SIGKILL watchdog. Physical links/driver policy remain manual.
- Actual child layout rectangles and pressed section/index/part keys anchor popup
  display, input and outside dismissal together. Output edges clamp the connector;
  moved/removed/duplicate slots are validated/pruned before cache reuse. This fixes
  leftmost placement without a speculative native pointer rewrite.
- Verification passed 256 normal tests/doctests, strict workspace Clippy, all 14 Nix
  outputs and actual native preview/revert/drop tests on owned Hyprland 0.51.1 and
  Sway 1.11. Final root is
  `/nix/store/6bbh8asvf9hpx1qs8gv9ix5yw97haqc5-oxibar-0.1.0/bin/oxibar`;
  final Notifications is
  `/nix/store/hdcydbyk69iir8788x00bwd36w24x25d-notifications-0.1.0/lib/libnotifications.so`.
- Final icon review also gave Stats a shared SVG chip and replaced System's
  font-dependent sun glyph with the shared SVG brightness icon. Focused provider
  tests, strict workspace Clippy, all 14 package builds and the full new UI smoke
  passed again with the final shared-component graph.
- New repeatable `scripts/ui-smoke.py` passed real clock/left-menu/inbox/session
  confirmation, button anchors, 1440×960 settings, GUI default-provider toggle and
  restart/calculator copy, pointer layout drag, all eight loaded theme hooks,
  native 1.25-scale Preview/Keep, 1.5 Preview/Revert, final scale restoration and
  bottom-bar geometry. Existing `scripts/smoke.py` also passed its full retained
  legacy flows with explicit optional window/panel preferences. Private bus/services,
  compositor, XDG state, immutable base and virtual pointer fixtures were cleaned;
  actual user desktop/input/hardware/session/clipboard were untouched. Images private.
- Development used filtered pinned Nix, serial package builds and exclusively the
  session target `/tmp/opencode/oxibar-target` with debug=0/incremental=0. No global
  GC, user-target cleanup, donor changes, activation, push or delegated agents.
- Overview plan requires exact per-window source handles and real pixels, not icons
  or output crops; includes capability-gated ext/Hyprland capture, per-monitor grids,
  keyboard/mouse activation, bounded visible-only caches and identity/hotplug gates.

## 2026-10-11 — SDK, Independent App Lifetime and Verified Release Gates

- Completed the final milestone after `5bfa536` (live themes/providers). User's
  instruction remains: "after this step is done, proceed with all remaining steps,
  committing after each step". Phases 0–7 are implemented and verified; this final
  phase completes the authorized commit sequence. Preserve unrelated staged deletion of
  `result`; no push, activation or delegation was requested.
- Added `PLUGIN_SDK.md`, a separately packaged Example provider, actual installed
  catalog/`oxibar plugins` with ABI/capabilities/enabled state, and real factory/
  availability/duplicate-name tests. The example shares one model across bar,
  popup, CC, desktop and OSD, live label/validator, owned visible timer and scoped
  unsubmitted reset cleanup. `example.available`/initialization probe are example-
  only contract seams, not new required ABI exports. ABI 6 remains unchanged.
- Successfully initialized native libraries stay process-resident. Opaque messages,
  futures/widgets/models can retain vtables during deferred executor teardown;
  pinning code avoids unload-before-destructor. Owned models/streams still cancel
  and drop normally. The real Example contract passed 25 cycles without host Tokio,
  and metadata/availability inspection initializes zero models; duplicate names one.
- SDK owns periodic interval streams; Network/Battery no longer spawn forever
  polling threads. Network action/edit generations prevent old completion from
  clearing a newer password form; modal close clears only unsubmitted state.
  Network/Tray/Clock optional command hooks expose cached UI/confirmed actions
  through ordinary command outcomes; Example demonstrates popup/serialized OSD.
- Shared SDK app handoff uses native systemd user-manager transient services in
  `app.slice`, outside shell termination scope. It resolves validated argv, strips
  caller service/FD variables (including `WAYLAND_SOCKET`), listens before submit
  and awaits actual job result. Definite unsupported/unavailable manager permits
  direct reaped process-group fallback; uncertain acceptance cannot duplicate an
  app. Private manager tests prove caller exit/app survival and accepted-error
  single execution. Direct fallback cannot escape an existing cgroup without a
  manager. Apps, Tools, explicit calendar command and dedicated locker share it.
- HM startup is opt-in false with session target/Wayland condition/control-group
  service ownership. `extraPlugins` installs disabled catalog packages without
  enabling them in generated base or managing overrides. Examples provide session
  environment/keybindings. Repeatable `nix/hm-test.nix` passed all eight false/true
  startup/catalog/ownership assertions, using the external HM checkout with pinned
  nixpkgs; its HM 26.05 vs nixpkgs 25.11 warning was version mismatch only. No real
  service was activated.
- Added repository `nix/gui-tests.nix`, private-bus config, isolated compositor
  coordinator, fixture server/shims and output-mapped virtual pointer. Test-only
  Sway xdg-shell advertisement permits a private pinned Hyprland 0.51.1 child;
  Mesa/vendor/GBM/DRI paths are pinned. It owns runtime/cache/bus/compositors and
  never touches actual desktop input/clipboard/network/calendar/hardware.
- Full repeatable smoke passed actual catalog, SDK confirmation Escape/replacement,
  theme density 1.2 application while OSD remains visible, launcher app handoff,
  CalDAV/day fixture, secure network save, native tray menu, notification toast/
  panel/inline reply/DND, two-instance pointer drag/resize, stale revision, hotplug
  and ten CC reopen cycles. Geometry corrections were fixture hit targets/padding;
  no speculative pointer backend changes were made. Screenshots stay private.
- Optional backend `OXIBAR_PERF_TRACE` binds `wp_presentation` only in measurement
  mode, requests feedback before next Iced draw/buffer commit and records actual
  presented clock stamps/native retirement. No normal-runtime protocol/writes.
  Nested Hyprland 0.51.1 omitted clock advertisement/returned stale timestamps, so
  it was rejected as a benchmark reference; owned Sway 1.11 advertises clock 1.
- Profiling found self-reindex feedback from unconditional watch replacement and
  sibling trace writes under a missing-root parent. SDK watches now diff path/mode/
  device/inode and filter to configured root relationships, with real quiet/create/
  update/recovery regressions. Traces live outside watched fixtures.
- Named release reference: Ryzen 9 7950X, GPU PCI `1002:744c`/`1002:164e`, kernel
  `7.2.8-xanmod1`, pinned Sway 1.11 headless GLES2, release Apps+Tools/no dock/local
  2,000-entry index. 50 warm opens/20 queries/10 seconds idle passed all six gates:
  cold control 122.685 ms/bar actual presentation 122.041 ms; warm median/p95/max
  20.555/20.827/21.120 ms; query max 0.826 ms; idle 0.30% one core/zero draws;
  all-thread voluntary context switches 202.60/s (wakeup proxy); reopen RSS median
  +130 KiB within 2 MiB noise budget, threads 66 → 64/max 66. Record/commands are
  committed in `RELEASE_CHECKS.md`/`performance-reference.json`.
- Final source review corrected relative executable/PATH resolution to use the
  requested working directory before native handoff. A private owned-file regression
  covers `./private-program`; argument vectors and direct child cwd remain unchanged.
- Final source verification passed 243 normal workspace tests/doctests, eleven
  explicit opt-ins ignored, strict all-targets Clippy and formatting. Actual native
  loader/Example FFI, private manager and full final compositor smoke passed. All
  fourteen final Nix outputs built; host is
  `/nix/store/vbl1q4zvr3mpywm2b491gwynbpblbxrv-oxibar-0.1.0`, Example
  `/nix/store/y15yanwva3qxda65rg5341x92kbpv2w4-example-0.1.0`. These final packages
  include the working-directory regression fix; presentation/query/idle paths did
  not change after their measured release proof.
- Keep explicit session-owned `/tmp/opencode/oxibar-target` with debug info and
  incremental disabled for agent checks, filtered source for untracked manifests
  and serial Nix builds. Two earlier test commands accidentally used the existing
  workspace target; it was not deleted. Review both external staging and new files
  before final commit; include only intended work and leave `result` deletion.
- All requested implementation milestones are complete. Remaining generic focused-
  output/window adapters, native hardware extensions, accessibility, optional
  weather/autohide and safe future hot unloading are documented follow-ups, not
  claims of implemented universal support. No deployment/activation is part of this
  source delivery.

## 2026-10-11 — Live Immutable Themes and Expanded Launcher Providers

- Completed milestone 6 after `2ed9892` (dock/widgets). User authorization remains
  all remaining steps with a commit after each; continue into the SDK/packaging/
  presentation-time release gate after this commit. Preserve staged deletion of
  `result`; no push, activation or delegation was requested.
- Host and every bundled provider accept explicit immutable theme snapshots. The
  seed palette remains exactly compatible; synchronized library-local handles and
  a 128-value immutable palette/font pool keep old field references valid without
  repeated-revision leaks. Legacy providers without the hook keep their fallback.
- Theme palette/font/preset/density/scale/motion/radius are source-aware live fields.
  Font lookup/read is bounded, bytes upload before canonical-family switching and
  late requests cannot overwrite newer palette/family. Shared ordinary text/inputs/
  pickers inherit the family, icons keep glyph fonts, reduced/zero motion bypasses
  animation builders. Legacy bar font and backend timers remain restart-only.
- Applications gained localized names/keywords/actions, inherited/cycle-safe icon
  lookup, filesystem refresh and private usage/favorite ranking. Action rows retain
  their own stable IDs and stay out of the shared dock catalog. Two latest-only
  workers replace per-query thread spawning; scopes and generations cancel stale work.
- Tools supplies every planned command/calculator/file/bookmark/named-action/clipboard
  provider. Immutable action content participates in local identity; favorites have
  separate logical keys. PATH/opener/actions are argument lists, never implicit shell
  evaluation. Non-UTF8 candidates are skipped rather than opened through lossy paths.
- One native clipboard owner prefers ext-data-control with wlr compatibility,
  wakeable polling, bounded FDs/queues/text/deadlines and owned cancel/join. Capture
  defaults off; calculator copy works independently. Opt-out clears synchronously;
  capture/selection epochs block stale reads and IDs remain monotonic across reconnect.
  Clipboard text is absent from persisted ranking state and historical identity keys.
- SDK workers keep one running/one latest pending job; shared filesystem ownership
  watches missing parents nonrecursively and recovers newly created roots. Usage
  state is versioned/private/atomic, loads before local save, merges unsaved counts
  once and retains corrupt formats. Favorite writes stay sparse config transactions.
- Root uses longest advertised prefixes, displays status/guide/stars and captures
  provider/local identity in rendered rows and input submission. Pure validators
  pin provider libraries while running before reload/candidate adoption and writes.
  Status exposes opaque result identities, not queries/titles/clipboard text.
- Source review fixed seat capability churn in the local 0.17.1 backend: unchanged
  devices are retained, removed ones released, stale keyboard events ignored and
  compositor Enter/Leave remains authoritative. Virtual keyboard fixtures still
  require binding delay before key events; the exact origin of earlier duplicate
  proxy/modifier timing was not claimed as proved.
- Verification passed 236 workspace tests/doctests, strict full-workspace Clippy,
  Nix host/Applications/Tools/Clock and private modern-preferred/forced-wlr clipboard
  integration. Real isolated provider GUI passed calculator copy, opt-in history,
  files/bookmarks/PATH/named actions, literal `$(not-a-shell)` arguments, French
  desktop actions, star overrides, filesystem refresh/stable IDs, private usage,
  invalid URL rejection, immutable base, opt-out and restart. Theme GUI separately
  passed density 1.2, palette/alpha/font/scale/zero-motion, all four upgraded providers,
  invalid edits, persisted restart and reset. Screenshots are private.
- Latest verified packages: host `fib14ar2c7i0rizqsxv8laawy7fdc31x`, Applications
  `r83ms2jabac13n8zm84kz3sahqf1dhfy`, Tools `jhm00yalvwqz855r31izfy3bwwc7zqxv`,
  Clock `8h086jmpwvsb5a2lqkdq6qhzyp7w07fx`. Temporary helpers are
  `/tmp/opencode/oxibar-theme-smoke.py` and `oxibar-providers-smoke.py`, under the
  isolated pinned compositor/bus setup. Their config/data/state/clipboard/processes
  were owned fixtures; real desktop, hardware and user services stayed untouched.
- A single earlier test command accidentally built into existing workspace target;
  it was not cleaned or deleted. Subsequent checks use only agent-owned
  `/tmp/opencode/oxibar-target`, debug=0/incremental=0, filtered source and serial Nix
  builds. The backend post-patch source has a separate input to preserve cache reuse.
- Next: reproducible SDK example/lifecycle/catalog, opt-in HM startup/keybinds,
  native detached application handoff, repository smoke tooling and named-reference
  first-presented-frame/query/idle/wakeup/RSS/worker release measurements.

## 2026-10-10 — Shared Dock and Persistent Desktop Widgets

- Completed milestone 5 after `bd8c9bc` (System/OSDs). User explicitly authorized
  every remaining milestone with a commit after each; continue through live themes/
  expanded providers and the SDK/packaging/performance gate after this commit.
- Applications now assigns persistent desktop-ID launch identities across reindex/
  removal. Runner and dock share one `Arc` catalog/icon index, not another scanner.
- One host Hyprland event broker supplies windows/workspaces and version-specific
  native activation. Window IDs include an incarnation, bounded event reads survive
  canceled partial reads, observed closes/reconnect retire identities, and activation
  freshly validates address/PID. Workspaces uses this snapshot, removing duplicate
  threads/font probes while retaining its id-sorted `launch(index)` compatibility.
- Desktop widget IDs/configuration are separate from shared provider models. Normal
  background surfaces use output-relative coordinates and zone `-1`, ignoring other
  clients' bar reservations. Hotplug clamps rendered fallback only; saved intent is
  unchanged. Clock/calendar, Audio media, Battery and new Stats reuse existing state.
- Explicit full-output editing supports add/remove/hide/output selection, drag/
  resize, grid snap, reset and revision-safe Save/Cancel. Save merges original raw
  tables by ID, retaining unknown fields and immutable Nix base ownership.
- Shared `oxi_drag_handle` captures press coordinates directly; stale global cursor
  state had caused nondeterministic drag origins. Normal input defaults to provider
  policy (Stats passive, media only interactive when present), with per-instance
  automatic/on/off override. Policy changes replace the native surface, not its model.
- Optional ABI-6 `dock_entries`, `desktop_view`, `desktop_interactive`,
  `compositor_changed` and `subscription_policy` retain required signatures. Services/
  legacy plugins stay resident; Clock/CalDAV and Stats use one cancellable visible-only
  stream and resume immediately. Hidden widgets cannot multiply provider pollers.
- Actual pointer traces showed exclusive Settings receiving clicks outside its bounds.
  ID-scoped outside dismissal uses actual configured size and existing cleanup hooks;
  no speculative pointer-backend rewrite or click replay. Native dock activation is
  tested after moving away from the fixture workspace, not merely observing focus.
- Full 209 workspace tests/doctests, strict Clippy and Nix host plus six plugin packages
  passed. Private nested Hyprland 0.51.1/headless Sway 1.11/pinned Mesa checks passed
  exact catalog launch/window activation, two-instance drag `(144,176)` and resize
  `432×416`, save/restart/hotplug, unknown fields/read-only base, stale drafts and hide.
  Stats correctly remains active while present in the bar; hiding that last consumer
  pauses it, and restoring it resumes one stream. The earlier assertion was fixture
  logic, not an SDK enum/ownership bug.
- Final verified packages: host `f87gvmi1bagyg6i3pbpvds7n6bmcvlid`, Applications
  `va72v4dcizpn4k85v2qbpzfvg8a0wd1h`, Clock `kmqr9hbnycmygyvxqq07d6h4xyf7v6k4`,
  Audio `rr51zg0whjmyp9c8l63cdj1fwm1fl83r`, Stats `zp3qhi0jysa3xmkjwk0678bikrr3b2vj`.
  The small final Workspaces launch compatibility correction passed its two focused
  tests and strict workspace Clippy, and was repackaged as
  `5r7m46mjihy68ihng93l4m6iwlcpvgs2`; ordinary UI activation was already covered by
  the GUI run.
- Temporary helpers live only in `/tmp/opencode`: desktop-dock smoke, output-mapped
  virtual pointer and isolated compositor coordinator. Parent xdg-shell 6 override and
  fully pinned graphics made a stable test environment; current-system nested Hyprland
  failures were not attributed to an unproved product/OOM cause. All owned children/
  fixtures were cleaned; real desktop/input/hardware/session stayed untouched. Images
  remain private. Repeatable repository tooling and presentation benchmarks are step 7.
- Review external staging before commit; exclude the unrelated pre-existing staged
  deletion of `result`. No push or activation requested.

This file records LLM-specific continuity decisions and low-impact choices that
are useful for future agents. Omitted from this file are high-level architecture
topics already covered by `ARCHITECTURE.md`.

## System services, resident commands and input-transparent OSDs (2026-10-10)

- User requested commit/next step: committed `28f59da feature(services): Add native
  audio and cancellable BlueZ pairing`, preserving unrelated staged `result`
  deletion. New steering: finish this step, then **all remaining roadmap steps**,
  committing after each. Continue directly into milestones 5, 6 and 7.
- One `System` provider combines backlights, modern/legacy power profiles and
  verified login1 capabilities/actions on one cancellable async zbus owner. Keep
  independent health, owner epochs, bounded/coalesced commands and snapshot sequence.
  Backlights alone use a watcher plus driver-compatible fallback poll. Relative
  brightness commands accumulate pending intent. Fresh UID/ID checks gate helper/
  logout calls; confirmations delegate policy, and locker arguments never use a shell.
- Add optional ABI-6 `command`/`osd_view`; required signatures remain unchanged.
  Commands apply ordinary model outcomes before structured completion replies.
  Bounded owned IPC handlers allow fast requests during slow actions. System power
  CLI commands open confirmation only. Audio provides volume/mute/media commands.
- Output-scoped read-only OSDs reuse IDs, abort replaced timers and generation-gate
  expiry. Initial snapshots are quiet; source actions and focused external changes
  route separately. OSD preferences apply live from Appearance. System joins the
  default control-center list after its original six entries and the HM plugin set.
- Packaged tests found SDK `Any` OSD requests lost across dependency graphs: use
  serialized SDK strings, as toasts already do. Also preserve captured command
  context when layer-action conversion returns an ordinary message.
- Actual typing exposed published layershellev 0.17.1 routing keys to the newest
  surface. Import the exact MIT dependency at `crates/layershellev`, outside app
  workspace membership, and separate compositor keyboard Enter/Leave from pointer/
  creation state. Share the path patch across graphs; Crane dummy artifacts retain
  its real API source for iced_layershell. Original registry files remain untouched.
- Verification: 195 workspace tests/doctests, strict Clippy, two private System
  tests, five Nix packages and full private System/Audio/GUI scenario passed. Typing
  through OSD changed duration to 900 while width remained 360; delayed brightness
  stayed on original output after focus moved, with concurrent status under 500 ms.
  Coalescing/expiry, live geometry, confirmation/cancel, foreign UID rejection,
  argument-vector locker, mute/media OSDs, disable and five reopen cycles passed.
- Final packages: host `gfp8ygbv905v2bjbnac79l8k7k03pllk`, System
  `39l189fq6swa0amy6ii8q5gi2rdb6iq6`, Audio `gxrc9p11zngy886ganyq7qknlz1zjg5z`.
  Temporary helper `/tmp/opencode/oxibar-system-osd-smoke.py` uses private bus,
  ordinary files, null PulseAudio/MPRIS and marker-only locker; no actual power,
  brightness/session/audio/daemon changes. Fixtures are cleaned, focus/cursor
  restored and translucent screenshots kept private.
- Hyprland 0.56.2 requires Lua dispatch: fixture monitor focus uses
  `hl.dsp.focus({monitor="DP-3"})`; old `focusmonitor DP-3` is invalid. Carry this
  version-aware capability into dock/workspace adapters. Settled screenshots wait
  for compositor animation; immediate clipping was capture timing, not UI layout.
- Use filtered Nix source and serial package builds. A session-owned Cargo cache
  was cleaned after debug links exhausted storage; subsequent checks consistently
  use dev/test debug=0 and incremental=0. User workspace target/store are untouched.

## Native BlueZ and cancellable task modals (2026-10-10)

- User requested the next step after native audio. Replace Bluetooth's blocking
  polling/placeholder CLI pairing inside the existing plugin; adapt ReSet's
  ObjectManager/domain ideas, not its daemon, GTK stack or fabricated agent replies.
  Keep ABI 6 and one model/subscription across bars, popup and control center.
- Use existing zbus 5.14/async-io on one subscription-owned worker, with a weak
  client bridge. Cache object paths and partial interface/property deltas; resnapshot
  invalidations and skip queued pre-snapshot signals by reply sequence. Address
  actions/agent calls by BlueZ unique-owner epoch; bound/expire commands, reject
  offline work and join cancellation/reconnect cleanup without host Tokio TLS.
- Discovery ownership is per client/adapter; stop only owned sessions on timeout
  or cleanup. Preserve `bluetooth.refresh_seconds` (default 20) as scan duration,
  no longer an inventory timer. Add full-flow pairing deadline default 90 seconds.
  Both settings are restart-only. Keep configured Bluetooth loaded for hotplug.
- Register `KeyboardDisplay`, not the global default agent. Implement every Agent1
  request/display/confirmation/authorization method with owner/device validation,
  typed UI replies, deadlines and explicit rejection. PIN 1–16 printable ASCII;
  passkey 1–6 digits and six-digit display. Pair then trust/connect, cancelling
  pending replies/Pair and a newly in-flight Connect without disconnecting an
  already-connected device. Final completion has a separate reliable channel.
- Review fixed old agent requests cancelling newer sessions: capture the session
  ID through reply/timeouts. Reject prompts after cancellation and suppress queued
  prompts/progress in the model. Private tests include delayed Connect cancellation.
- Add optional ABI-6 `modal_closed(model)` cleanup and window-scoped modal Escape/
  text traversal. Host dismissal, replacement, actual closure and output retirement
  notify once; retired-window cleanup tasks still update models. Hidden bars retain
  modals on connected outputs. Pair event tasks retain their original source output.
- Split Bluetooth DTO/cache, owner, agent, runtime and themed views into sibling
  modules. Use shared oxiced buttons/switches/cards/input, icon fallbacks, inline
  cached-state diagnostics and scrolling; keep host chrome and existing palette.
- Verification: 179 workspace tests/doctests, strict Clippy, four host modal-lifecycle
  tests and both opt-in private-BlueZ tests passed. Fixtures cover all agent methods,
  stale/rejected/spoofed replies, multiple adapters, invalidation, hotplug, owned
  scans, reconnect, cancellation and cleanup without system hardware or host Tokio.
- Nix built host/Bluetooth/audio/applications/clock with the updated optional API.
  Host output is `/nix/store/np5z6135knacpfr2dvz59wjpx8pzml83-oxibar-0.1.0`;
  Bluetooth is `/nix/store/3qqbpkz1br43r1dg0fp1zn8bl78vix5z-bluetooth-0.1.0`.
  Keep filtered source/serial builds; clean only session-owned Cargo artifacts.
- `/tmp/opencode/oxibar-native-bluetooth-smoke.py` uses a private no-autoactivation
  bus injected only into the fixture daemon's system-bus address. Actual GUI PIN
  `0123-abcd`, passkey `000123`, confirmation/authorization/service approval, display
  and Escape/replacement/Connect-stage cancellation all passed. Owned scan timeout,
  hotplug, offline/recovery and five reopens passed with one agent, no default-agent
  request, unchanged startup snapshot baseline (2) and zero `bluetoothctl` calls.
  Virtual-pointer fixtures must emit motion even at unchanged coordinates after
  remapping a surface. No application bug was inferred from early fixture clicks.
  Test children/paths were cleaned; screenshots stayed private and hardware untouched.
- Next: brightness/power/session/OSD services, richer native capabilities, then
  dock/widgets. Incoming pairing/global-agent policy and real hardware permissions
  remain separate extensions/checks. This step and preceding native audio changes
  are uncommitted; preserve the unrelated staged `result` deletion.

## Native audio device backend (2026-10-10)

- User requested commit and the next roadmap step. Commit `99f32f1`
  (`feature(shell): Add multi-output bars and themed control center`) contains
  the completed registry/control-center/roomy UI work. Preserve the unrelated
  pre-existing staged `result` deletion outside commits.
- Implement the next milestone-4 adapter inside the existing Audio plugin rather
  than adding a mandatory daemon or shared-service crate. Adapt ReSet's native
  PulseAudio subscription/domain ideas, not its unsafe threaded callback borrowing.
  Keep ABI 6, existing exports, models, popup/control-center content and MPRIS.
- Add `libpulse-binding` 2.30.1 through Cargo/Nix. Shared Crane dependency builds
  have `libpulseaudio`; only the final Audio plugin gets the extra runtime search
  path. Use one standard mainloop/context on a dedicated thread, not host Tokio TLS.
- Subscription-owned worker cancellation wakes Unix IO/reconnect waits, joins,
  rejects pending requests and permits a later subscription. A weak client slot
  bridges the existing no-model-argument subscription ABI. Thread-affine handles
  never cross into the UI model.
- Default `audio.backend` to `native`; retain explicit `pactl` compatibility, both
  restart-classified. `audio.poll_seconds` remains default 4 and restart-only, but
  now controls MPRIS refresh and devices only in compatibility mode. Native device
  updates use sink/source/server events, coalesced full snapshots and cached indexes/
  channel volumes. Card/profile/stream and incremental-object extensions are deferred.
- Bound native commands/operations, coalesce queued volume/mute replacements and
  reject expired/offline requests instead of replaying them after reconnect. Keep
  last valid devices and inline health status until resubscribe/resnapshot succeeds.
  Filter monitor sources using protocol metadata; scale maximum channel volume
  while preserving balance, restoring equal channels when starting from silence.
- Split device and media snapshots; device failures/events never replace the player.
  Convert MPRIS calls to bounded asynchronous zbus with cancellable async-io polling.
  Generation-gate action acknowledgements per domain and media queries. Review found
  that one global acknowledgement gate could discard a completed media refresh when
  a device action overlapped it; use typed action targets and independent latest
  domain generations while retaining the latest pending action. Add both ordering
  and offline-error regression coverage. Compatibility CLI calls run off the Iced
  update path with bounded output/time and child reaping.
- Add shared themed input/output mute switches and the muted bar label. Scroll the
  compact popup so all controls fit within its existing 420px height. Keep the host's
  control-center scrolling and existing palette; no new per-view backend owner.
- Verification: 171 workspace tests/doctests, strict workspace Clippy, Audio unit
  tests and the ignored private PulseAudio integration passed. That test covers
  balanced volume, defaults/mute, typed monitor exclusion, hotplug, disconnect,
  offline rejection, reconnect and prompt cancellation without a host Tokio runtime.
- Nix host/audio/applications/clock builds and library resolution passed. Final
  Audio output is `/nix/store/mg754dk73ab5n0w76w5j5c8vkwjla50z-audio-0.1.0`.
  Keep filtered-source imports and serial package builds for limited store space;
  cleanup only session-owned Cargo artifacts, never the user's workspace target.
- `/tmp/opencode/oxibar-native-audio-smoke.py` uses private PulseAudio null devices,
  isolated XDG paths and a no-autoactivation private bus/MPRIS fixture. Actual GUI
  device/default/volume/mute and playback/next/previous/player-volume actions passed,
  as did hotplug, offline/recovery visuals and five control-center reopen cycles.
  The final package also passed a delayed media acknowledgement overlapping a
  completed device mute; media refreshed before its 60-second fixture timer.
  A failing shadow CLI recorded zero native `pactl` invocations. Switching backend
  and restarting only the test daemon proved persisted compatibility actions.
  Fixtures were reaped/removed and screenshots inspected privately; the user daemon,
  real devices and media players were not test targets.
- Next: native BlueZ inventory/events, discovery/connect/disconnect and a complete
  cancellable pairing agent. Then remaining brightness/power/session/OSDs and richer
  audio capabilities. Event-driven MPRIS and a separate PipeWire fixture remain
  follow-ups. Native audio work is uncommitted after the requested prior-step commit.

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
