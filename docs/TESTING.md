# Testing

## Strategy

- Unit tests cover host main-loop helpers and every plugin crate.
- Tests avoid live DBus, NetworkManager, Bluetooth, Hyprland, PulseAudio, Wayland, or notification daemon dependencies unless explicitly marked as integration tests.
- External command wrappers remain manually verified unless they are refactored behind injectable seams.
- Plugin system modules carry parser/helper tests next to the code they own.

## Current Unit Coverage

- Host/config: plugin allow-list parsing, plugin unavailable error formatting, anchor parsing, host-request message mapping, dynamic bar width/height fallback parsing, popup geometry/config/dynamic metrics, toast keyboard interactivity actions, toast output-option fallback mapping, panel output anchoring, bar section lookup, layer resize state updates, popup state setters, `[instance]` policy parsing (defaults, configured values, wrong value types), and single-instance lock acquisition (second acquire reports already-running; lock released when the guard drops).
- `audio`: percent parsing, file URL decoding, selected-device fallback, config
  interval, view/popup construction, error draining, balanced/silent channel-volume
  conversion, bounded/coalesced action queues, offline rejection, worker cancellation,
  independent device/media state, overlapping-domain acknowledgements, and stale
  action/query rejection.
- `battery`: sysfs battery availability/parsing, percentage and duration estimates, config poll interval, view construction, and error draining.
- `bluetooth`: typed object-cache reduction/noise filtering, config and view
  construction, PIN/passkey validation, stale replies and old-session cancellation,
  queued-prompt suppression, cached offline state, owner cancellation/resubscription
  and error draining without a system bus or hardware.
- `clock`: config parsing, invalid config defaults, format second-specifier detection, display-interval granularity, simulated-now arithmetic, month arithmetic, Thunderbird launcher mapping, CalDAV HTTPS guard, iCalendar/CalDAV parsing, recurrence/RDATE expansion, event tooltip details, ABI metadata, and error draining.
- `network`: `nmcli` field splitting, scan interval parsing, label/icon helpers, view/popup/modal construction, and error draining.
- `notifications`: markup stripping, notification action selection, notification timeout config, notification add/replace/remove/clear state transitions, DND toast suppression, toast-only expiry, hover/focus/draft expiry preservation, toast keyboard opt-in requests, panel rendering, and error draining.
- `tray`: StatusNotifier address normalization, tray label fallback, DBusMenu label cleanup, item/hover/context-menu state update, dynamic popup metrics, popup/action rendering, and error draining.
- `workspaces`: active workspace state update, empty launch behavior, view construction, and error draining without requiring Hyprland.
- Runner: XDG precedence/hidden masking, localized fields/actions, inherited/cyclic
  icon themes, argument expansion, stable catalogs and bounded ranked matching;
  latest-running/pending cancellation, scope reactivation, metadata load/save races,
  private versioned state, exact-provider activation and stale displayed identities.
- IPC: command validation, configured socket path, a real command/acknowledgement round trip and stale-socket recovery.
- Settings: sparse merge/reset/source behavior, explicit equal-to-base intent,
  readonly Nix symlink regeneration, unknown-field preservation, private atomic
  writes, managed-symlink refusal, revision conflicts, invalid-input retention,
  live geometry/input-region actions, widget layout, retained plugin models and
  structured socket replies. Plugin tests cover live hook state preservation.
- Multi-output: stable IDs, independent logical dimensions/profiles, popup/modal
  source routing, output fallback/disable, hotplug cleanup, retained plugin models,
  late task results and unknown-window rejection. Monitor parser tests preserve
  fractional scale/rotation, negative coordinates and precise refresh rates.
- Control center: optional capability/fallback selection, configured ordering and
  deduplication, retained provider state, source-scoped async modals, overlay
  dismissal, live resize/disable/reset, visible-inbox toast suppression, hidden-bar
  retention, output-removal cleanup and late model updates.
- Modal lifecycle: optional cleanup once on Escape/replacement/compositor closure/
  overlay dismissal/output removal, hidden-bar retention, source placement and
  cleanup/background results after window retirement.
- System: backlight bounds/rounding/path validation, quiet baselines, independent
  domain acknowledgements, repeated relative intents, confirmation-only session
  commands, locker argument parsing and live config preservation.
- OSD/commands: per-output ID reuse, stale expiry, input-transparent layer settings,
  live geometry/disable, output retirement, authoritative model-before-reply,
  captured async output, serialized SDK transport and concurrent slow/fast IPC.
- Dock/desktop: shared stable application catalogs, incarnation-safe window IDs,
  bounded cancellation-safe event lines, two shared-model widget instances,
  logical fallback/restore, grid drag/resize, revision conflicts, unknown fields,
  hide/add/remove/output assignment, passive-input overrides and visible ownership.
- Clock/Stats: bounded proc parsing and immediate owned-timer resume without a host
  reactor. Modal tests also cover actual-bound outside dismissal and cleanup once.
- Themes: exact legacy seed, immutable/exact-Arc delivery, bounded interning without
  revision growth, alpha/preset/density/motion validation, live source/reset, retained
  startup font and late uploaded-font guards.
- Tools: bounded calculator parsing, all provider prefixes, quoted argument vectors,
  bookmark/action validation, immutable edited actions, opt-out history clearing,
  no clipboard text in identity/usage state and actual failed copy outcomes.
- Provider validation: pinned library lifetime, rejection before adoption/writes,
  longest-prefix cancellation and sparse live favorite changes. Filesystem tests
  cover nonrecursive missing parents, created roots and prompt owned watcher drop.

## Expected Commands

- `nix develop -c cargo fmt --all -- --check`
- `nix develop -c cargo test --workspace`
- `nix develop -c cargo clippy --workspace --all-targets -- -D warnings`
- `nix build .#oxibar .#oxibar-audio .#oxibar-bluetooth .#oxibar-system` after native backend/API
  changes, with more plugin package targets as needed.

For this runner integration, build `oxibar`, `oxibar-applications`, and a retained
bar plugin such as `oxibar-clock`. While new files are still untracked, use the
local-path flake reference so Nix sees them:

```sh
nix develop path:. -c cargo test --workspace
nix develop path:. -c cargo clippy --workspace --all-targets -- -D warnings
nix build path:.#oxibar path:.#oxibar-applications path:.#oxibar-clock --no-link
```

A `path:.` import also includes ignored build directories before Cargo source
cleaning. For a workspace with a large `target/`, use an equivalent filtered
source import for checks instead of copying build artifacts into the Nix store:

```sh
nix develop --impure --expr '
  let source = builtins.path {
    path = ./.;
    name = "oxibar-verification";
    filter = path: type:
      !(builtins.elem (builtins.baseNameOf path) [".git" ".direnv" "target" "tmp"]);
  }; in (builtins.getFlake (builtins.unsafeDiscardStringContext (toString source)))
    .devShells.${builtins.currentSystem}.default
' -c cargo test --workspace
```

The dylib regression test is explicitly opt-in because it requires an actual
built library. Run it against the Nix plugin output (or a same-toolchain Cargo
build). It executes plugin indexing/search tasks and its timer stream without
entering a host Tokio runtime:

```sh
plugin=$(nix build path:.#oxibar-applications --no-link --print-out-paths)
nix develop path:. -c env OXIBAR_APPLICATIONS_PLUGIN="$plugin/lib/libapplications.so" \
  cargo test -p oxibar --test runner_dylib -- --ignored
```

## Dock and Desktop Runtime Verification

Use a private compositor, independent XDG/base/override/lock/socket files, built
Applications/Clock/Audio/Stats/Workspaces libraries and a single harmless fixture
desktop entry. Check actual dock launch and exact existing-window activation after
switching away, rather than treating an already-focused window as activation proof.

Add two calendar instances plus media/stats. Drag and resize using event-local
pointer presses, save, restart and hotplug the requested output. Verify unknown
fields and a read-only base symlink survive, fallback does not overwrite intended
geometry, stale saves retain dirty drafts, and hidden widgets remove their surfaces.
Check passive input flags and one visible-only subscription shared across consumers:
hide both the widget and any bar/OSD consumer before asserting it is paused.

The phase-5 packaged run passed every check using pinned Hyprland 0.51.1 nested on a
private headless Sway 1.11 parent, pinned Mesa and an isolated DBus/MPRIS fixture.
The temporary parent advertised supported xdg-shell version 6 for Aquamarine.
Virtual pointer clients were output-mapped; the real desktop/input stayed untouched.
Captured geometry was `(144,176)` and `432×416`; restart/hotplug preserved it.
Outside Settings dismissal released the compositor grab before exact dock activation.
209 workspace tests/doctests, strict Clippy and Nix host/six-plugin packaging passed.
Images remain private; first-presented-frame release benchmarks remain milestone 7.

## Launcher Runtime Verification

On a layer-shell compositor, use an isolated XDG config, a distinct instance
lock/socket, built application and clock libraries, and a fixture desktop entry.
Check typed search plus Enter activation, Escape, repeated IPC open/close, focused
output placement, and continued bar/plugin operation after launching. Do not
launch arbitrary real applications as fixtures.

`RUST_LOG=oxibar=debug` logs elapsed time from handling an open request to the
first launcher surface configure event. This excludes client startup and does
not measure the first painted/presented frame; measure CLI round trip and frame
latency separately. The packaged Hyprland smoke run is recorded in `DECISIONS.md`.

Oxiced is a local workspace member at `crates/oxiced`; workspace checks include
its library, demo targets and doctests. Focused checks use:

- `nix develop -c cargo test -p oxiced`
- `nix develop -c cargo clippy -p oxiced --all-targets`

## Live Theme and Expanded Provider Runtime Verification

Use private XDG config/data/state, immutable base symlink, distinct lock/socket and
a private compositor/bus. Theme checks change a real GUI density field, then palette,
font, scale and reduced/zero motion; verify all loaded upgraded providers receive the
same revision, font upload completes, invalid edits retain state and restart/reset
preserves GUI-over-Nix semantics. Inspect settings, widgets, controls and launcher
without publishing screenshots that may reveal desktop content.

The native clipboard test is opt-in and must run against an owned compositor:

```sh
OXIBAR_TEST_PRIVATE_WAYLAND=1 cargo test -p tools private_wayland -- --ignored
```

The tested isolated fixture injected its private parent socket and compared a
modern-preferred producer with a forced wlr consumer. It proved copy/history bounds,
opt-out/current recapture, monotonic identities, duplicate subscription isolation,
resume and owned shutdown without a host reactor. It never read the real clipboard.

Packaged launcher checks exercise actual typed prefixes and Enter: calculator copy
while history is disabled, opt-in history/paste, PATH arguments, named actions with
literal shell-looking text, private files/URL opener, French main/action names,
star persistence, filesystem refresh before the fallback timer and stable old IDs.
Validate no clipboard content in private state, reject invalid URL records before
write, retain the immutable base, clear on opt-out and restart. Virtual keyboard
fixtures wait for device binding before modifiers/keys and for actual layer
retirement before reopening; input screenshots alone are not action proof.

These runs passed: 236 workspace tests/doctests, strict Clippy, Nix host/Applications/
Tools/Clock packages, the private native clipboard integration and the full isolated
provider GUI. The earlier theme GUI passed coherent snapshots, real density editing,
canonical live font, invalid-input retention and restart/reset across four upgraded
providers. The backend capability patch retains devices on unchanged capabilities
and ignores retired keyboard proxies. First-presentation release measures remain
milestone 7, rather than being inferred from layer configuration or sleeps.

## Settings Runtime Verification

Use a private XDG root, a readonly symlinked base, distinct instance/control
paths and built application/clock plugins. Open `oxibar settings`, search for bar
height, Tab to the field, change it and press Enter. Verify `overrides.toml`,
effective source via `oxibar config get bar.height`, actual layer height and
continued plugin operation.

Replace the base symlink as Home Manager does: the GUI height override must remain
while an unoverridden launcher limit updates automatically. Reset the height to
reveal the new base. Also check stale JSON revisions, malformed external TOML,
recovery, restart-only indicators, GUI access with launcher disabled, launcher
activation and persistence across daemon restart. The isolated packaged Hyprland
scenario passed all these checks.

Home Manager path verification uses a custom `xdg.configHome` and confirms that
both base and library symlinks target that directory and `overrides.toml` is not
managed. No real Home Manager activation is needed for this module check.

Run commands through `nix develop` when the ambient Rust toolchain was installed through Nix or rustup with a Nix linker wrapper.

## Multi-output Runtime Verification

Use private XDG config/locks/socket and application/clock plugins. Set
`bar.outputs = ["*"]`, compare actual layer widths with logical output geometry,
and apply a named bottom/height profile without altering the other bars. Check
`oxibar outputs`, the Monitor settings page, live output selection and settings
access while `bar.enabled = false`.

The isolated Hyprland smoke passed across three physical outputs, including a
portrait output. A temporary headless output was added/removed to verify hotplug
reconciliation; cleanup removed only that test output. Loaded plugin set remained
Applications/Clock throughout. Deterministic tests cover source-owned surface
cleanup and late background results.

The same packaged smoke also passed with `HYPRLAND_INSTANCE_SIGNATURE` omitted,
forcing the generic wl_output/xdg_output backend. This verifies protocol fallback
inventory/geometry/hotplug on the running compositor; a separate Sway/Niri session
has not been exercised. Existing settings/Nix-override smoke and the opt-in real
runner dylib test also passed after the surface refactor.

For limited temporary storage, build packages with `nix build --max-jobs 1 ...`:
each final derivation unpacks the shared Crane artifact cache.

## Control-center and Visual Runtime Verification

Build the host and application/audio/battery/Bluetooth/clock/network/notification
libraries with the same pinned toolchain. Use a private XDG config and distinct
lock/socket. Test notifications on a private session bus with no service
autoactivation, so fixtures do not involve the desktop notification daemon or
start unrelated portal services.

Check `oxibar control-center [open|close|toggle]`, Alt+number selection, scoped
Escape, repeated reopen, live width/widget/enablement changes, and settings/
launcher dismissal. Open the center while bars are hidden, and verify invalid
dimensions retain the last valid state. Render every available provider, including
the legacy calendar popup fallback.

Send fixture notifications while another provider is selected, then select the
inbox: visible toasts close and new notifications remain stored without toasts.
Switch away and verify later toasts resume. Provider state and the loaded plugin
set must survive close/reopen.

Inspect settings/control-center screenshots at normal and narrow widths. Verify
roomy themed cards/actions/switches, sidebar-to-picker and two-to-one-column
transitions, scrollable content and consistent oxiced colors. Repeat the existing
keyboard-driven settings/Nix-base regeneration scenario after form changes.

The isolated packaged Hyprland scenario passed with all six control providers and
seven loaded plugins, ten reopen cycles, live preferences and private-bus inbox/
toast checks. Both width variants and the settings regression passed. Screenshots
are private because compositor translucency may reveal desktop content. Workspace
tests and strict Clippy passed; host and all seven library packages built with Nix.

## Native Audio Integration and Runtime Verification

The ignored test in `plugins/audio/src/integration_tests.rs` starts a private
PulseAudio server with null sinks/source, private socket/state/cookie directories
and no hardware modules. It tests native snapshots/actions and stream cancellation
without a host Tokio runtime or changes to the session's audio server. Use the
flake's pinned PulseAudio package to provide `pulseaudio` and `pactl`:

```sh
nix shell --impure --expr '
  let source = builtins.path {
    path = ./.;
    name = "oxibar-verification";
    filter = path: type:
      !(builtins.elem (builtins.baseNameOf path) [".git" ".direnv" "target" "tmp"]);
  };
  flake = builtins.getFlake (builtins.unsafeDiscardStringContext (toString source));
  pkgs = import flake.inputs.nixpkgs { system = builtins.currentSystem; };
  in pkgs.pulseaudio
' -c nix develop -c cargo test -p audio \
  private_server_events_actions_hotplug_reconnect_and_cancellation -- --ignored
```

The fixture verifies typed monitor-source exclusion, balanced stereo volume,
output/input defaults, volume/mute, added/removed sinks, disconnect/offline action
rejection, reconnect without stale-action replay and prompt worker shutdown.
Normal unit tests use pure state helpers or a missing private socket.

For GUI verification, build host/audio/applications/clock with the same pinned
toolchain. Use isolated XDG config/locks/socket, a private PulseAudio server, and a
private session bus without autoactivation. A fixture MPRIS player should expose
metadata, playback and volume controls. Set a long media interval and verify
external device changes still arrive immediately. Place a failing marker `pactl`
on only the native daemon's `PATH` to prove it never invokes the CLI; use an absolute
real `pactl` path with the private server address for fixture assertions.

Exercise actual GUI default-device selection, balanced volume, both mute switches,
media playback/next/previous/volume, device hotplug, offline cached content and
recovery. Delay the fixture media acknowledgement and complete a device action
while it is pending; verify media refresh still runs without waiting for its timer.
Close/reopen the center repeatedly and inspect the compact scrollable
popup. Select `audio.backend = "pactl"`, verify restart classification, restart only
the fixture daemon with the real CLI, and repeat device actions in compatibility mode.

These checks passed against the Nix packages: 171 workspace tests/doctests, strict
workspace Clippy, the opt-in private-server test, host/audio/applications/clock
builds, and the private packaged GUI scenario. Native mode made zero `pactl` calls;
all device/media GUI actions, reconnect and five reopen cycles passed. Audio's
`libpulse.so.0` runtime dependency resolved through its Nix search path. Private
screenshots retain the existing oxiced theme. A separate PipeWire fixture and
profile/stream capabilities remain follow-ups.

## Native BlueZ Integration and Runtime Verification

Two ignored tests in `plugins/bluetooth/src/integration_tests.rs` launch their own
private DBus daemon without service autoactivation and export mock BlueZ interfaces.
They inject the address into the native client, without a host Tokio runtime,
system-bus changes or hardware. Supply `dbus-daemon` from pinned Nixpkgs:

```sh
nix shell --impure --expr '
  let source = builtins.path {
    path = ./.;
    name = "oxibar-verification";
    filter = path: type:
      !(builtins.elem (builtins.baseNameOf path) [".git" ".direnv" "target" "tmp"]);
  };
  flake = builtins.getFlake (builtins.unsafeDiscardStringContext (toString source));
  pkgs = import flake.inputs.nixpkgs { system = builtins.currentSystem; };
  in pkgs.dbus
' -c nix develop -c cargo test -p bluetooth private_bus -- --ignored
```

Coverage includes multiple adapters, partial/invalidated properties, object hotplug,
owned discovery timeout/cleanup while another client scans, power/connect/disconnect/
trust/remove, no normal inventory polling, owner restart/offline rejection and joined
worker shutdown. Agent tests cover every PIN/passkey/display/confirmation/authorization/
service request, invalid/stale responses, rejection, sender validation, local/BlueZ/
Release/timeout cancellation, stream drop and cancellation during Connect.

For packaged GUI checks, build host/Bluetooth/applications/clock with one pinned
toolchain. Use private XDG config/locks/socket and a no-autoactivation private bus;
set only the fixture daemon's `DBUS_SYSTEM_BUS_ADDRESS` to that bus. Export mock
BlueZ with controllable modes/state and a delayed Connect. Place a failing marker
`bluetoothctl` on only its `PATH` to prove native inventory/actions never use it.

Exercise actual GUI PIN/passkey input, numeric confirmation, authorization/service
approval, displayed codes, scoped Escape and settings replacement. Accept a PIN
then cancel during Connect and verify Disconnect. Check owned scan timeout without
stopping another client's discovery, hotplug, cached offline content/recovery and
repeated center reopening. Record agent registrations/default-agent requests and
GetManagedObjects reads: normal events/reopens must not add owners or inventory polls.

The private Rust tests and packaged Hyprland scenario passed. Workspace verification
passed 179 tests/doctests and strict Clippy; Nix built host/Bluetooth/audio/applications/
clock. Five reopen cycles kept one agent, with no default-agent request or
`bluetoothctl` invocation. Two startup snapshots remained the GUI fixture's constant
baseline through normal events; explicit refresh/invalidation/reconnect can resnapshot.
Typed replies, full-flow cancellation, hotplug/reconnect and themed visuals passed.
Fixtures were reaped/removed and screenshots stayed private. Actual hardware and
system permission-policy verification remain manual.

## System Services and OSD Runtime Verification

Run `cargo test -p system private_bus -- --ignored` with pinned `dbus-daemon` on
PATH, using the same filtered-source Nix shell pattern as the BlueZ tests above.
The fixtures create private ordinary backlight files and login1/power-profile
services. They verify independent domains, verified/fresh session UID, ordinary
file fallback, native helper/profile/session requests, owner restart, a slow query
concurrent with file actions and prompt joined shutdown. They never call the real
system bus or change hardware/session state.

For compositor checks use isolated config/socket/lock, private system/session bus,
private backlight files, null PulseAudio sink and MPRIS fixture. Check quiet startup,
relative actions, external events, volume/mute/media OSDs, ID coalescing/expiry,
live edge/size/duration, disabled OSDs and repeated reopen. Delay a brightness reply,
change monitor focus and verify its captured output while a fast status query replies.
Session CLI verbs must open confirmation only; Escape has no effect, and Confirm
calls only the mock interactive method. Reject a freshly foreign session UID.
Test configured locker arguments with a marker-only executable.

Prove input pass-through by typing before and after OSD creation, traversing to a
field and applying it. A screenshot alone is insufficient. The local layershellev
keyboard patch passed this check: duration changed to 900 ms while width stayed 360.
Wait for compositor animations before inspecting settled OSD screenshots.

Current step passed 195 workspace tests/doctests, strict Clippy, both private System
tests and Nix host/System/Audio/applications/clock builds. The full private Hyprland
scenario passed actual actions, keyboard pass-through, source routing, concurrent
status, confirmation, UID rejection, locker launch and five reopen cycles.
Screenshots remain private. With limited storage, use `CARGO_PROFILE_DEV_DEBUG=0`,
`CARGO_PROFILE_TEST_DEBUG=0` and `CARGO_INCREMENTAL=0` consistently in a session-owned
target directory. The patched backend is outside workspace membership; formatting
its changed Rust file uses `rustfmt` explicitly alongside workspace `cargo fmt`.

## Coverage Goals

- Host: config-derived bar placement, plugin message mapping, popup geometry, and modal/panel state transitions where they can run without a GUI runtime.
- Plugins: ABI metadata, model defaults, parser helpers, update-side error draining, and helper functions for external data formats.

## Known Gaps

- Dynamic library loading from `$XDG_CONFIG_HOME/oxibar/plugins/` is not covered by an integration test yet.
- The opt-in runner dylib test exercises real library exports/tasks, but general availability-gate/load-order coverage through `load_plugins()` is still missing.
- Normal tests avoid live system services. Opt-in tests exercise the real runner
  timer, native audio events/actions, private BlueZ state/pairing/cancellation and
  private System domains/actions;
  other service subscriptions and general dylib lifecycle coverage remain incomplete.
- UI rendering tests only validate that view functions return elements for valid models; they do not assert pixel output.
- Keyboard search, text-field traversal/application, category shortcuts and Escape
  are verified; generic checkbox/button keyboard focus and screen-reader support
  remain a shared-control follow-up.
