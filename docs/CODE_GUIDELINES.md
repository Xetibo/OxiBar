# Code Guidelines

## SDK and Release Ownership

- Retain initialized native plugin code for process lifetime; deferred trait-object
  destructors, messages and tasks may outlive their original surface. Cancel owned
  streams normally; do not equate code retention with another service or hot unload.
- Preserve provider identity until host-request dispatch. Disabled models may
  consume pending data/cleanup results but cannot open surfaces or emit host actions.
  Serialize live factories and reconcile completed loads against current config.
- Use owned SDK interval streams instead of detached forever polling threads.
- Diff filesystem watch path/mode/device/inode identity. Filter parent-watch events
  to the configured root relationship; unrelated logs must not cause reindex loops.
- Launch external apps through the shared argument-based user-manager handoff from
  worker context. Never direct-launch again after uncertain submission acceptance.
  Keep shell-only graphics/library/font overrides out of child apps; preserve and
  restore the incoming session environment rather than stripping legitimate user settings.
- Normalize recovered Wayland endpoints before startup creates threads. Later
  handoffs repair their own child environment; never mutate process environment
  from threaded UI/worker code. Remove discarded variables from transient-manager
  inheritance explicitly, not only from the supplied environment list.
- Measure actual compositor presentation for first-frame gates. Keep measurement
  traces outside watched fixtures and preserve normal idle behavior when disabled.

## Rust Style

- Authentication secrets must not implement Clone or unredacted Debug, enter
  general shell messages/configuration/logs, or be copied into ordinary widget
  state. Keep response allocations bounded and zeroized, guard attempt identity,
  and treat PAM-owned native response buffers as a separate ownership boundary.
- Masked authentication input uses explicit application focus, not ordinary widget
  focus. Route prompt keys even when Iced marks them captured, preserve native
  username editing outside prompts and use modified character keys when text is absent.
  Keep one logical form focus. Native inputs must relinquish focus during Tab/submit
  event dispatch, before later batched keys; deferred focus tasks are insufficient.
  Retain native-widget context on delayed events across view changes.
- PAM is a fixed administrator-owned current-user policy, never a user-selected
  service. Keep blocking conversations in the private cancellable worker, reject
  missing service configuration, and never map errors or cancellation to unlock.
- Build authentication binaries and their compositors from the deployment package
  set: system PAM modules and EGL vendors must share a compatible native ABI.
- Greeter IPC authenticates its root peer, bounds complete frames and serializes
  requests. Drain cancellation before reusing an authentication conversation. Parse
  normal protocol errors separately from transport/protocol corruption; worker
  cleanup failure must not discard a healthy connection or authenticate a user.
  Automatic prompt preparation must wait for cancellation acknowledgement and stop
  on create-session errors rather than retrying them indefinitely. Parse
  administrator-owned session metadata into argv without shell re-interpretation;
  only an acknowledged authenticated start permits successful kiosk exit.
- Authentication appearance parsing and image allocation are bounded; greeter
  configuration/assets retain administrator ownership checks. Verify native CPU
  pixel byte order with rendered frames, not only opacity/geometry assertions.
- Treat lock readiness as a protocol event, never process spawn. Hold the scoped
  inhibitor through a live child's validated readiness token; bound delay leases,
  reject explicit suspend on readiness failure and preserve a locked child when its
  watcher stops. Clear repeat identity on focus/device loss and honor widget wake
  deadlines without retaining password text in repeat or redraw state.

- Keep changes small and aligned with existing module boundaries.
- Prefer pure helper functions for parsing, state transitions, and config extraction so behavior is unit-testable without DBus, Wayland, Hyprland, or shell commands.
- Keep dynamic plugin ABI entry points thin. Delegate testable behavior to normal Rust functions where practical.
- Plugins that require optional hardware or system services should export `availability(config)` and keep the probe cheap; put the actual probe logic in `system.rs` so it can be unit-tested.
- Reconnecting native services may stay loaded with empty/offline state. Do not
  gate them on currently present hardware when hotplug recovery is required.
- Keep model-dependent popup sizing in pure helpers and expose it through optional `popup_metrics(model)` only when content height truly varies.
- Keep plugin `lib.rs` focused on ABI, model updates, and view composition. Move command/DBus integrations, parsers, and domain data into modules such as `system.rs` before files become god files.
- Use `Result<T, String>` for plugin-local external command/DBus errors when the error is surfaced through a plugin error queue.
- Avoid long-lived locks around UI building or external calls.
- Add comments only when code is not self-explanatory or unsafe/lifetime behavior needs documentation.
- Do not leave magic numbers in code. Use named constants for domain/protocol values, put user-tunable values in config, and use `oxiced` theme/helper tokens for UI spacing, padding, margins, border radii, and font sizes.
- Keep bar/window dimensions behind `src/layout.rs` helpers. Do not reintroduce monitor-width constants in app, message, or surface code.
- Startup retry behavior is config-driven via the `[startup]` TOML table parsed into `config::StartupRetryPolicy`. Keep timing constants and retry knobs out of `src/app.rs`; add new user-tunable startup options there (see `DECISIONS.md`).
- Single-instance behavior is config-driven via the `[instance]` TOML table parsed into `config::InstancePolicy`. Keep lock acquisition in `src/single_instance.rs`; `src/app.rs` only acquires the guard at startup (see `DECISIONS.md`).
- Add active-output compositor integrations under `src/monitor/` as small backends returning an output name; keep toast/panel layer construction in `src/messages.rs` backend-agnostic.
- When the same UI number repeats across plugins, add or use a shared `oxiced` token/helper before adding plugin-local constants.
- Extend plugins with optional capability exports when existing signatures suffice. Preserve provider-local launch indexes when ranking search results globally; never broadcast a selected result's index to unrelated plugins.
- Keep blocking plugin work in runtime-independent workers with immutable snapshots. Dylibs have independent Tokio TLS; they must not assume the host's reactor is visible. Cancel/join long-lived workers when their owning stream is dropped, and generation-gate asynchronous search results.
- Read runtime settings from immutable `ConfigSnapshot`s. Route all GUI/IPC writes
  through the revision-checked config store; never write base/Nix configuration or
  expand defaults into the sparse override file.
- Declare full dotted plugin paths in optional `settings_schema` exports. Live
  fields require a compatible `config_changed` hook; hooks receive the full
  document and must ignore unrelated changes and retain restart-only running values.
- Add shared controls/styles to `crates/oxiced` and use the workspace dependency;
  do not restore per-plugin git/path forks of that library.
- Deliver immutable theme snapshots explicitly through the optional hook. Use
  font-aware shared text/input/picker helpers; retain glyph fonts for icons. Do
  not mutate unsynchronized palettes, and bypass animation builders for reduced
  motion or zero duration. Upload font bytes before changing the active family.
- Map Base24 slots to shared semantic roles before explicit role/accent overrides.
  Partial schemes inherit validated seed values; derive missing Base16 extensions
  from base colors. Preserve explicit-role metadata and validate automatic text
  contrast across state backgrounds without rewriting requested accent/scheme colors.
- Runner jobs use bounded latest-only workers, cooperative cancellation and
  generation guards. Capture immutable provider/local action identity in rendered
  activation messages; keep logical favorite keys separate from action contents.
- Persist versioned usage in private XDG state only after loading existing counts.
  Invalid state must not be silently overwritten. Clipboard content/identities
  must not enter persistent ranking state or historical intent caches.
- Pure configuration validators may run off the UI thread; keep their library
  owner alive and validate before adopting or writing a candidate.
- Missing filesystem roots watch an existing parent nonrecursively. Re-register
  actual roots on creation without adding an entire home-directory recursive watch.
- Register every surface before creation; route view/events by its known ID and
  kind. Never infer that an unknown window is a bar. Surface tasks must use explicit
  target IDs for geometry/input updates.
- Normalize output inventory off the UI thread, preserve refresh-rate precision,
  and separate logical output geometry from pixel modes/native buffer scale.
- Keep drag snapping pure and deterministic over enabled logical display bounds,
  including fractional scale, rotation and negative positions. Preserve the gesture
  origin for overlap rollback; exact numeric edits bypass snapping.
- Display previews belong to a bounded backend owner with a monotonic rollback
  lease. Save through the revision-checked sparse store before releasing it; stale
  confirmations and failed writes must not retain an unconfirmed native layout.
  Use advertised wlr head/mode proxies and serials, never global head state or
  refresh-rate rounding as identity.
- Capture actual bar-child bounds and the pressed slot. Use the same clamped
  placement for popup drawing, input regions and dismissal, and prune stale slots
  after layout changes. Attached dropdown closure must not remove its owning bar.
- Bounds-capture widgets need stable logical identity as well as geometry. Invalidate
  cached rectangles when a same-sized replacement reuses the widget tree.
- Edge-reveal state belongs to the registered output-local bar. Update content,
  input region and exclusive zone without recreating the surface or provider models.
- Output-local label preferences belong in optional per-view context, never in shared
  provider state. Preserve required exports and existing ABI context layouts.
- Drag/drop mutations carry source identity and starting revision. Keep keyboard
  reorder alternatives and preserve unrelated sparse fields when replacing arrays.
- Draw-only previews never intercept drop input. Custom widget wrappers need unique
  tree tags and defensive child reconciliation across empty/optional-child views.
  Avoid vertical `Fill` inside unbounded scroll content, including alignment helpers
  that silently replace a fixed height with `Fill`.
- Host keys belong in the validated shortcut schema and central surface-scoped
  dispatcher. Reject overlapping-context conflicts, preserve text-input capture
  semantics and derive visible hints from effective bindings.
- Keep plugin service ownership independent of output/view count. Preserve source
  context for UI-initiated asynchronous work without dropping model updates after
  the originating surface disappears.
- Embedded capability views reuse the existing model and return inner content;
  host surfaces own chrome and scrolling. Factor shared popup/embedded view logic
  without creating another subscription, backend or polling owner.
- Reusable subwidgets expose stable provider-local IDs and natural-height inner
  views. Model-aware page availability is separate from load-time availability;
  hiding an unsupported domain must not stop its reconnecting service.
- For nonblocking clipboard capture, set `NONBLOCK` on the read end only. External
  producers must be able to write payloads larger than pipe capacity without EAGAIN.
- Keep thread-affine native handles on their owning worker. Bound request queues
  and operation waits, coalesce replacement actions, reject expired/offline
  commands and resnapshot after reconnect rather than replaying stale actions.
- Keep independently refreshed domains separate in the model, and generation-gate
  action acknowledgements and queries so older work cannot overwrite newer state.
- Resident command replies report completion, not queue acceptance. Apply the
  ordinary model outcome first; capture output context before starting work.
  Bound concurrent IPC handlers so slow actions do not block fast commands.
- Use the SDK's serialized host-request helpers across dylibs, rather than relying
  on SDK struct `Any` identity across independently compiled dependency graphs.
  Bar callbacks deliver plugin-local messages; emit host requests from the returned
  update task so host translation runs consistently for all surfaces.
- Non-interactive surfaces must not alter keyboard ownership. Route keyboard events
  by compositor Enter/Leave, independently of pointer focus or surface creation.
- Pending keyboard-repeat work must be canceled with its focus/device ownership;
  revalidate the live keyboard and repeating key before installing a timer.
  Device capability loss must also clear and publish native focus loss, even if
  the removed device never sent Leave. External paste waits for native handoff,
  with bounded generation-scoped cancellation rather than a fixed delay.
- Coalesce transient OSDs per output and abort replaced timers; expiry carries a
  generation. Initial service snapshots are quiet, failed actions do not show success.
- Scope DBus state/actions to the service's unique-owner epoch. Subscribe before
  taking snapshots, reconcile signal ordering and resnapshot invalidated properties.
- Interactive protocol requests need typed, validated replies and session-scoped
  cancellation. Old request timeouts must not cancel newer sessions. Export optional
  nonblocking `modal_closed` cleanup for operations whose lifetime includes a modal;
  cleanup tasks must survive the retired surface without reopening stale prompts.
- Keep schema-driven view construction separate from settings state/actions and
  persistence. Use shared oxiced surface buttons, badges and themed switches for
  spacious forms; preserve compact bar controls and the existing theme palette.
- Give nested settings pages stable route IDs and assign each declared descriptor
  once. Preserve route/expansion/drafts across live schema changes, fall back to a
  valid ancestor after removal, and retain existing root shortcut indexes. Keep
  navigation transitions separate from configuration writes.
- Keep save/reload feedback in bounded status space and preserve busy widget trees;
  changing callback availability must not resize controls or the form viewport.

## Instance and Compositor State

- Keep provider-local actions stable across sorting, reindexing and removal; never
  reinterpret a stale vector position as a new application.
- Treat native window addresses as reusable handles. Keep incarnation IDs and
  revalidate native identity/PID before delegating activation.
- Capture and activate through exact native toplevel resources. Bridge protocol
  identities only through an advertised exact mapping, never title/app-ID matching
  or a truncated address. Do not rebind an old window after its activation handle closes.
- Own capture proxies, buffers and deadlines in one visible-only worker. Validate
  pixel shape/stride/format before allocation, cap aggregate storage/jobs, and gate
  callbacks by current frame object as well as window identity. Constraints are
  batched, not ordered; capture completion is protocol-specific, not buffer release.
- Release all exclusive overview surfaces and wait for their native Closed events
  before activating a window. Abort completed/canceled retirement deadlines and
  discard clipped hit bounds rather than retaining an old visible rectangle.
- Animate reveal bounds independently of full child layout. Calendar/form content
  must not relayout to each intermediate animation height; external refresh/launch
  work stays off the UI executor.
  Mask the child cursor outside the visible/parent viewport intersection in update,
  hover and draw; still forward releases with an unavailable cursor to clear presses.
- Persist requested logical geometry, not clamped fallback placement. Keep unknown
  instance fields and starting-revision conflicts intact when saving drafts.
- Capture drag press coordinates from the actual widget event; a separately queued
  global cursor update is not a reliable gesture origin.
- Passive views should not capture desktop input. Allow explicit per-instance
  overrides and keep editing input separate from normal interaction.
- Visible-only subscriptions must own cancellable timers/resources. Services remain
  resident by default, and multiple consumers share one model and stream.

## Testing Expectations

- Add unit tests near the helper functions they cover.
- Prefer deterministic tests that do not require live system services.
- Main loop tests should cover pure host logic: config parsing, message mapping, popup metrics, placement math, and state transitions that do not require layershell runtime.
- Plugin tests should cover each plugin's parsing, config, helper logic, and model update behavior where possible.

## Formatting And Checks

- Use `cargo fmt` for formatting.
- Use `cargo test --workspace` for test coverage.
- Use `cargo clippy --workspace --all-targets` when practical before larger refactors.

## Nix

- `flake.nix` exists. Prefer running commands from the flake/dev shell where practical.
- Nix Rust packages use Crane. Keep host and plugin packages wired to the shared workspace `cargoArtifacts` in `flake.nix` instead of creating per-plugin dependency caches.
- Wire native libraries into shared dependency compilation and the owning plugin's
  final build/runtime search path; keep unrelated plugin runtime inputs minimal.
