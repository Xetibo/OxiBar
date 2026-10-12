# Testing

## Authentication state checks

`cargo test -p oxi-auth-ui -p oxi-lock -p oxi-pam -p oxi-session` checks bounded/redacted
response storage, single-flight attempts, stale/cancelled result rejection,
protocol readiness/destructor state and acknowledged greeter handoff.
`oxi-lock` integration tests create a private Wayland socket pair/server and render
actual Iced CPU frames. They check latest configure acknowledgement, non-NULL
opaque SHM pixels and native RGB byte order, integer/fractional geometry, output
hotplug/removal, denied and
external-finish destruction, readiness-before-callback, authenticated final sync,
missing-protocol refusal, malformed configure and crash-without-unlock.
`oxi-pam` tests compile a private PAM module and load it through test-only
`pam_start_confdir`; production always uses the fixed system service. They exercise
the real PAM library, information/error/visible/secret MFA prompts, wrong answers,
account-policy rejection, identity remapping, cancellation/reaping and partial
bounded IPC. No system PAM file or real account password is changed. Run in the
flake development shell for the PAM library/headers and pinned C compiler.
`oxi-session` tests use private Unix socket pairs, fragmented native-endian JSON
frames and multi-prompt conversations. They cover cancelled in-flight replies,
retry identities, malformed/truncated/oversized frames, rejected starts and final
acknowledged handoff. Catalog fixtures verify direct argv/field-code expansion,
hidden/unavailable entries and administrator ownership rules.
Authentication integration must use private fixtures or the dedicated VM, never
the current user's desktop session or credentials.

`nix build .#oxilock .#oxisession` builds independent authentication packages and
runs their selected Cargo tests. `checks.<system>.authentication-unit` runs all
four authentication crates (30 normal tests plus the test-only ignored PAM child
entry); `authentication-clippy` runs strict all-targets Clippy. Bounded appearance
and asset decoding, truthful Caps inference, repeat retirement and widget wake
deadlines have service-free checks. A private p2p logind fixture transfers actual
inhibitor FDs and verifies held-through-readiness leases, live-child reuse and
refusal/timeout gates for explicit suspend. Release assertions allow at most one
second for EOF from asynchronous reply/fork cleanup and still require a live locker.
A non-close-on-exec descriptor regression detects child-owned inhibitor copies;
an exec-failure check preserves spawn error reporting. On x86-64,
`checks.x86_64-linux.authentication-cage` and `authentication-hyprland` boot real
NixOS VMs with disposable credentials, greetd, PAM and real DRM. They assert
failed-auth retention, retry, old greeter/compositor retirement, UID 1000,
Wayland environment isolation, seat0/VT1 ownership, logout and different-PID crash
recovery. Native checks also cover decoded assets, permission-gated power
confirmation/cancellation and restored focus; actual protocol/PAM lockers,
fractional output scaling/hotplug, Caps state, held-key editing, failed-password
retry and parent/worker crash retirement. Cage's VM verifies both real S3 paths,
protocol readiness before delay-lease release, visible locked resume, reacquired
inhibition, watcher-stop child preservation and authenticated unlock after resume.
The sleep fixture uses legacy VGA/pixman and a protected block-backed Nix store;
pinned virtio-GPU fences stall after S3. Hyprland retains virtio-GPU coverage.
Use raw-frame OCR for opaque-card readiness/dialog presence; PAM results and
process/seat state, not OCR or sleeps alone, gate authentication assertions.
Deployment examples are in `AUTH_DEPLOYMENT.md`.

## Strategy

- Unit tests cover host main-loop helpers and every plugin crate.
- Tests avoid live DBus, NetworkManager, Bluetooth, Hyprland, PulseAudio, Wayland, or notification daemon dependencies unless explicitly marked as integration tests.
- External command wrappers use private injectable fixtures where available;
  physical hardware/policy behavior remains a separate manual check.
- Plugin system modules carry parser/helper tests next to the code they own.

## Current Unit Coverage

- SDK/release: actual-completion app handoff validation, owned interval cancellation,
  quiet root-filtered filesystem watches, process-resident code/deferred vtable
  cleanup, shared example views/config/modal state and Network stale-edit replies.

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
  structured socket replies. Navigation tests cover unique schema-to-page coverage,
  nested routes, expansion/removal fallback and retained drafts/surface state through
  saves. Plugin tests cover live hook state preservation.
- Multi-output: stable IDs, independent logical dimensions/profiles, popup/modal
  source routing, output fallback/disable, hotplug cleanup, retained plugin models,
  late task results and unknown-window rejection. Monitor parser tests preserve
  fractional scale/rotation, negative coordinates and precise refresh rates. Pure
  snapping fixtures cover ReSet-style opposing/matching edges, disabled peers,
  deterministic peer order, bounds and overlapping-drop restoration.
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
- Overview: checked shape/stride/format, orientation and premultiplied alpha;
  coalesced mailbox wakes, bounded scheduling, centered modal bounds, Tab cycling
  within a monitor, spatial monitor navigation and scoped configurable shortcuts,
  native retirement-before-activation, stale generations and modal-origin-aware
  clipped cross-monitor hit bounds.
  A normal private Wayland server executes modern SHM capture, any-order constraints,
  resize/stopped/denied sources and exact activation across recycled addresses.
- Dock/calendar opening: qualified/Steam class ranking, lower-priority executable
  fallback despite alternate declared classes, library-path precedence, SVG filename aliases and
  portable format-aware icons; blocked calendar work stays off the UI executor.
  Shared reveal tests keep expensive child layout full-sized throughout animation.
  Actual Iced buttons with the headless CPU renderer cover hidden/partly revealed
  click-through, parent-viewport clipping, visible hover and canceled child presses.
  Drag-preview tests reconcile an empty/previous surface tree and preview insertion/
  removal while preserving the content's widget state.
  Bounds-capture tests invalidate equal geometry when logical widget identity changes.
- Themes: exact legacy seed, immutable/exact-Arc delivery, bounded interning without
  revision growth, alpha/preset/density/motion validation, live source/reset, retained
  startup font and late uploaded-font guards.
- UI/menu refinements: default clock-attached center, configured entries, integrated
  inbox/toast retention, centered session tiles and native confirmation/output handoff,
  disabled capabilities, stale session IDs/menu disablement, stale drag identity/
  revision rejection, sparse default-provider enablement and exact button anchoring.
  Anchor tests cover relayout, duplicate providers, edge clamps and removed slots.
  Live-provider tests retain one model through disable/re-enable, accept pending
  data updates while disabled and reject deferred disabled-provider host requests.
  Multi-output tests cover live global/profile label preferences without replacing
  bar IDs or shared provider state.
- Base16/Base24: complete slot inheritance, derived missing Base16 extensions,
  standalone flat/nested equivalence, validated accent precedence, explicit semantic
  overrides/bright states and legacy seed equality. Catppuccin Mocha, Latte and
  factory-default text roles meet 4.5:1 contrast across their opaque neutral and
  filled-control states. Monitor tests cover precision/negative geometry/diagram transforms,
  invalid/all-disabled profiles, independent rollback leases and failed/stale saves.
- Tools: bounded calculator parsing, all provider prefixes, quoted argument vectors,
  bookmark/action validation, immutable edited actions, opt-out history clearing,
  no clipboard text in identity/usage state and actual failed copy outcomes.
- Provider validation: pinned library lifetime, rejection before adoption/writes,
  longest-prefix cancellation and sparse live favorite changes. Filesystem tests
  cover nonrecursive missing parents, created roots and prompt owned watcher drop.

## Refined UI and Native Display Runtime Verification

`scripts/ui-smoke.py` runs only inside an owned isolated compositor. Supply the host
and Clock/System/Notifications/Audio/Bluetooth/Stats/Applications/Tools DLLs:

```sh
nix develop --impure --file nix/gui-tests.nix -c \
  python scripts/run-isolated.py -- \
  python scripts/ui-smoke.py "$OXIBAR" "$CLOCK" "$SYSTEM" "$NOTIFICATIONS" \
    "$AUDIO" "$BLUETOOTH" "$STATS" "$APPLICATIONS" "$TOOLS"
```

For unwrapped local Cargo binaries/libraries, prefix this command with
`nix develop -c` to retain the flake's dynamic Wayland/graphics library paths.

It checks actual clicked-button geometry, icon-only host/provider bars and live label
width changes, attached clock/left navigation/inbox, centered 800×480 OxiShut-style
session tiles and native confirmation, 1440×960 settings, live default-provider enablement without
restart, indexing/search/activation of an owned desktop-entry fixture and calculator
copy. Three-column pointer drag and its content preview, coherent Base24/accent and
real monitor Preview/Keep/Revert/sparse-save plus bottom-bar placement are covered. The fixture owns
its XDG config/data/state, immutable base symlink, bus, mock logind/power profiles,
MPRIS player, null-device PulseAudio server, virtual pointer and compositor;
it never changes the real desktop/monitor/session. Additional assertions cover the
640-high ordinary Audio dropdown and Escape, idempotent session open/toggle/close,
unchanged settings surface geometry on saves, shared notification/music edges and
spacing with live bottom-left placement, and actual pointer reveal/hide with
dropdown pinning. The hover fixture retains its virtual pointer until replacement;
removing the seat's only pointer emits Leave and must not masquerade as hovering.
The fixture also creates a second owned headless output. It checks nested page
selection, sidebar folding/expansion, reduced motion, the narrow full-path picker,
header reset placement and a compact footer that retains its bounds across saves.
Pointer clicks select every sidebar root and Dock children with normal, reduced and
zero-duration motion, after transitions settle.
Real pointer drags exercise edge snapping and overlap restoration on both outputs
before the existing native preview/Keep/Revert flow. Dedicated session tiles remain;
embedded System content omits duplicate session actions.
Optional screenshots are private. Capture after compositor animation settles,
except the explicit intermediate/final sidebar images used for visual motion review.
Read layer geometry after waiting, so resizing does not crop against an old position.
Use the whole-output narrow-window capture to verify padding after a live resize;
reported layer bounds can differ from the rendered position during repositioning.
Native navigation uses captured child-page card bounds; sidebar bounds collected
during animated layout changes are not a reliable animation timing assertion.
The smoke enables backtraces, retains panic diagnostics and fails if the daemon's
retry loop catches a panic. A restarted daemon cannot satisfy the run.

Installer verification uses private fake-library fixtures: missing Applications or
OxiPaste files must fail before any installation, all thirteen libraries must install with
correct contents/mode, and absolute/relative `CARGO_TARGET_DIR` plus custom
`XDG_CONFIG_HOME` must work. `bash -n scripts/install-plugins.sh` checks syntax.

The native controller's opt-in test also runs against both the isolated Hyprland
session and the parent Sway wlr-output-management session:

```sh
nix develop --impure --file nix/gui-tests.nix -c \
  python scripts/run-isolated.py -- \
  nix develop -c cargo test -p oxibar \
    private_compositor_native_preview_revert_and_shutdown_restore -- --ignored
# Repeat with scripts/run-isolated.py --parent for the native wlr backend.
```

Use the filtered source dev-shell form below while new files are untracked, and
the session target/debug flags when storage is limited. The test requires
`OXIBAR_TEST_ISOLATED=1` set by the coordinator, preserves advertised zero refresh
for virtual heads, and proves fractional-scale preview, Revert and owner-drop
restoration. Pure fixtures additionally prove lease expiry without a GUI loop,
stale confirmations never save and failed persistence immediately rolls back.

The preceding UI-refinement task passed **256 normal tests/doctests**, strict workspace
Clippy, all **14 Nix package outputs**, both native display tests and the full new
UI smoke. The existing all-flow smoke also passed with explicit legacy standalone
control/panel preferences, covering retained CalDAV, password/modal, tray,
notification reply/DND/toast/panel, desktop revision/hotplug and reopen behavior.
The new default presentation is tested separately rather than weakening those
legacy assertions. Physical display links/driver policy and abrupt-kill rollback
are separate known boundaries, not inferred from headless success.

## Expected Commands

- `nix develop -c cargo fmt --all -- --check`
- `nix develop -c env -u HYPRLAND_INSTANCE_SIGNATURE cargo test --workspace`
- `nix develop -c cargo clippy --workspace --all-targets -- -D warnings`
- `nix build .#oxibar .#oxibar-audio .#oxibar-bluetooth .#oxibar-system` after native backend/API
  changes, with more plugin package targets as needed.

For this runner integration, build `oxibar`, `oxibar-applications`, and a retained
bar plugin such as `oxibar-clock`. While new files are still untracked, use the
local-path flake reference so Nix sees them:

```sh
nix develop path:. -c env -u HYPRLAND_INSTANCE_SIGNATURE cargo test --workspace
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
' -c env -u HYPRLAND_INSTANCE_SIGNATURE cargo test --workspace
```

Service-free host fixtures must unset `HYPRLAND_INSTANCE_SIGNATURE`; otherwise
real focused-monitor lookup can override the fake inventory. Native tests use the
owned coordinator's compositor environment instead. Workspace formatting excludes
the vendored backend, so check it separately after editing:

```sh
nix develop -c rustfmt --edition 2024 --check crates/layershellev/src/lib.rs
```

## Window Overview and First-Open Responsiveness

The preceding overview phase passed **268 normal tests/doctests**, 14 explicitly
opt-in tests, strict workspace Clippy and all **14 Nix package outputs**. The modern
protocol fixture is a normal test with its own private socket/server and no actual
desktop dependency. Nix host checks load their declared Wayland runtime before the
final executable wrapper; protocol XML remains included in cleaned sources.

The real native capture test is deliberately opt-in and must run only under the
owned coordinator, never against the user's actual desktop:

```sh
nix develop --impure --file nix/gui-tests.nix -c \
  python scripts/run-isolated.py -- \
  nix develop -c cargo test -p oxibar --lib \
    private_overview_real_pixels_exact_handles_and_owned_cleanup -- --ignored --nocapture
```

It creates two native, identical-title red/white and blue/white windows. Actual
Hyprland 0.51.1 tests overlap them and move one to another workspace, then change
both buffers and require fresh matching preview pixels. It also verifies exact
handle activation, stale identity rejection, storage limits and prompt owner drop.
Modern ext coverage is separately executed by the normal hermetic server, including
formats-before-size, resize, denial and same-address replacement with old handles
still present. A failed nested Hyprland 0.56.2 startup is not modern desktop proof.

Build the owned window-fixture test binary with the same pinned graph, then supply
the packaged host/Applications/Clock libraries to the repeatable GUI smoke:

```sh
FIXTURE="$(nix develop -c cargo test -p oxibar --lib --no-run --message-format=json | \
  jq -r 'select(.reason == "compiler-artifact" and .profile.test and .target.kind == ["lib"]) | .executable')"
nix develop --impure --file nix/gui-tests.nix -c \
  python scripts/run-isolated.py -- \
  python scripts/overview-smoke.py "$HOST" "$FIXTURE" "$APPS" "$CLOCK"
```

Use the filtered-source form while files are untracked and the session target/debug
flags when storage is limited. The script owns both monitor/window fixtures and
virtual pointer, captures proof pixels in memory, verifies grouping/keyboard and
centered 1400×900 modal geometry, Tab staying within a monitor, spatial h/l navigation
and live Ctrl+h remapping. Cross-monitor mouse activation after native grab retirement,
changed source pixels, hotplug and ten reopen cycles are covered. A real green SVG dock icon and portable missing-icon
fallback are checked too. Every closed cycle has zero capture workers, empty
windows/staging/cache and stable thread count. Optional screenshots stay private.
Applications is deliberately absent from the private XDG plugin directory; an
`OXIBAR_PLUGIN_DIR` fallback supplies its pinned library for the real SVG-icon proof.

Actual presentation and closed ownership have a separate conforming-clock reference:

```sh
nix develop --impure --file nix/gui-tests.nix -c \
  python scripts/run-isolated.py --parent -- \
  python scripts/overview-performance.py "$HOST" "$FIXTURE" \
    --output /tmp/oxibar-overview-performance.json
nix develop --impure --file nix/gui-tests.nix -c \
  python scripts/run-isolated.py --parent -- \
  python scripts/control-center-performance.py "$HOST" "$CLOCK" \
    --output /tmp/oxibar-control-center-performance.json
```

Sway 1.11 advertises `CLOCK_MONOTONIC` but does not support individual-window capture
through these paths. The two-window overview benchmark explicitly reports that
limitation; it measures the UI and closed resource release, not captured-content
latency. Thirty opens pass first presentation **31.73 ms**, warm p95 **37.25 ms**,
closed CPU **0.40% of one core**, zero closed draws/capture workers, **+138 KiB**
median reopen RSS and no worker growth. The real-pixel Hyprland run separately has
zero closed capture workers and stable thread count (64 in the final ten-cycle run).

Control-center first-open measurement blocks a private CalDAV curl fixture for two
seconds. First actual bar presentation is **31.36 ms**, warm p95 **4.94 ms** and
concurrent status **3.85 ms**, all below 50 ms while sync remains in flight. Stable
reveal layout and off-executor calendar jobs are measured directly; no old-binary
percentage improvement is inferred. Both complete retained UI/legacy smoke suites
also pass. Exact JSON records and counter definitions are in `RELEASE_CHECKS.md`.

The subsequent shortcuts/provider/presentation refinement passed the full normal
workspace tests/doctests, formatting and strict all-target workspace Clippy. After
real GUI checks exposed a drag-preview tree transition and an unbounded-scroll
height mistake, focused host/overview/settings/theme flows and the shared-widget
regression passed again. All fourteen Nix outputs built from the pinned graph;
the final host was rebuilt after those host-only presentation corrections and the
complete refined UI smoke passed with the verified provider libraries. The real-pixel
overview smoke also passed with centered modal navigation and live key remapping.

The same blocked-sync control-center workload measures **31.25 ms** first actual
presentation, **6.38 ms** warm p95 and **4.77 ms** concurrent status, all below 50 ms.
The prior reference above is retained; the additional record is
[`control-center-refinements-reference.json`](control-center-refinements-reference.json).

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

## Persistent Clipboard and Native Paste

`nix develop -c env -u HYPRLAND_INSTANCE_SIGNATURE cargo test -p oxibar --test oxipaste_flow`
checks command errors, idempotence/toggle, native layer settings, output bounds,
shared visible-only subscriptions, exclusive-overlay dismissal, captured/scoped
Escape, outside bounds, native closure, hidden bars, output removal, provider
disable/re-enable and retained late results without a live desktop service.
IPC parser tests validate all three clipboard window verbs and reject invalid actions.

Build the updated OxiPaste binaries separately, and build Oxibar with its OxiPaste
dylib from one workspace graph. The following opt-in checks require owned services:

```sh
nix develop -c nix develop --impure --file nix/gui-tests.nix -c \
  dbus-run-session --config-file=scripts/private-bus.conf -- \
  env OXIBAR_TEST_PRIVATE_BUS=1 \
      OXIBAR_OXIPASTE_PLUGIN="$PWD/target/debug/liboxipaste.so" \
  cargo test -p oxipaste private_bus -- --ignored
# In another identical private-bus invocation:
# cargo test -p oxibar --test clipboard_dylib -- --ignored

nix develop -c nix develop --impure --file nix/gui-tests.nix -c \
  python scripts/run-isolated.py -- \
  python scripts/clipboard-smoke.py \
    "$PWD/target/debug/oxibar" "$PWD/target/debug/liboxipaste.so" \
    /path/to/OxiPaste/target/debug
```

Use the filtered working-source shell in Expected Commands while source files are
untracked. The outer development shell supplies graphics library paths for
Cargo-built GUIs. The private D-Bus fixture checks delayed actual write completion,
byte-exact payload identity and failed-write propagation; the real dylib fixture
checks tasks and visible-only timers without a host Tokio reactor.

The smoke owns HOME/XDG directories, bus, compositor, daemon, host, standalone
picker and raw terminal receiver. It compares receiver bytes for both Ctrl+V and
Ctrl+Shift+V after native focus release; screenshots are optional diagnostics.
It also proves original watcher stdin/trailing newlines, exact Copy after history
reordering, Copy retaining the picker without typing, failed writes retaining
picker/history, keyboard-device removal without Leave, standalone Copy/Enter,
shell-command open/close/toggle/idempotence and centered placement, automatic
search focus, mutual launcher dismissal, Copy retention and native paste with the
bar disabled, provider-disable cleanup/errors, binary MIME bytes, private file
permissions and restart persistence. The same
suite accepts Nix host/plugin/OxiPaste package paths; normal writes use the
packaged helper wrappers, with the unwrapped daemon isolated for failed-write
injection. `--trace-focus` records native keyboard/focus transitions.
Never run these keyboard/clipboard checks against the user's session.

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
inventory/geometry/hotplug on the running compositor. The later release benchmark
also runs generic Wayland startup/presentation under an owned Sway session; Niri
runtime remains untested. Existing settings/Nix-override smoke and the opt-in real
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

## Final SDK, Lifecycle and Release Verification

The original roadmap release passed **243 normal tests/doctests**, with eleven explicitly
opt-in tests, plus strict Clippy for every workspace target. All fourteen Nix
outputs built with shared Crane artifacts. Actual Nix example dylib checks ran
25 model/task/view/theme/config/modal/owned-stream cycles without host Tokio TLS.
The actual loader verifies metadata does not initialize models, availability
skips precede factories and duplicate names initialize once.

The private user-manager test verifies transient unit properties, actual job
completion, caller exit while its app remains alive, and no duplicate execution
after uncertain accepted submission. It uses only a private bus/service/child:

```sh
nix develop --impure --file nix/gui-tests.nix -c \
  dbus-run-session --config-file=scripts/private-bus.conf -- \
  nix develop -c env OXIBAR_TEST_PRIVATE_BUS=1 cargo test -p oxibar-plugin-api \
  private_user_manager_handoff_survives_caller_exit -- --ignored
```

`RELEASE_CHECKS.md` supplies real loader, example, module-evaluation and compositor
commands. `nix/hm-test.nix` verifies startup defaults to no user unit, deliberate
session-target ownership, disabled installed catalog packages, enabled-only base
generation and unmanaged GUI overrides. It also evaluates the default
`nixosModules.home-manager` export, checking that the clipboard provider is enabled,
present in `home.packages` and linked under the configured XDG directory. An
explicit enabled list still replaces the defaults. Use a Home Manager release
aligned with the pinned nixpkgs; no activation is involved.

Repository tooling owns its private runtime, bus and compositor:

- `scripts/run-isolated.py`: headless Sway/private nested Hyprland; `--parent`
  selects Sway for a conforming presentation clock.
- `scripts/smoke.py`, `fixtures.py`, `virtual-pointer.c`: catalog, confirmation
  Escape/replacement, actual live field application while an OSD is visible,
  launcher handoff, CalDAV/day fixture, secure network password save, native tray
  menu, notification toast/panel/inline reply/DND, two-instance drag/resize,
  stale-save retention, hotplug and ten control-center reopen cycles.
- `scripts/performance.py`: actual first `wp_presentation.Presented` feedback,
  native retirement, accepted query latency, idle CPU/draws, all-thread voluntary
  context-switch wakeup proxy and reopen RSS/worker samples.

The strengthened complete smoke passed against final Nix packages. Private fixture
shims and a private SNI server never touch real NetworkManager/HTTPS calendars;
all test children/compositors/files are reaped. Screenshots remain private.

The named 2,000-entry Sway reference passed all six gates: actual warm frame p95
20.83 ms, query max 0.83 ms, idle 0.30% of one core/zero redraws in ten seconds,
+130 KiB median reopen RSS and thread count 66 → 64. See `RELEASE_CHECKS.md` and
`performance-reference.json` for exact workload, counters and the 2 MiB RSS noise
budget. Configure/IPC timestamps are not presentation substitutes. Nested Hyprland
0.51.1's missing clock event/stale feedback was rejected for benchmark use.

Profiling uncovered watcher feedback from redundant registration and unrelated
missing-root sibling events. Real filesystem regressions now assert both quiet
idle and correct create/update/recovery behavior. Optional presentation tracing is
disabled in normal runtime.

## Coverage Goals

- Host: config-derived bar placement, plugin message mapping, popup geometry, and modal/panel state transitions where they can run without a GUI runtime.
- Plugins: ABI metadata, model defaults, parser helpers, update-side error draining, and helper functions for external data formats.

## Known Gaps

- Normal tests avoid live system services. Opt-in tests exercise the real runner
  timer, native audio events/actions, private BlueZ state/pairing/cancellation and
  private System domains/actions, modern/compatible clipboard, real example and
  loader lifecycle plus private user-manager handoff. Additional native hardware,
  system policy and compositor-specific capabilities remain manual/extension work.
- Ordinary component tests validate constructed elements rather than full application
  golden images. Native overview tests additionally inspect real RGBA buffers and
  private compositor RGB output; broader full-surface visual snapshots remain manual.
- Keyboard search, text-field traversal/application, category shortcuts and Escape
  are verified; generic checkbox/button keyboard focus and screen-reader support
  remain a shared-control follow-up.
