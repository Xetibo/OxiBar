# Technical Debt

## Dock and Desktop Extensions

- Initial dock/widgets are implemented and verified. Running-window activation is
  currently a Hyprland adapter; other compositors retain pinned launches and
  output-aware widgets but need a window/focus capability adapter.
- Dock auto-hide and weather are optional extensions, not enabled implementations.
- Generic output inventory still has its documented fallback polling/focus limits.
  The shared Hyprland window/workspace broker is event-driven.
- Legacy providers default to resident subscriptions. Clock/Stats opt into owned
  visible-only timers; Network/Battery use owned SDK intervals. SDK, loader, private
  app lifetime, compositor smoke and release measurements are now verified.

This file tracks accepted deviations from the current UI contract in `UI.md`. These are allowed only because `oxiced` does not yet provide the needed shared component or semantic token.

It also tracks non-UI runtime/testing limitations that affect future plugin work.

## Runtime Debt

### Theme Compatibility and Remaining Restart-only Settings

- Status: live immutable snapshots and bundled theme hooks implemented.
- Palette/font/density/motion and installed-provider selections apply live;
  legacy `bar.font`, library-code replacement and unsupported backend timers keep
  explicit restart classification.
  Old providers without the optional hook retain their seed palette.
- Compatibility field access interns at most 128 immutable palette/font values
  per library; exhausting that budget requires restart rather than unbounded leaks.
  Wallpaper-derived colors remain an optional extension.
- Further live timer hooks depend on owned resource lifecycles; the release SDK
  gate documents those rules rather than attempting unsafe dylib unloading.

### Launcher Index and Desktop-entry Coverage

- Status: localized fields/actions, inherited icons, filesystem refresh, usage/
  favorites and coalesced cancellable searches implemented.
- Resolved: PATH commands no longer crowd desktop entries in default merged search;
  they have a separate prefix-scoped provider. Application executable/desktop-class
  aliases and comparable exact-match scores retain discoverability of Nix-profile apps.
- Remaining gaps: DBusActivatable-only entries and a persistent mtime index cache.
  Large configured result lists do not yet scroll keyboard selection automatically.
  Tools' files and Commands' executables skip non-UTF8 paths rather than lossy retargeting.
- Clipboard text is bounded, opt-in and ephemeral; non-text/oversized selections
  are not captured. Compositors without data-control keep other Tools providers
  usable but cannot provide native clipboard copy/history.
- Preserve shared indexes and stable immutable actions when extending coverage.

### Native System Backend Migration

- Status: native audio device and BlueZ integration implemented.
- Current state: audio defaults to one native PulseAudio/PipeWire context with
  device/default/volume/mute events and actions, bounded commands, reconnect and
  stream-owned cancellation. Explicit `pactl` compatibility remains available.
  Bluetooth uses one native ObjectManager/property owner with complete typed,
  cancellable pairing and bounded discovery. Control-center views reuse these owners.
  Reviewed ReSet backends provide BlueZ object/property events and broader audio/
  monitor capabilities suitable for selective adaptation.
- Remaining requirements: richer hardware capabilities. Native Hyprland and
  wlr-output-management display settings are now implemented. Keep bounded ownership
  and proven state/action parity;
  donor placeholders, blocking waits and unsafe thread markers are not reused.
- Target fix: the ReSet reuse workstream in `SHELL_ROADMAP.md`; keep existing plugin capabilities and themed Iced views throughout migration.

### Remaining Audio Capabilities

- Status: accepted initial native-backend limits.
- Current state: sink/source/server events trigger coalesced full device snapshots,
  not incremental per-object queries. MPRIS uses bounded asynchronous DBus calls
  and cancellable periodic refresh. The compatibility adapter still runs `pactl`.
- Gaps: card profiles/ports, per-application stream routing and event-driven media
  properties are not implemented. PulseAudio protocol parity was verified against
  a private server; a separate PipeWire server fixture remains a follow-up.
- Target fix: extend the existing owner with typed card/stream capabilities and
  media signals when needed; avoid adding a service/poller for each surface.

### Remaining Bluetooth Capabilities and Hardware Verification

- Status: user-initiated native pairing implemented and privately verified.
- Current state: all Agent1 request/display/confirmation/authorization methods have
  typed responses, owner/device validation, deadlines and full-flow cancellation.
  The plugin registers its own agent without claiming the global default role.
- Gaps: unsolicited incoming pairing/default-agent policy, discoverability settings
  and further adapter/device capabilities remain extensions. Private mock-BlueZ
  tests cover multiple adapters, property invalidation, hotplug and reconnect;
  real hardware/BlueZ-version and system permission-policy checks remain manual.
- Target fix: extend the same owner and explicit UI policy rather than introducing
  another connection/agent per view or importing placeholder replies.

### Remaining Control-center Services and OSDs

- Status: initial milestone implemented.
- Current state: host-owned control-center selection, live preferences, source
  routing and existing audio/network/Bluetooth/battery/notification/calendar views
  work with one shared model/subscription per provider. System adds backlight,
  power-profile and verified session controls; host OSDs reuse cached provider state.
- Remaining extensions: additional driver-specific brightness devices, profile
  holds and richer native Audio/BlueZ capabilities. Actual hardware/system policy
  checks remain manual; private fixtures exercise ordinary files and mock services.
- Backlight refresh uses one watcher plus a bounded fallback poll because sysfs
  notification support varies. Remote profiles/session state remains event-driven.
- Keep domain health independent and avoid one backend/poller per surface.

### Layer-shell Keyboard Ownership

- Status: keyboard-stealing bug resolved locally.
- Published layershellev 0.17.1 routed keyboard events through its newest/pointer
  surface. Creating a non-interactive OSD redirected typing from existing settings.
- The local patch separates compositor keyboard Enter/Leave from pointer/creation
  state. Packaged checks prove typing and applying settings through an OSD.
- Repeated seat capabilities retain resources rather than rebinding/synthesizing
  unfocus; removed devices release resources and retired keyboard events are ignored.
  Private provider typing/copy/reopen checks passed with virtual-device startup
  delays in the fixture, without changing the user's input devices.
- Resolved: removing a temporary keyboard could leave an unscheduled repeat behind
  and unwrap a missing keyboard at timer installation. Focus loss, release and device
  removal now clear pending repeats; installation revalidates live ownership.
  The complete private compositor UI flow passes rapid Escape/reopen afterward.
- Resolved: removing a temporary keyboard could leave the host's focused-surface
  set stale and cause paste handoff timeouts. Capability loss now clears keyboard
  ownership and emits Unfocus; ordinary native Leave remains the paste barrier.
- Follow-up: upstream the correction and evaluate a released replacement before
  removing the local dependency; startup connection error propagation remains separate.

### Output Inventory Refresh and Remaining Shell Capabilities

- Status: initial multi-output slice implemented.
- Current state: explicit surface registry, per-output profiles, hotplug
  reconciliation and Hyprland/Wayland inventory work. Inventory is
  refreshed on background workers at `monitor.refresh_seconds` (default 2), and
  unchanged snapshots do not redraw the UI.
- Gaps: fully event-driven inventory and further compositor focus adapters remain
  follow-ups. Generic wl_output names/logical geometry alone do not provide global
  focus or display control; the separate native management capability is required.
  Compositors without
  output names retain compositor-selected placement.
- Target fix: event-driven output service and additional compositor adapters;
  do not conflate shell placement with display-mode/compositor ownership.

### Display Configuration Boundaries

- Status: native Hyprland and wlr-output-management preview/save/revert implemented
  and tested on owned Hyprland 0.51.1/Sway 1.11 sessions.
- GUI mode/refresh, scale, position/diagram, transform, enablement and supported VRR
  use a bounded owner and current protocol serials. Profiles persist sparsely over
  Nix, with startup application available; fractional refresh and zero compositor-
  defined refresh are preserved.
- The 20-second rollback lease survives GUI dismissal/stalls while the controller
  process runs. Orderly drop and failed persistence restore the baseline; SIGKILL
  or process crash cannot run in-process rollback. An external watchdog is a future
  extension. Physical link/driver/VRR policy, compositor restart and HDR/color-profile
  checks remain separate from the private-headless proof.
- Compositors without the management protocol/native adapter retain inventory and
  show unsupported-control diagnostics; Mutter/KWin-specific apply adapters are not
  silently emulated by shell geometry.

### Window Content Overview

- Status: implemented and verified; `WINDOW_OVERVIEW_PLAN.md` records the design.
- Native Hyprland export v2 is tested on a private 0.51.1 compositor with real changed
  occluded/other-workspace pixels, exact activation, hotplug and repeated cleanup.
  Modern ext capture with exact Hyprland mapping/wlr activation is tested against a
  hermetic Wayland server, including resize, denial and recycled native addresses.
  An actual Hyprland 0.56.2 nested fixture failed before connection; no real-modern-
  compositor runtime claim is inferred from that attempt.
- Modern compositors need an exact mapping/output/activation bridge; ext toplevel
  listing alone provides neither output membership nor activation. Sway 1.11 has
  native window inventory/activation but no supported per-window capture path in
  this implementation, so its overview explicitly reports capture unsupported.
  Mutter/KWin-specific capture, minimized-window policy and DMA-BUF imports remain
  extensions; output-only capture is not substituted for occluded window content.
- SHM staging and thumbnail image data are bounded at 64/96 MiB, two jobs and 256
  windows. Driver texture overhead is not an exact image-byte counter. Legacy export
  refreshes at the configured bounded cadence, while modern frames can wait for
  compositor damage/readiness; reduced unchanged-frame work is a possible optimization.
- Performance reference measures actual overview presentation and closed resource
  release on Sway separately from the real-pixel Hyprland proof. Closed capture
  workers and draws are zero; all-thread voluntary context switches remain a wakeup
  proxy, not a kernel hardware-wakeup measurement.

### Dock Icons and Calendar Opening

- Resolved: SVG catalog paths were passed to the raster renderer and absent icons
  became letters. Shared format-aware icons, portable fallback, automatic theme
  selection, filename aliases and ranked window-class matching now retain actual icons.
- Resolved: animation constrained calendar/form layout on every frame, and calendar
  refresh/launch calls blocked the Iced executor. Stable reveal layout and bounded
  off-executor jobs are privately measured while a two-second CalDAV sync is blocked.
- Resolved: full-layout reveal children could capture clicks beyond their visible
  clip. Shared pointer/hover clipping now preserves neighboring controls' hit targets.
- Remaining desktop-entry coverage such as DBusActivatable-only entries stays in the
  launcher section; this fix does not claim every application ships a usable icon.
- Resolved: provider switches now load Applications immediately, and Nix profile
  directories supplement incomplete session XDG paths while preserving precedence.
- Resolved: the development installer omitted Applications despite enabled config,
  leaving the dock without a catalog. It now preflights all fourteen libraries,
  includes Applications, Commands and OxiPaste and respects custom build/config roots.
  The Nix host supplies a pinned
  Applications fallback after user-installed libraries, still requiring enablement.

### Persistent Clipboard Integration

- Resolved: clipboard views depended on a separately installed/running daemon and
  could appear empty when only the provider was enabled. The provider now owns
  native persistent capture in one resident, cancellable stream, including while
  its views/bar are hidden. Disable stops capture without deleting saved data.
- Direct shell access uses a registered provider-backed picker and window-command
  IPC; it no longer depends on a visible bar or a provider command ABI export.
- Resolved: the shared Home Manager/NixOS module default omitted the clipboard
  provider. It now installs the library and enables it in the generated base;
  explicit plugin selections remain authoritative. Its backend ships in that library.
- Remaining: external-service polling and manual Refresh transfer complete D-Bus
  snapshots, and image preview decoding/allocation is delegated to Iced. Remote
  revision/signal snapshots and bounded cached thumbnails are future optimizations
  for large image-heavy histories. Bundled capture is already event-driven.
- Paste relies on compositor virtual-keyboard support, native focus restoration
  and the configured application shortcut. Unsupported injection returns an error
  after copy; no compositor-specific focus heuristic is substituted.

### Legacy Bar Label Capability

- Built-in status providers export optional `bar_view` for live output-local labels;
  clock/workspace content remains meaningful when names are hidden.
- Older third-party libraries without this hook retain their existing opaque Iced
  views. The host cannot reliably remove text from arbitrary elements. Rebuild updated
  built-ins together with the host; required ABI 6 and existing contexts are unchanged.

### Isolated GUI Panic Diagnostics

- An earlier follow-up run lost an `Option::unwrap()` location. Improved diagnostics
  subsequently located a layershellev pending-repeat unwrap after temporary keyboard
  removal; that lifecycle bug is fixed and the complete isolated UI flow passes.
  The earlier location was not retained, so its identity cannot be proven retroactively.
- Smoke now retains a larger log buffer, enables backtraces and fails on caught
  startup/runtime panics rather than accepting a restarted daemon. Keep that diagnostic
  coverage; the independently fixed equal-geometry anchor cache is not a proven cause.

### Home Manager Plugin Path

- Status: resolved.
- Current state: base configuration and plugin libraries both use `xdg.configFile`;
  GUI overrides are excluded from Home Manager ownership. Nonstandard config homes
  were checked using Home Manager module evaluation.

### iced_layershell Startup Connection Panic

- Status: mitigated, not root-fixed.
- Affected code: `src/app.rs` `run()` / `run_bar()`, dependent on iced_layershell 0.17.1 and layershellev 0.17.1.
- Current state: when the compositor is not yet accepting clients, `layershellev::WindowState::build()` calls `Connection::connect_to_env()?` and iced_layershell -0.17.1 `multi_window.rs` turns that into `.expect("Cannot create layershell")`, a panic. The host works around this with a config-driven retry loop (pre-flight `connect_to_env()` probe plus `catch_unwind` around `run_bar()`), so transient startup failures retry with exponential backoff instead of exiting.
- Why accepted: a real fix requires surfacing connection errors through iced_layershell's `Error` enum instead of panicking.
- Target fix: patch or update `iced_layershell`/`layershellev` so `WindowState::build()` connection failures return an `Error` variant rather than panicking; then the probe/`catch_unwind` workaround can be simplified or dropped. Tracked as a future option pending an upstream or vendored dependency update.

### Compositor-Specific Active Toast Output

- Status: accepted temporary limitation.
- Affected code: `src/monitor/` and `src/messages.rs`.
- Current state: toast layer creation asks the `src/monitor/` backend registry for an active output name. The only backend currently queries Hyprland's focused monitor name and passes it to layer-shell as `OutputOption::OutputName`; if no backend returns an output name, toasts fall back to `OutputOption::LastOutput`.
- Why accepted: the available `wayland-protocols` output protocols describe/configure outputs and surfaces, but do not provide a compositor-neutral global focused-monitor query for layer-shell clients.
- Target fix: switch to a compositor-neutral focused-output source if `iced_layershell`, layer-shell integration, or a shared workspace/monitor service exposes one.

### Hardcoded Bar Width

- Status: resolved.
- Current state: the main layer requests automatic width from the compositor-selected active output with layer-shell width `0` and left/right/top anchors. `OxiBar` stores the configured width from iced window events, and `[bar] width` / `[bar] height` provide config fallback/override values.

### Missing Dynamic Plugin Loading Integration Test

- Status: resolved with the Nix-built SDK example.
- Actual loader tests verify metadata inspection does not initialize models,
  availability skips before factories and duplicate names instantiate once.
  The real example contract exercises 25 shared-model/task/view/stream/theme/config/
  modal cycles without a host Tokio reactor; initialized code remains process-resident.
- Live GUI verification enables installed Applications/Tools without restart,
  searches and launches an owned desktop fixture and executes calculator copy.
  Host tests retain stable model data through disable/re-enable and reject late
  disabled-provider host requests.

### Release and Application Lifetime Tradeoffs

- Named Sway/headless release gates passed actual presentation, bounded local query,
  idle CPU/draws and reopen RSS/workers. The record is workload-specific; all-thread
  voluntary context switches are a wakeup proxy, and RSS uses a documented 2 MiB
  noise budget. See `RELEASE_CHECKS.md` and `performance-reference.json`.
- Nested Hyprland 0.51.1 did not advertise its presentation clock and returned stale
  feedback; it is used for owned UI smoke, not the presentation reference.
- Successful plugin code is retained until process exit for opaque/deferred Rust
  vtable safety. Existing notification/tray bus servers remain process-lifetime
  providers; safe generic in-process unloading is not part of ABI 6.
- Disabled models remain resident for stateful re-enable; model-owned legacy bus
  workers may remain until exit even though host subscriptions/consumers stop.
  Replacing initialized library code still needs restart.
- Native user-manager app handoff escapes the shell termination cgroup where
  supported. Direct fallback creates a reaped process group but cannot escape an
  existing systemd cgroup without a user manager.
- Resolved: packaged/development shell graphics and font overrides no longer leak
  into handed-off apps. Post-exec application errors now retain stderr diagnostics.
  Follow-up journal evidence identified empty `WAYLAND_DISPLAY` as the cause of
  OpenCode/Nheko connection refusal. Startup and handoff recover a live endpoint;
  transient unset rules prevent stale manager FD/driver variables from reappearing.
  Both installed applications now open through the launcher in an owned compositor
  with an intentionally empty incoming display. Clipboard follows the same repair.

## UI Theme Debt

### Scheme Compatibility and Custom Contrast

- Resolved: built-in Catppuccin muted text, filled-control text and neutral hover
  use readable semantic roles. Base16-only inputs derive coherent Base24 extensions.
- Base16/Base24 remains an ecosystem adapter, not a GUI semantic standard. Explicit
  semantic overrides are preserved; transparent/backdrop-dependent surfaces and
  arbitrary custom overrides cannot share the automatic opaque-background guarantee.

### Settings and Control-center Accessibility

- Status: shared-control accessibility follow-up.
- Current state: keyboard search, text-field traversal, Enter/Apply, category/tab
  shortcuts and window-scoped Escape work. Settings now have animated nested
  groups, breadcrumbs and a narrow full-path picker. Generic Iced button/switch keyboard
  focus and screen-reader semantics are not yet uniformly exposed by shared controls.
- Target fix: reusable focusable/accessible controls in `crates/oxiced`, with
  full form navigation and runtime checks as the settings pages expand.

### Native Locking and Login Handoff

- Status: planned stages implemented and verified. Independent protocol/PAM locker,
  fullscreen greetd greeter, kiosk packages, bounded themed assets, native input/wake
  scheduling and readiness-aware sleep paths have private and real VM checks.
  Existing shell actions use the configured locker; system PAM/greetd policy remains
  administrator-owned. Deployment and current verification are in `AUTH_DEPLOYMENT.md`.
- Rebuild-time inhibitor FD inheritance is resolved: unrelated descriptors are
  retired at locker exec, and private lease checks tolerate bounded asynchronous
  cleanup while retaining their live-child ownership assertions.
- Resolved: locker mouse interaction now publishes a visible native cursor and
  explicit response focus; Tab/Shift+Tab reaches enabled input/buttons without
  moving secrets into ordinary widgets. Cursor-shape uses Hyprland's theme instead
  of the fallback arrow. Ordered dispatch preserves click-then-type input in one
  native batch; a shared-password/native-XKB fixture verifies this behavior.
- Resolved: the greeter ignored response-focus messages and handled only ignored
  keyboard events. It now shares explicit prompt focus, accepts captured keys while
  prompting and derives missing text from modified character keys. Private greetd
  frontend fixtures and the default Hyprland VM verify editing and focus traversal.
- Resolved: greeter startup selects CPU rendering before GPU initialization.
  Authentication packaging no longer pins the compositor to a software Vulkan ICD;
  the Cage login/lock/suspend VM passes with deliberately invalid Vulkan settings.
- Resolved: pinned authentication libc could not load the newer system PAM and Mesa
  modules (`GLIBC_2.43` missing), preventing password prompts and Cage EGL startup.
  Module defaults now build from the consuming system/Home Manager package set.
  System-vendor GLES2 context creation and caller-package-set assertions pass.
  Explicit standalone package overrides still need a compatible system ABI.
- An explicit-suspend VM run encountered asynchronous block-inhibitor retirement
  and refused Suspend; the rerun passed. Timeout diagnostics now include the
  inhibitor list. This does not change the readiness/authentication gates.
- Actual DRM/firmware behavior beyond the tested virtual devices still needs hardware
  coverage. The pinned virtio-GPU fence queue stalls after QEMU S3; the sleep fixture
  uses legacy VGA/pixman while Hyprland retains normal virtio-GPU tests. No production
  authentication or ownership policy is weakened for that fixture.
- Generic Iced greeter input exposes no initial exact Caps state or compositor group
  name: it displays the enforced kiosk layout and unknown Caps until observable key
  state establishes it. The locker uses native XKB state. Dedicated IME integration,
  uniform assistive semantics, X11 sessions and optional media/notification UI remain
  independently scoped extensions; they are not authentication bypass fallbacks.

### Selectable Search Result Row

- Status: resolved.
- Current state: launcher and settings navigation use the workspace `oxiced`
  selectable-row helper, sharing neutral/selected/hover/pressed states.

### Missing Additional Flat Button Variants In `oxiced`

- Status: partially resolved.
- Affected code: popup row actions in `plugins/network/src/lib.rs`, `plugins/notifications/src/lib.rs`, and `plugins/tray/src/lib.rs`.
- Current state: `oxiced::widgets::oxi_plugin` provides shadowless bar buttons,
  normal-sized surface buttons, source/mode badges and flat background styles;
  themed switches are shared through `oxi_toggler`. Settings/control-center actions
  use these helpers. Some compact popup row/action buttons still use local styles
  or `oxi_button::button`.
- Workspace/focus pills now use shared accent selection with readable inactive text.
- Target fix: migrate remaining compact popup row/action buttons to `oxi_plugin` helpers or add more helper variants as needed.

### Duplicated Bar Button Styling

- Status: resolved.
- Current state: `plugins/audio/src/lib.rs`, `plugins/bluetooth/src/views.rs`, `plugins/clock/src/lib.rs`, `plugins/network/src/lib.rs`, `plugins/notifications/src/lib.rs`, and `plugins/tray/src/lib.rs` use `oxiced::widgets::oxi_plugin::bar_button` or `bar_button_style`.

### Missing Compact List Row/Card Helpers

- Status: accepted temporary exception.
- Affected code: `plugins/audio/src/lib.rs`, `plugins/bluetooth/src/views.rs`, `plugins/network/src/lib.rs`, `plugins/notifications/src/lib.rs`, `plugins/tray/src/lib.rs`.
- Current state: plugins build compact row/card containers with local `container::Style` or `button::Style` closures while still using `OXITHEME` colors and spacing/font/radius tokens where available. `audio` media/device cards now use `oxi_plugin::compact_card`.
- Current state: `oxiced::widgets::oxi_plugin` includes `compact_card` and `compact_card_style`, but remaining plugin-local list-row styles have not all been migrated yet.
- Target fix: migrate popup list-row/card wrappers to `oxi_plugin` helpers.

### Missing Text Role Helpers

- Status: partially resolved.
- Affected code: most plugin popup/panel/modal views.
- Current state: `oxiced::widgets::oxi_plugin` provides `text_primary`, `text_muted`, and `text_accent`; several plugin-local title/section closures remain.
- Target fix: migrate remaining section/title text closures and add more semantic text helpers only where needed.

### Calendar Adjacent-Month Color

- Status: resolved. Adjacent-month days use the shared readable muted-text role
  instead of a low-alpha accent. Bounded calendar content fills the available body
  height with theme-derived spacing.

### Missing Shared Tooltip Chrome Helper

- Status: accepted temporary exception.
- Affected code: `plugins/clock/src/lib.rs`, `plugins/battery/src/lib.rs`.
- Current state: clock calendar event days and the battery bar item use local themed tooltip container styles so hover details share the same chrome.
- Why accepted: `oxiced` does not currently provide a shared plugin tooltip helper.
- Target fix: add an `oxi_plugin` tooltip chrome helper and migrate local tooltip styles to it.

### Tray DBusMenu Completeness

- Status: accepted temporary limitation.
- Affected code: `plugins/tray/src/lib.rs`, `plugins/tray/src/system.rs`.
- Current state: tray rows render `com.canonical.dbusmenu` `GetLayout` entries in a right-click context menu and dispatch selected entries through `Event(id, "clicked", ...)`. Items without DBusMenu fall back to StatusNotifier `Activate`, `SecondaryActivate`, and `ContextMenu` actions. StatusNotifier registration returns immediately with a fallback item and refreshes metadata asynchronously.
- Limitation: advanced DBusMenu properties such as icons, keyboard shortcuts, toggle/check/radio state, and lazy submenu refresh are not rendered yet.
- Target fix: extend DBusMenu rendering to cover icons, shortcuts, toggles, and `AboutToShow` submenu refresh behavior.

### Clock CalDAV Transport

- Status: accepted temporary implementation detail.
- Affected code: `plugins/clock/src/caldav.rs`.
- Current state: CalDAV sync shells out to `curl`, requires `https://`, forces HTTPS protocol use, and stores credentials in a temporary `0600` curl config rather than command-line arguments.
- Target fix: consider a native minimal HTTP/TLS client if the workspace adopts a stable HTTP dependency.

### Clock Fractional-Second Display

- Status: accepted temporary limitation.
- Affected code: `plugins/clock/src/lib.rs`.
- Current state: the display granularity floor is one second. Formats that show sub-second precision (`%f`, `%.3f`) still update at 1 Hz, so fractional digits are anchored to the last re-read of the real clock rather than tracked continuously.
- Target fix: add sub-second simulation granularity or an explicit precision config if fractional-second display is ever required.
