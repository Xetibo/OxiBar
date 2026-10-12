# Architecture

## Authentication applications

Authentication is isolated from the shell and its dynamically loaded plugins.
`crates/oxi-auth-ui` owns bounded prompt/attempt state and masked themed views;
`crates/oxi-lock` owns the Wayland lock lifecycle; `crates/oxi-pam` owns current-user
PAM in a private cancellable subprocess; `crates/oxi-session` owns acknowledged
greeter handoff.
The backend selection and privilege boundaries are recorded in
`AUTH_BACKEND_AUDIT.md`. Lock readiness follows the protocol `locked` event,
and greeter exit follows an authenticated, acknowledged greetd start request.

The locker owns its Wayland event queue rather than reusing generic window Close
actions. Each registered output has a lock role, configure/ack state, two
release-aware opaque SHM buffers and an Iced CPU-renderer/cache. Integer scaling
and fractional-scale/viewport destinations preserve exact surface-local geometry.
Output removal retires its input focus and resources. Input uses per-seat contexts,
rejects retired devices and supplies no clipboard service. Authentication is an
application capability; only its explicit success action can request unlock.
Readiness follows `locked`, and successful/external unlock is followed by display
sync before returning. Renderer errors and process loss never send unlock.

The locker requires an administrator-owned dedicated PAM service before acquiring
a lock, preventing fallback to another service's policy. Its same-image worker
receives only a private same-UID socket; cancellation kills and reaps its process
group. Bounded zeroized frames carry attempt IDs, multiple prompt types and final
authentication/account-policy results. Secret text stays in preallocated zeroizing
storage; ordinary Iced messages/widgets receive masks instead of password strings.
The PAM conversation owns native response allocations until PAM consumes them.
Both processes disable core dumps and debugger attachment before handling input.

Authentication appearance is read-only and independent of shell configuration:
system-owned greeter settings/assets and per-user locker settings feed shared theme
tokens, bounded image decoding and opaque fallback composition. Native seat state
supplies locker Caps/layout indicators. Repeat retains physical key identity, not
text, and is retired with focus/devices; per-output widget wake deadlines preserve
idle rendering. The CPU backend explicitly converts Iced BGRA to native XRGB.

The separate sleep owner subscribes to logind before taking a delay inhibitor.
Its private child pipe opens the readiness gate only for a complete protocol-ready
token from a live locker. Resume reacquires the lease and reuses a live locked child;
watcher shutdown leaves authentication available. Explicit lock-then-suspend holds
a block inhibitor through readiness and cannot call Suspend on refusal/timeout.
Delay leases remain bounded by logind policy and report failed readiness.

The greeter is a standard fullscreen xdg-shell Iced application, independent of
layer-shell and the locker. Its root-peer-validated greetd client serializes one
bounded request at a time, drains cancelled replies before retry, and holds response
serialization in zeroized storage. A system-only desktop catalog resolves trusted
executables to argument arrays and explicit XDG environment values. Authentication,
session selection and start acknowledgement remain separate states; only accepted
start permits normal frontend exit. The surrounding kiosk owner then releases the
seat before greetd starts the authenticated user's session.

Authentication packages use a separate Cargo artifact graph from the dynamic shell
plugins. The kiosk supervisor owns a private compositor process group and runtime
directory; an acknowledged start must be followed by successful compositor teardown
before it returns success to greetd. Instance-specific exit binds both signature
and owned PID. NixOS supplies root PAM/greetd policy and the greeter account; Home
Manager only configures the per-user application. Real VM tests verify that both
kiosk processes retire before a new UID acquires the active seat/VT.

Oxibar is a Rust workspace containing the host bar, a shared plugin API crate, and dynamic plugin crates.

## Workspace Structure

- `src/main.rs`: module wiring and binary entry point.
- `src/app.rs`: iced layershell daemon setup, main application state, update loop, subscriptions, and popup/modal/panel state transitions.
- `src/layout.rs`: geometry constants, layershell settings, bar dimension config/fallback parsing, popup metrics, popup config parsing, and bar section math.
- `src/monitor/`: normalized output inventory and validated display configuration.
  Native Hyprland IPC and wlr-output-management adapters run on one owned controller;
  geometry/drafts, pure logical edge snapping, the layout diagram and preview/save/rollback
  remain separate. Drag snapping does not modify exact numeric edits or backend policy.
  Generic wl_output/xdg_output inventory and focused-output capabilities remain distinct.
- `src/shell/`: explicit surface registry, per-output bar instances, source-window
  routing, hotplug reconciliation, input regions and auxiliary surface ownership.
- `src/config/bars.rs`: common/per-output bar profile merging and validation.
- `src/config/toasts.rs`: validated shared transient placement and layer-shell
  anchors/margins, independent of provider content and output selection.
- `src/config/menus.rs`: attached control/inbox and standalone session preferences, with output-bounded
  popup headroom, separate from visible bar height and its exclusive zone.
- `src/config/desktop.rs`: typed dock/widget layout, stable instance validation and
  logical-coordinate persistence.
- `src/compositor.rs`: one cached, event-driven window/workspace broker and bounded,
  version-aware Hyprland activation adapter.
- `src/dock.rs`: per-output dock composition from the resident application catalog
  and shared compositor snapshot.
- `src/overview/`: host-owned per-monitor grids, visible-only native toplevel
  capture, checked SHM pixel conversion and exact-handle activation. Protocol
  bindings and their pinned upstream XML remain separate from UI state.
- `src/desktop.rs`: desktop widget surfaces, revision-bound edit drafts, grid
  geometry, input policy and output-removal fallback.
- `src/single_instance.rs`: config-driven single-instance lock (`[instance]` table), flock acquisition, and the RAII `SingleInstanceGuard`.
- `src/surfaces.rs`: bar, popup, modal, and panel view construction.
- `src/messages.rs`: host message enum, layer-shell action conversion, and plugin message routing.
- `src/font.rs`: bar font config, fontconfig resolution, and font byte loading.
- `src/theme.rs`: immutable live theme delivery, bounded font loading and
  late-result guards; host renderer and upgraded providers receive the same snapshot.
- `src/config/`: source-aware snapshots, typed host/plugin schemas, recursive
  base/GUI merging, atomic override persistence, revision checks and filesystem
  watching. Startup/instance compatibility helpers remain in `mod.rs`.
- `src/settings/`: settings state/actions, a pure schema-to-page navigation tree in
  `navigation.rs`, spacious schema-driven views in `view.rs`, and host-side
  command/application logic in `runtime.rs`. Stable routes and expansion state
  survive live snapshot changes independently of drafts and persistence; every
  descriptor has one page, with dedicated content for structured editors.
- `src/control_center.rs`: host-owned control-center state, capability selection,
  left navigation and window-scoped keyboard routing. Attached mode reuses a bar ID;
  optional standalone mode owns its own overlay ID.
- `src/session_menu.rs`: centered host-owned session surface, configured cached
  action tiles and keyboard navigation, delegated through the provider's ordinary
  command/confirmation flow.
- `src/shortcuts.rs`: validated live key bindings, context conflict checks and one
  surface-scoped keyboard dispatcher for all host-owned interactions.
- `src/osd.rs`: output-scoped, non-interactive OSD instances and abortable lifetimes.
- `src/commands.rs`: optional resident provider commands, authoritative completion
  replies and captured-output routing through asynchronous results.
- `src/background.rs`: runtime-independent bridge for serialized filesystem jobs
  into Iced tasks; rendering uses immutable snapshots without locking the store.
- `src/launcher.rs`: resident search-overlay state, cached global result ranking, keyboard routing, and themed view.
- `src/ipc.rs`: bounded, versioned Unix-socket commands for launcher, session, settings,
  control center, output inventory, configuration queries/mutations, provider
  actions, overview, OSDs and status. A bounded, owned set of connection handlers
  prevents slow actions from blocking fast launcher/status requests.
  CLI invocations exit before daemon/font/renderer initialization; an active-endpoint record keeps clients
  connected while socket-setting changes await restart.
- `src/plugins.rs`: dynamic library discovery, ABI symbol resolution, optional plugin availability checks, plugin model initialization, and safe wrappers for plugin view/update/error calls.
- `src/plugins/runtime.rs`: serialized background loading, retained inactive models,
  live consumer reconciliation and file-identity-cached catalog metadata.
- `crates/oxibar-plugin-api`: shared ABI types used by host and plugins.
- SDK runner utilities own latest-only cancellable workers, private versioned usage
  state and debounced filesystem watchers with nonrecursive missing-root fallbacks.
- `crates/oxiced`: local shared theme/widget library imported from the previously
  pinned upstream revision; host/plugins use one workspace dependency and lockfile.
- `plugins/*`: dynamic crates from `$XDG_CONFIG_HOME/oxibar/plugins/`, loaded when
  their filename appears in either enabled plugin list.
- `plugins/*/src/system.rs`: command/DBus/sysfs integration, parsing, and domain
  data extracted from plugin ABI/UI files. Audio additionally separates its native
  PulseAudio owner in `native.rs` and runtime-independent job deadlines in `worker.rs`.
- Bluetooth separates cached DTO reduction in `system.rs`, native DBus ownership
  in `native.rs`, the pairing agent in `agent.rs`, model transitions in `runtime.rs`
  and themed content in `views.rs`.
- `plugins/system/`: one native system-bus owner with independent backlight,
  power-profile and session domains; DTO/file helpers, model transitions and themed
  views remain separate modules.
- `plugins/stats/`: bounded proc-file snapshots and a visible-only, cancellable
  refresh stream shared by all displays.
- `plugins/tools/`: pure typed configuration/calculator/search, cached PATH/file
  indexes and one owned native data-control clipboard connection.
- `crates/layershellev`: the published 0.17.1 backend with a local keyboard-focus
  correction. Compositor keyboard Enter/Leave owns keyboard routing independently
  of newly created or pointer-active surfaces. Seat capability updates retain
  existing resources, release removed devices and ignore retired keyboard events.
  Pending repeats are canceled on focus/device removal and are installed only
  while their keyboard and repeating key still exist.
- `plugins/clock/src/caldav.rs`: HTTPS-only CalDAV sync, current-month calendar queries, minimal iCalendar parsing, common recurrence expansion, and Nextcloud-compatible calendar query support.
- Clock calendar refresh/launch jobs run outside the Iced executor through its
  runtime-independent worker; curl connection and total waits are bounded.
- `flake.nix` and `nix/*.nix`: Nix packaging for the host and dynamic plugin libraries. Crane builds one workspace dependency artifact derivation and reuses it for every host/plugin package so shared Rust dependencies are not rebuilt per plugin.

## Runtime Flow

- `main()` configures tracing, layershell settings, font loading, theme, subscription, update, and view callbacks.
- `run()` takes an optional single-instance lock via `single_instance::acquire()` from the `[instance]` config table; when another oxibar holds the lock the new process prints an error and exits non-zero (`allow_multiple_instances = true` skips the lock). The guard is held for the process lifetime.
- `run()` wraps startup in a retry loop driven by `[startup]` config (see `DECISIONS.md`). Tracing is initialized once before the loop. A cheap `wayland_client::Connection::connect_to_env()` probe skips attempts while the compositor socket is not yet accepting clients, and the daemon `run_bar()` is wrapped in `catch_unwind` so the layershell `Cannot create layershell` panic is converted into a backoff-and-retry instead of an immediate exit. Retries use exponential backoff (double the delay, capped at `max_delay_ms`).
- The daemon starts in background mode and creates every bar with a known ID.
  Output selection retains one selected output by default or creates one bar per
  selected connected output. Automatic width uses layer-shell anchors and size
  `0`; configured sizes update only the registered bar for that ID.
- Optional edge reveal retains that native geometry and provider state. Hidden bars
  expose only an output-local edge input strip and reserve no desktop space;
  pointer presence or an owned open dropdown reveals the bar. Visibility, input
  regions and reservations update without retiring the registered surface.
- All view routing is through registered surface kinds. Unknown/retired window
  events do not become bars. Output hotplug reconciles surface instances without
  rebuilding plugin models or subscriptions.
- `OxiBar::new()` loads the deduplicated union of `plugins` and
  `launcher.default_plugins`, then reads start/center/end bar placement. Installed
  but disabled providers are still inspected without initializing models.
  Library discovery prefers the user's XDG plugin directory, then colon-separated
  `OXIBAR_PLUGIN_DIR` fallbacks. The Nix host bundles its pinned Applications library
  through that fallback; this does not implicitly enable it.
- Plugins are keyed by their declared `name()` string, not by filesystem path or load index. Fallback layout order follows config plugin order.
- Plugin emissions retain their provider identity before host-request translation.
  Active providers use the same routing for views, streams and initialization tasks;
  inactive providers may finish model updates but cannot emit new host actions.
- The host owns popup, modal, panel, toast and control-center layer-shell surfaces.
  Plugins render only inner content for those surfaces. Popup open/close reveals
  use the shared theme duration and a fixed full-size child layout, avoiding repeated
  calendar/form relayout during animation. Reduced motion or zero duration bypasses
  animation builders rather than evaluating a zero-duration transition.
  Shared reveals clip child pointer input and hover to the same visible bounds and
  parent viewport as drawing, despite retaining the full child layout.
- Modal retirement invokes an optional provider `modal_closed` hook once for that
  instance, including Escape, replacement, compositor closure and output removal.
  Nonblocking cleanup tasks still update the model after their window disappears.
  Modal Escape and text-field traversal are scoped to the current modal ID.
- The host also owns a keyboard-focused launcher overlay. Opening it reuses the resident renderer and plugin models; closing or activating a result removes only that surface. Compositor bindings invoke `oxibar launcher` over a runtime Unix socket.
- Optional runner providers return bounded result data with stable provider-local launch indexes. The host caches a globally ranked list on state updates and dispatches activation only to the selected provider's existing `launch` hook.
- Per-output bars share one plugin model/subscription per plugin. Optional
  context-aware views receive output name, native scale and an instance identifier;
  `bar_view` also receives the output-local label preference and takes precedence
  over `view_with_context` and ordinary `view`. Existing context/signatures remain
  unchanged. UI/task messages retain their source
  bar for popup/modal/panel placement. Late task results still update shared models
  after their source surface disappears.
- Popups are visible/input-active only on their owning bar. Modal/panel/launcher/
  settings/control-center/session/toast surfaces carry output ownership; output removal
   dismisses their surfaces while preserving plugin data. Notification toasts and
   media feedback share a configured per-output stack, with live anchor/margin
   changes and restacking after notification resize or retirement.
- The control center composes one selected provider's existing shared model. It
  prefers optional `control_center_view`, then falls back to popup/panel content;
  the host owns scrolling, cards, selection and output placement. Configured order
  is case-insensitive and deduplicated, and unsupported/unavailable providers are
  omitted. Opening/closing a center never initializes another plugin or service.
- By default the clock and Controls button open that center inside the owning bar,
   with fixed left navigation and independently scrolling long detail content.
   Bounded stretch content instead receives the available height without an
   unbounded scroll ancestor. The clock
  selects the configured calendar entry. Notification-center requests likewise use
  the bar inbox by default; a separate panel remains an explicit fallback.
- Attached menus never register or remove a second surface for their bar ID.
  Every dropdown temporarily requests exclusive keyboard interaction for that bar;
  scoped Escape remains effective even when an inner widget captured the event.
  Dismissal restores the bar policy without
  deleting the bar or provider data. Session instead owns a centered overlay with
  output-bounded action tiles. Its window scopes navigation and retirement; selection
  captures output/owner before removing that surface and invokes cached provider
  commands. Destructive commands still open the existing confirmation on that output.
- Actual child layout rectangles and pressed slot identities anchor dropdowns.
  Rendering, input regions and outside dismissal share one clamped offset. Layout
  changes prune stale section/index/provider keys before reusing cached bounds.
  Capture wrappers include logical identity so replacement controls/windows republish
  bounds even when their recycled widget tree has identical geometry.
- Settings, launcher and control center dismiss each other's surfaces. Plugin
  task results preserve source routing for password/pairing modals. Hiding a bar
  on a still-connected output detaches ownership and retains open auxiliary
  standalone surfaces; attached menus close with their bar. Viewing the notification
  inbox dismisses visible toasts and suppresses new ones while retaining records.
- Settings writes are serialized behind the config store on background workers.
  Validated sparse overrides are written with owner-only permissions using a
  temporary file, file sync, atomic rename and directory sync. Base/Nix files are
  read-only inputs. External writes and symlink replacement reload snapshots;
  invalid edits retain the last valid state and surface diagnostics.
  Saving/reloading preserves the settings surface and form geometry; bounded status
  space and stable busy-control trees prevent footer feedback from resizing the body.
- Bar-layout dragging carries its source section/index/name and starting revision.
  A valid drop atomically replaces all three layout arrays, preserving other sparse
  fields; stale gestures cannot reorder a newer configuration.
- Bar height/width, transparency, labels, layout and launcher settings apply live through
  layer actions and explicit input-region refresh. Effective-value and override
  provenance changes both affect layout: explicit empty GUI arrays suppress the
  legacy all-empty fallback. Control-center geometry/order/enablement also apply
  live. Settings, launcher and control-center focus are window-scoped.
- Provider selection applies live. A single background loader initializes missing
  models, registers optional schemas/validators and reconciles against the latest
  allowed union. Disable closes consumers/cancels subscriptions and retains the
  model/code; re-enable reuses state and reapplies current config/theme/compositor
  snapshots. Queued config changes cannot create duplicate models or revive a
  disabled provider through a stale completion. Library replacement, legacy bar
  fonts and unsupported backend timer changes remain restart-only.
- Live palette, font, density and motion preferences are a separate supported
  theme capability. Configuration validation retains provider library ownership
  while pure callbacks run on background transactions or external reloads.
- Visible popup size comes from `[bar.popup_sizes]`, then optional runtime `popup_metrics(model)`, then optional plugin metadata, then host default size. Plugins can also request a larger transparent popup input region through runtime metrics or metadata for detached overlays such as tray context menus.

## Dock and Desktop Ownership

The dock reads the Applications provider's immutable `Arc` catalog; it does not
scan desktop entries or create another provider. Desktop IDs map to persistent
provider-local launch IDs across reindexing. Rendered activations revalidate both
IDs, so removal/reordering cannot launch a different application.

Catalog icons render through the shared SVG/raster-aware helper, with a portable
application icon when none resolves. Automatic icon-theme detection, inherited
themes, filename aliases and Nix data paths feed the same resident index. Running
window matches rank explicit class/desktop identity above generic executable matches;
Steam URI-derived classes keep a game's icon distinct from the shared Steam launcher.
Executable fallback is still considered below explicit identities when desktop
entries declare another window class; equally ranked matches prefer a resolved icon.

One host compositor broker subscribes before taking its initial snapshot, coalesces
window/workspace events, bounds IPC/event data and reconnects with fresh identities.
Opaque window IDs include a monotonic incarnation, rather than exposing a reusable
native address as UI identity. Observed close/reconnect invalidates it; activation
checks the current snapshot and freshly verifies the native address/PID. Legacy and
Lua dispatch are version-specific. Workspaces consumes this same snapshot and has
no duplicate event listener or font-probing thread. Other compositors can still
launch pinned applications when running-window activation is unavailable.

Desktop widget IDs identify persistent instances, not provider models. Each instance
receives its output/scale context while reusing one provider model. Requested
output-relative logical geometry survives hotplug; only rendered fallback geometry
is clamped. Normal background surfaces cover their own bounds and ignore other
clients' reserved bar space. Passive providers use empty input regions; an optional
per-instance override can enable/disable input. The editor alone covers the whole
output with interactive previews and captured press coordinates for drag/resize.

Editing keeps a starting revision and raw tables. Save atomically changes only the
widget array, retaining unknown fields; conflicts preserve the dirty draft. Cancel
restores the current effective configuration. Add/remove/hide, output assignment,
grid snap and reset do not initialize another service or overwrite the Nix base.

Legacy subscriptions remain resident. Pure visual providers can opt into
`WhenVisible`: one owned stream runs while any bar/widget/editor/control-center/
popup/modal/panel/toast/OSD/catalog consumer is visible, then cancels without
unloading the model. Clock/CalDAV and Stats resume immediately using cancellable
timers; repeated hidden/resume cycles do not leave detached polling threads.
Exclusive overlays dismiss on ID-scoped outside clicks using actual configured
bounds; this releases compositor input grabs without inventing pointer routing or
replaying an application click.

## Window Overview Ownership

One overview model creates a centered size-bounded modal per enabled named monitor,
defaulting to at most 1400×900 and 90% of that output's logical dimensions, with
only the active surface requesting keyboard focus. Grids receive native toplevel
metadata and memory-only image handles from one visible-owned Wayland thread.
The existing compositor broker remains the cached metadata source; capture never
starts a window scanner or service for each card/output.

The legacy Hyprland path captures the exact wlr foreign-toplevel handle with the v2
export protocol. The modern ext source/copy path uses Hyprland's full 64-bit mapping
protocol to bridge an exact ext handle to an exact wlr activation/output handle.
That association is made once and cannot rebind an old ext window after address
reuse. Titles/app IDs and truncated addresses are never identity bridges. Unsupported
capture capabilities produce a visible diagnostic while available native activation
can remain usable.

The worker owns every proxy, wake socket, memfd, mapping and frame. It validates
shape/stride/format, handles transforms, inversion and premultiplied alpha, and
limits staging to 64 MiB, thumbnail image data to 96 MiB, windows to 256 and jobs
to two globally/one per window. Update cadence is configurable up to 8 Hz; modern
frames honor compositor readiness/damage, while legacy export requests fresh
frames at that bounded cadence. Constraints may arrive in any order. Frame object
IDs reject late results, and denial/stopped/closed sources clear their pixels.
Dropping the subscription wakes and joins its worker; closed overview owns no
capture session, pixel cache or capture timer.

Rendered cards retain their current native identity. Configurable Tab bindings
cycle windows within the active monitor; h/j/k/l chooses spatial monitor neighbors
using output geometry. Grid navigation scrolls the selected card. Actual modal
origins and visible card bounds
support cross-monitor clicks redirected by an exclusive layer-shell grab without
guessing another surface's layout or activating clipped cards. Activation waits for
every overview native surface's Closed event before invoking the exact toplevel/seat.
The generation-scoped retirement deadline is abortable; closing/reopening cannot
activate another incarnation or leave a delayed timer behind.

`WINDOW_OVERVIEW_PLAN.md` records implemented capabilities, verified protocol paths
and remaining platform extensions. Real private-compositor tests prove changed
occluded/other-workspace pixels and exact activation; a hermetic modern Wayland
server proves SHM, constraint ordering, resize/denial and address-reuse behavior.

## Live Theme and Runner Ownership

`oxiced::theme::snapshot::ThemeSnapshot` carries immutable tokens, font and motion
metadata. The host explicitly delivers the same `Arc` to every optional
`theme_changed` hook. Bundled providers install that snapshot into their synchronized
library-local handle; missing hooks retain the legacy seed. Built-in Catppuccin
defaults use readable text and neutral surfaces; explicit custom seed roles remain
authoritative. Presets and GUI
overrides derive new snapshots rather than mutating shared token objects.

Snapshots carry a complete Base24 scheme (`base00` through `base17`) plus an optional
accent. Partial schemes inherit the seed; Base16-only inputs derive missing extra
slots from their base colors. Semantic palette overrides follow scheme
mapping and the accent override is final. Standalone oxiced files accept flat or
nested slots, while the host uses `theme.base24`. Preserving explicit seed-role
overrides keeps legacy theme files exact; a new host scheme replaces that lower
scheme mapping. Bright states use the extra six slots, not an invented second
settings palette. Pure contrast helpers make automatic normal/muted and filled
text readable across opaque idle/hover/pressed backgrounds without changing raw
scheme slots or explicit role overrides. Forms and menus share mantle chrome, neutral `primary_bg` cards,
flat navigation and embedded scrollbar gutters with the bar/launcher theme.

Legacy field access remains safe through bounded immutable palette/font interning
(128 distinct values per library). Previously returned references stay valid;
revision/motion-only changes reuse values. Validation refuses exhausted capacity.
Shared text/input/picker helpers use the active font. Native font lookup/read is
bounded; bytes are uploaded before switching to the canonical family, and stale
font requests cannot replace newer colors or families.

Runner providers advertise optional prefixes, status, cancellation and favorites.
The host selects the longest matching prefix and cancels inactive scopes; providers
parse their own query. Rendered rows and input submission capture provider/local
identity, not ranked position. Favorite changes go through the revision-checked
config store as a single sparse declared field. Pure provider validators retain
their library `Arc` and run before candidate adoption or persistence.

Applications and Tools each own one index worker and one query worker, with one
running and one latest pending job. Cancellation is cooperative and results are
generation-gated. Ranking keeps bounded top results, combines usage/favorites, and
does not clone whole indexes per query. Owned filesystem watchers debounce changes;
missing roots watch an existing parent nonrecursively until the real root appears.

Applications supports localized desktop fields, separate stable desktop actions,
icon-theme inheritance and a dock catalog that excludes action rows. Tools supplies
argument-list commands, bounded calculator evaluation, files, bookmarks and named
actions. Immutable action contents participate in identity so edits/reindexing
cannot redirect an old displayed item. Usage is versioned owner-only state under
XDG state, loaded before local counts are persisted; invalid metadata is retained.

Clipboard history is opt-in, ephemeral and bounded. One native worker prefers
ext-data-control with wlr compatibility and owns polling, wake sockets, pipe FDs,
deadlines and shutdown. Copy works without history capture and acknowledges actual
selection ownership. Capture/selection epochs prevent old reads surviving opt-out;
history identities remain monotonic across reconnect. Clipboard text is not stored
in usage/favorites or the historical intent ledger. Status reports opaque result
identities, not queries/titles/clipboard contents.

## Native Display Configuration

The settings editor normalizes connector, pixel mode/precise refresh, fractional
scale, transform, logical position, enablement and supported VRR. Its diagram uses
the same geometry as numeric drafts and freezes the pointer-to-layout mapping
during a drag. Configuration validation rejects invalid modes/values, duplicate
connectors and an all-disabled layout before native application.

One bounded, thread-owned controller handles discovery, preview, keep/save and
revert. Hyprland uses bounded native IPC with legacy/Lua version adapters;
wlr-output-management uses proxy-specific state, the advertised manager serial,
and separate one-shot test/apply configurations. No donor unsafe thread markers,
global current-head state or rounded refresh-rate identifiers are reused.

Preview records a monotonic 20-second rollback lease in the controller, independent
of the GUI's event loop. Revert, expiry, failed persistence and orderly owner drop
restore the connected baseline while preserving newly connected outputs. Keep saves
one revision-checked sparse profile array before releasing the lease; it retains
unknown profile fields and never writes Nix/compositor configuration files. Saved
profiles can apply at startup or through an explicit Apply Saved action. Abrupt
process termination cannot execute in-process rollback; that limitation and hardware
policy checks remain documented separately.

## Native Audio Ownership

The Audio plugin defaults to one native `libpulse-binding` context/mainloop for
PulseAudio or PipeWire's PulseAudio-compatible server. The subscription owns the
worker; dropping it wakes and joins the worker. Thread-affine handles stay on
that thread, and a bounded request queue plus Unix wake socket bridges Iced tasks
to it without host Tokio TLS or unsafe thread markers.

Sink/source/server events coalesce into device snapshots queried through the same
context. Cached indexes and channel volumes support default-device, volume and
mute actions; volume scaling preserves channel balance. Monitor sources are
filtered by protocol metadata. Normal device refresh does not invoke `pactl` or
run a periodic device scan. Commands are bounded, expire while queued, and are
rejected offline rather than replayed after reconnect. Errors retain the previous
snapshot; reconnect resubscribes and queries authoritative state.

Device snapshots and MPRIS state are independent model domains. Media properties
still refresh at `audio.poll_seconds`, using bounded asynchronous zbus calls and a
cancellable async-io timer. Per-domain action generations and media query generations
reject stale results; a device action cannot suppress a completed media refresh.
All bar, popup and control-center views share this model and subscription. Explicit
`audio.backend = "pactl"` retains the compatibility adapter on background workers;
backend and refresh-interval changes require restart. Card profiles, per-stream
routing and event-driven MPRIS remain follow-ups.

## Native Bluetooth Ownership

The Bluetooth plugin owns one asynchronous zbus system-bus connection on a
subscription-owned worker. A weak client bridges the no-model-argument subscription
ABI; dropping the stream cancels and joins the owner without host Tokio TLS.
ObjectManager supplies initial state; interface and property signals update the
object-path cache. Invalidated properties resnapshot. Reply sequence watermarks
avoid replaying pre-snapshot events, and BlueZ's unique owner identifies each epoch.
Commands target that owner, expire while queued and are rejected offline. Reconnect
registers a fresh agent and resnapshots; cached content remains visible meanwhile.

Adapter power/selection, connect/disconnect, trust/remove and bounded discovery
share this owner. Discovery tracks only sessions started by this connection;
timeout/cleanup releases those sessions without stopping another client's scan.
Configured plugins stay loaded with empty/offline state so service and hardware
hotplug can recover. Normal inventory/actions do not run `bluetoothctl` or poll.

The `KeyboardDisplay` Agent1 handles PIN/passkey input and display, numeric
confirmation, authorization and service authorization. Calls must come from the
current BlueZ owner for the active user-initiated device. Typed replies wait for
the UI, reject invalid/stale answers, and never fabricate codes or auto-accept.
The plugin registers its own agent without taking global default-agent ownership.

Pairing events travel in the originating task stream, preserving modal output
routing. Pair, trust and connect form one deadline-bound operation. Closing the
modal cancels the pending reply and protocol operation; cancellation during Connect
also disconnects that new connection. Session IDs prevent old agent requests or
queued prompts from cancelling/reopening a newer operation. Discovery and pairing
limits are restart-only settings. General incoming pairing and additional adapter
capabilities remain follow-ups.

## Plugin ABI

- Required symbols: `abi_version`, `name`, `model`, `update`, `launch`, `view`, `errors`, and `subscription`.
- Optional symbols: `availability`, `metadata`, `popup_metrics`, `popup_view`,
  `modal_view`, `panel_view`, `toast_view`, `runner_query`, `runner_entries`,
  `settings_schema`, `config_changed`, `view_with_context`, `bar_view`, and
  `control_center_view`, `modal_closed`, `command`, `osd_view`, `dock_entries`,
  `desktop_view`, `desktop_interactive`, `compositor_changed` and
  `subscription_policy`, `theme_changed`, `runner_prefixes`, `runner_status`,
   `runner_cancel`, `runner_favorite`, `config_validate` and `session_menu`.
- `availability(config)` lets a plugin skip loading before model initialization, init tasks, and subscriptions when required hardware or system services are unavailable.
- `ABI_VERSION` in `oxibar-plugin-api` gates host/plugin compatibility.
- Plugin models are `Arc<RwLock<Box<dyn OxiAny>>>`; plugin messages are `Arc<dyn OxiAny>`.
- Plugin subscriptions return a raw pointer to a boxed stream. Successfully
  initialized libraries stay process-resident: opaque models, messages, tasks and
  deferred destructors can contain plugin vtables after a surface is retired.
  Owned subscriptions still cancel/drop normally; library retention is not another
  service instance and does not introduce hot unloading.
- `control_center_view(model)` has the ordinary `ViewFn` signature and returns
  inner content without a second full-height scroll surface. The capability is
  optional at ABI 6; existing popup/panel exports remain compatible fallbacks.
- `bar_view(model, PluginViewContext, bool)` uses the optional `BarViewFn` alias to
  render output-local labels without mutating a shared provider. Required ABI 6,
  existing context layout and legacy view exports remain unchanged.
- `modal_closed(model)` optionally returns a cleanup task. It is idempotent and
  nonblocking; returned work must remain valid after surface retirement. Required
  exports/signatures and ABI 6 remain unchanged.
- `command(model, verb, arguments)` validates a bounded resident action and returns
  a task whose outcome contains both the ordinary provider message and the actual
  result. The host applies that message before replying to IPC.
- `session_menu(model)` returns cached enabled actions/descriptions. The host filters
  configured choices and delegates selection through the existing command capability;
  it does not introduce another logind owner or bypass confirmation.
- `osd_view(model, topic)` renders cached, read-only inner content. Cross-library
  OSD requests use `HostOsdRequest::into_message()` string transport, matching the
  existing toast convention; SDK struct `Any` identity can differ between separately
  compiled dependency graphs.
- `dock_entries(model)` returns a shared catalog with stable launch/desktop IDs.
  `desktop_view(model, context)` returns instance-aware inner content, and optional
  `desktop_interactive(model)` defines automatic input policy.
- `compositor_changed(model, snapshot)` shares host window/workspace state.
  `subscription_policy()` optionally selects resident or visible-only ownership;
  absent capabilities retain compatible defaults at ABI 6.
- `theme_changed(model, Arc<ThemeSnapshot>)` receives an explicit immutable value.
  `runner_cancel(model)` is nonblocking and returns no task; favorite hooks return
  a declared sparse field, and `config_validate(table)` is pure and thread-safe.

## System Services and OSD Ownership

System owns one cancellable asynchronous zbus connection and bounded command queue.
Power profiles prefer the modern UPower interface, with the older hadess interface
as fallback. Unique-owner epochs, independent bounded profile/login queries and
sequence-gated snapshots prevent one unavailable domain from erasing another.
Backlight files have one owned watcher and a bounded fallback poll, because sysfs
notifications are not reliable across drivers. No remote service inventory polls.

Brightness prefers the current user's verified login1 session helper, falling back
to ordinary file permissions only when that helper is unavailable/unsupported.
Fresh session UID/ID checks protect brightness/logout from stale session identity.
Capabilities distinguish allowed, authentication-required and unsupported actions.
Power/logout commands open a confirmation modal; only confirmation delegates to
login1 and the system policy agent. Locking launches a configured argument vector
for a dedicated locker. This shell does not implement a lock protocol.

One OSD per output reuses its ID and replaces its previous abort-on-drop timer.
Generation-gated expiry cannot close a newer update. Initial snapshots are quiet;
successful actions retain their captured output, while external changes follow
current focus. OSDs use no keyboard interactivity, an empty input region and zero
exclusive zone. Hiding bars retains them; output retirement cancels their timers.
Geometry, enablement and duration apply live without another provider model.
Media feedback uses the notification stack's shared placement while retaining the
same OSD instance, size, generation and expiry policy. Other topics use the ordinary
OSD placement; changing topics reconfigures the existing surface.

## Current Plugins

- `audio`: native PulseAudio/PipeWire device/default/volume/mute events and actions,
  asynchronous MPRIS controls, and explicit `pactl` compatibility; shared bar,
  scrollable popup and control-center content.
- `battery`: Linux power-supply sysfs percentage, charging/draining state, and calendar-style hover tooltip with duration estimates.
- `bluetooth`: native BlueZ adapter/device events, owned discovery, power,
  connect/disconnect, trust/remove and complete cancellable Agent1 pairing; shared
  bar, popup, control-center and task-modal content.
- `system`: cached backlights, native power profiles, verified login1 session
  capabilities/actions and dedicated-locker launch; shared controls and brightness/
  profile OSD content.
- `clock`: time display, local calendar popup with event-day tooltips, optional HTTPS CalDAV event sync, and configurable external calendar launcher. `tick_seconds` is the real-clock poll interval; when the format shows seconds the plugin still emits a `Tick` every second, simulating intermediate ticks from a monotonic anchor and only re-reading `Local::now()` every `tick_seconds`.
- `network`: NetworkManager `nmcli` connection management and password modal; popup and modal surfaces.
- `notifications`: Freedesktop notification server, independent toast layers,
  inline replies and DND; the inbox uses a bar dropdown by default, with optional
  separate-panel presentation. Toast hover may enable on-demand reply focus;
  incoming toasts do not steal it. Inbox visibility suppresses toasts without
  deleting records, and toast expiry removes only its surface.
- `tray`: StatusNotifier watcher, dynamically sized tray item popup, activation, and DBusMenu-backed detached context menu rendering. Item registration publishes a fallback row immediately and refreshes DBus metadata asynchronously so registering applications are not blocked by property queries.
- `workspaces`: output-aware workspace display and dispatch from the shared host broker.
- `applications`: resident OxiRun application/icon index, fuzzy matching, terminal
  support, localized fields/actions, inherited icons, filesystem refresh and
  usage/favorite ranking; runner/dock share immutable catalogs and stable IDs.
- `tools`: commands, calculator, files, bookmarks, named argument actions and
  opt-in native clipboard history with cancellable cached searches.
- `stats`: cached CPU, memory and uptime summaries with visible-only refresh.
- `example`: separately packaged SDK reference with shared views, live schema,
  validation, scoped confirmation cleanup, commands/OSD and an owned visible timer;
  it is available for the installed catalog but not in the default enabled set.

Plugin `lib.rs` files should keep ABI symbols, model update, and view composition. External-system logic and parsers should live in sibling modules once they grow beyond small helpers.

## Tradeoffs

- Runtime integrations combine pure unit coverage with opt-in private service,
  real dylib and repeatable isolated compositor tests. Physical hardware and policy
  integrations retain explicit manual follow-ups.
- The clock plugin's CalDAV sync shells out to `curl` to avoid adding a full HTTP stack to the dynamic plugin. It enforces `https://`, curl HTTPS-only protocol flags, and normal certificate verification.
- Thunderbird integration is limited to launching `thunderbird --calendar`; date-focused navigation remains a user-configured `calendar_command` concern because Thunderbird has no stable CLI for opening a specific calendar date.
- Per-output width/height/layout/transparency and top/bottom placement come from
  common bar settings plus named profiles. Geometry is in logical coordinates;
  the layer-shell backend handles native compositor buffer/fractional scaling.
- Active-monitor toast placement uses a modular compositor-backend registry. The current backend uses Hyprland monitor data because Wayland/layer-shell and the available `wayland-protocols` output protocols describe/configure outputs but do not expose a compositor-generic focused-output query; non-Hyprland sessions fall back to the previous `LastOutput` behavior.
- The stream raw-pointer ABI is documented as practical but not fully C-ABI-safe.
- Dynamic plugins cannot assume the host's Tokio runtime is visible through their
  own copy of Tokio TLS. Runner workers and native audio/BlueZ/System owners are cancelled
  and joined by their owners; asynchronous DBus/media uses async-io/zbus. Native
  clipboard uses a wakeable poll loop and owns all transfer FDs.

## Shell Expansion

`SHELL_ROADMAP.md` defines the proposed Rust/Iced widget shell, compatible plugin
capability extensions, and source-aware GUI overrides over immutable Nix defaults.
Source-aware overrides, structured control commands and the initial settings UI
are implemented, together with the first multi-output registry/inventory slice
and control-center composition. Native audio, BlueZ and system services/OSDs are implemented;
settings/control-center layouts share spacious oxiced controls while retaining
the existing palette. Dock and persistent desktop widgets are implemented with
shared catalogs, compositor state and visible-only streams. Live theme snapshots
and expanded providers are implemented. The SDK, startup/app handoff, repeatable
smoke checks and named presentation-time release gate are implemented and verified.

## SDK, Application Lifetime and Release Verification

`PLUGIN_SDK.md` defines required ABI 6 signatures, optional capabilities and owned
resource rules. Installed catalog inspection resolves ABI/name/capability metadata
without model or subscription initialization; availability precedes factories and
duplicate names instantiate once. GUI enablement loads missing providers on a
serialized background worker; disable/re-enable retains the resident model and code.
Host requests from disabled providers are rejected, and code replacement requires restart.

SDK application handoff prefers a native user-manager transient service in
`app.slice`, separate from the shell service's termination cgroup. It subscribes to
job completion before submission and reports the actual accepted result. An
uncertain or accepted failure cannot trigger a second direct execution. Definite
unsupported/unavailable managers fall back to argument-based, reaped process-group
launch; that fallback cannot promise independent cgroup ownership. Caller-only
service/activation FD variables are removed from the new unit's environment.

Home Manager startup is opt-in and bound to a configured compositor session target.
Enabled packages and disabled installed catalog packages are separate; immutable
base generation never owns GUI overrides. All host/plugin packages share Crane
artifacts and the same patched layer-shell dependency graph.

The SDK interval stream owns its cancellation timer. Clock/Stats visible timers and
Network/Battery resident refresh intervals no longer create detached polling loops.
Filesystem watchers retain unchanged watches, track directory device/inode/mode
and filter unrelated events from missing-root parent watches, preventing idle
reindex feedback loops.

`scripts/run-isolated.py` owns a private bus/compositor/runtime for repeatable GUI
checks. `scripts/performance.py` uses optional `wp_presentation` feedback before
the next drawn buffer, actual presented timestamps and native surface retirement.
Normal runtime has no tracing protocol/writes. `RELEASE_CHECKS.md` records the named
reference, query/idle/wakeup-proxy/RSS/worker gates and repeatable commands.

The roadmap identifies selective ReSet audio/BlueZ and monitor backend patterns.
Audio, BlueZ and native display-configuration adapters follow these boundaries.
No donor daemon/GTK architecture is required by the Rust/Iced host or plugin API.

Window overview is implemented in `WINDOW_OVERVIEW_PLAN.md`: exact-window content
thumbnails grouped per monitor, capability-gated capture and identity-safe mouse/
keyboard activation. Dock icon rendering and first-open calendar/control-center
responsiveness are verified alongside the existing shell flows.

## Nix Packaging

- `inputs.crane` provides Cargo derivations for Nix builds.
- `flake.nix` creates one cleaned Cargo source, one vendored dependency set, and one `buildDepsOnly` workspace artifact derivation with `--workspace` package selection.
- `nix/default.nix` builds only the host package from those shared Cargo artifacts.
- Cargo source cleaning retains the vendored overview protocol XML. Host checks
  receive their declared Wayland runtime library paths before the final wrapper,
  so private native protocol tests also execute inside the Nix sandbox.
- `nix/plugin.nix` builds only the requested plugin package from the same shared Cargo artifacts and installs the resulting `lib*.so` under `$out/lib/`.
- The local layershellev patch is shared by every host/plugin graph. Crane's dummy
  dependency build preserves that backend's real public source for external
  iced_layershell consumers; the final package source remains normal workspace code.
- Native dependencies must be available to shared dependency compilation and the
  owning final plugin. `libpulseaudio` is in the Audio library's runtime search
  path; other plugins do not gain that plugin-specific runtime dependency.
- The host wrapper supplies fontconfig and the argument-based `xdg-open` helper.
  The backend source has its own filtered input so unrelated application edits do
  not invalidate the shared dependency artifact's post-patch source identity.
- This reduces rebuild time when multiple plugins are updated because dependency crates such as `iced`, `oxiced`, `zbus`, and `oxibar-plugin-api` are compiled once per dependency graph instead of once per plugin derivation. Final plugin crate compilation and linking still happen per plugin.
