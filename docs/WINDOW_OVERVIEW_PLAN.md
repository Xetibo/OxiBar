# Window Overview Design and Verification

Status: **implemented** after approval of this plan. Host-owned monitor grids,
native per-window content capture, exact activation and bounded visible ownership
are verified. Platform-specific capabilities and proof levels are listed below.

## Requested experience

Show a small, real view of every open window, grouped on its monitor. Mouse click
or keyboard navigation selects a specific window and brings that existing window
to the foreground. A thumbnail must contain the window's content, not merely its
icon/title or a crop of whatever currently covers its output.

Use the existing Rust/Iced/oxiced shell, theme snapshots, output registry and
compositor boundaries. The compositor continues to own window management.

## Interaction

- Open one large centered modal per connected monitor, bounded to 90% of output
  dimensions and configurable maximum width/height. Each shows its monitor's grid;
  all surfaces share one overview model and capture owner.
- Keep a clear selected card, title/application label and live content thumbnail.
  Preserve aspect ratio with themed letterboxing; identify unavailable/minimized
  captures honestly rather than presenting an icon as a content preview.
- Click a card or press Enter to activate its exact current native window and close
  all overview surfaces. Do not relaunch its application.
- Arrow keys navigate the active grid; Tab/Shift+Tab cycles that monitor's windows.
  h/j/k/l chooses left/down/up/right monitor neighbors by logical geometry.
  Home/End and Escape are scoped to the active overview. Selection scrolls into view.
- All navigation bindings are live preferences in Settings → Shortcuts.
- Only the active overview requests keyboard focus. Pointer selection on another
  monitor moves that active context; surface creation order must not own focus.
- Animate selection using the shared duration, with direct final states for zero
  duration/reduced motion. Keep card geometry stable and do not animate old captured
  content into a newly reused native window identity.

## Architecture

### 1. Native inventory and exact identities

The visible capture owner exposes an opaque window key, native capture/activation
handle, title/app ID, output membership and capture/state capabilities. Native proxies
stay on their owner thread; the UI receives immutable metadata and image handles.
The existing host compositor broker supplies cached metadata through one bridge.

Hyprland's mapping protocol combines both 32-bit words into the complete native
address. It links an exact ext capture resource to a unique exact wlr output/seat
activation resource once. If that resource closes, a recycled address cannot rebind
the old ext window. Legacy capture uses the exact wlr handle directly. A title/app-ID
match or truncated address is never an identity bridge.

Closed/recycled handles invalidate captures, queued jobs and displayed actions.
New windows, moves, title changes, minimized state and output hotplug update one
shared snapshot; there is no window scanner or polling owner per monitor/card.

### 2. Capability-gated per-window capture

Probe advertised protocols and versions before creating capture sessions:

| Backend path | Real content source | Identity/activation requirement |
| --- | --- | --- |
| Modern ext protocols | `ext_foreign_toplevel_image_capture_source_manager_v1` + `ext_image_copy_capture_manager_v1` | Exact ext handle plus Hyprland mapping to an exact wlr output/activation handle; real private protocol-server tests. |
| Hyprland extension | `hyprland_toplevel_export_manager_v1` v2 | `capture_toplevel_with_wlr_toplevel_handle` and exact wlr handle/seat activation; actual private Hyprland 0.51.1 compositor tests. |
| Native list without capture | Window metadata/available activation only | Explicit unsupported-content diagnostic; Sway 1.11 is verified in this mode. |
| Output-only screencopy | Insufficient for this feature | Cannot show an occluded or other-workspace window's content. Report unsupported preview capability. |

Do not truncate a 64-bit Hyprland native address into the extension's legacy
32-bit capture argument. Verify extension/handle support on each target release.
`ext_foreign_toplevel_list_v1` provides stable identifiers/title/app ID and close
events, but it does not provide window-to-output/workspace mapping or activation.
An output capture source is distinct from a toplevel capture source.

The implementation uses compositor-advertised SHM formats and checked CPU
copy/downsample, including stride, transforms, Y inversion and premultiplied alpha.
DMA-BUF imports remain an optimization requiring separate lifetime/performance proof.

### 3. One visible-only capture owner

- Start captures when overview opens; drop sessions, callbacks, mappings and timers
  when its last surface closes. Retain cheap metadata separately from pixels.
- Allow at most one outstanding frame per window and a small global in-flight
  budget. Use latest-only jobs with window/source generation checks.
- Validate width/height/stride/format and checked byte multiplication before
  allocation. Bound full-size staging separately from downsampled image data;
  reject oversized/unsupported sources visibly. Driver texture overhead is separate.
- Enforced budgets: at most two full-size captures/one per window in flight, 64 MiB
  staging, 96 MiB thumbnail image data and 256 windows. Thumbnail dimensions adapt
  downward when many sources would exceed the cache budget.
- Modern capture honors damage/readiness; legacy export requests fresh frames at
  the configured cadence, capped at 8 Hz. Prioritize selection and never force
  continuous rendering/capture work when overview is closed.
- Do not persist pixels or expose them in diagnostics. Clear/recycle buffers when
  a window closes, changes incarnation, is denied capture or overview closes.

Follow each protocol's lifecycle rather than ordinary surface assumptions:
ext-copy sessions batch constraints through `done`, permit one active frame, and
signal `ready`/`failed`/`stopped`; resize replaces constrained buffers. That protocol
does not use `wl_buffer.release` to signal capture completion. Hyprland export
advertises buffer details, then `buffer_done`, copy, flags/ready/failed; destroy the
completed frame and account for Y inversion, transforms and actual stride.

### 4. Host-owned overview surfaces

`SurfaceKind::Overview` and source-scoped actions render grids through the existing
registry/logical geometry. Output removal closes its surface and moves selection
to a still-valid group without discarding unrelated captures. Only one overview
surface requests keyboard focus.

Cards carry the immutable displayed window key, not a list index. Activation checks
that key/handle again, waits for every overview native Closed event, then delegates
to the compositor. An abortable generation-scoped deadline handles failed retirement.
A late capture cannot repaint another card after reorder/closure. Actual visible
card bounds handle cross-monitor exclusive-grab pointer redirection; hidden/clipped
cards cannot receive those clicks.
Launching apps and existing dock activation continue using their current contracts.

## Completed delivery slices

1. **Protocol inventory and identities:** capability report and one native toplevel
   inventory; exact output grouping, close/reuse guards and activation tests.
2. **Capture proof:** a private compositor fixture with distinct changing window
   content. Capture one exact window while another occludes it and while it is on
   another workspace. Prove pixels, format/stride/transform and cleanup.
3. **Overview UI:** themed per-monitor grids, stable selection, click/keyboard
   activation, Escape and reduced-motion transitions.
4. **Dynamic/lifecycle behavior:** native moves/hotplug, identical titles, stale
   frame/activation callbacks, source resize/denial, address reuse and owned shutdown.
5. **Release checks:** bounded capture scheduling/cache, idle/worker/RSS checks,
   repeatable isolated smoke, SDK/configuration/architecture documentation.

No thumbnail plugin is required for each monitor. `oxibar overview [toggle|open|close]`
and the reserved bar `overview` item expose the feature. Settings → Launcher contains
`overview.enabled`, `width`/`height`, `refresh_fps` (1..8), `thumbnail_size` (128..640) and `columns`
(1..8), applying without daemon restart. Bind the CLI through the compositor for a
global shortcut. A title filter or additional platform adapter is future work.

## Acceptance gates

- Real text/checkerboard/video-like fixture pixels appear in the correct miniature
  on every monitor, including occluded and other-workspace windows when supported.
- Identical titles and multiple windows of one app remain separate. Mouse/keyboard
  activation selects the exact displayed native instance; stale/closed/reused IDs
  can never activate a different window.
- Moving a window updates its output group; hotplug, resize, denial and reconnect
  produce valid previews or explicit unavailable states.
- Open/close cycles leave no capture workers, sessions, callbacks or pixel cache
  growing monotonically. Closed overview causes no capture/render timer wakeups.
- Buffer limits are enforced; selected-card responsiveness and first actual
  presentation are measured on a named multi-window reference, separately from
  compositor configure acknowledgement.
- Existing bar dropdowns, notification replies, dock activation, modals and input
  focus remain correct while opening and closing the overview.

## Recorded verification and boundaries

- Full workspace: **268 normal tests/doctests**, 14 explicitly opt-in tests, strict
  workspace Clippy and all **14 Nix outputs** pass. Native protocol XML remains in
  cleaned Nix sources; private native checks receive their declared Wayland runtime.
- Actual private Hyprland 0.51.1 capture proves distinct changing red/blue pixels for
  identical-title windows, including fresh pixels while occluded/on another workspace.
  Packaged per-monitor grids prove real colors, exact keyboard/cross-monitor pointer
  activation after grab retirement, hotplug and ten reopen cycles. Every close has
  zero capture workers, empty pixels/staging and stable thread count.
- A normal hermetic Wayland server proves modern ext SHM capture, formats before
  size, resize, denial/stopped cleanup and exact activation across native address reuse.
  This is real protocol execution, not an actual Hyprland 0.56.2 desktop runtime
  claim: the attempted nested current-version fixture failed before connection.
- Sway 1.11 advertises a conforming presentation clock, but not supported per-window
  content capture here. Its two-window overview **UI/closed-ownership** reference
  passes first actual presentation **31.73 ms**, warm p95 **37.25 ms**, closed CPU
  **0.40% of one core**, zero closed draws/capture workers and **+138 KiB** reopen
  median RSS across 30 opens. Pixel capability is explicitly reported unsupported;
  the Hyprland real-pixel proof is separate. All seven reference gates pass.
- Existing complete UI/legacy smokes pass, including real settings-through-OSD input,
  native display save/revert, bar menus, CalDAV, password, tray and notification flows.
  Private fixture processes/files/compositors are reaped; user desktop/hardware is untouched.

Repeatable commands and exact workload/counters are in `TESTING.md` and
`RELEASE_CHECKS.md`. Mutter/KWin capture, driver-specific minimized behavior,
DMA-BUF imports and broader modern compositor integration remain explicit extensions.

## Protocol references

- [ext-image-capture-source-v1](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/main/staging/ext-image-capture-source/ext-image-capture-source-v1.xml)
- [ext-image-copy-capture-v1](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/main/staging/ext-image-copy-capture/ext-image-copy-capture-v1.xml)
- [ext-foreign-toplevel-list-v1](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/blob/main/staging/ext-foreign-toplevel-list/ext-foreign-toplevel-list-v1.xml)
- [Hyprland toplevel export](https://github.com/hyprwm/hyprland-protocols/blob/main/protocols/hyprland-toplevel-export-v1.xml)
- [Hyprland exact toplevel mapping](https://github.com/hyprwm/hyprland-protocols/blob/main/protocols/hyprland-toplevel-mapping-v1.xml)
- [wlr-foreign-toplevel-management-v1](https://gitlab.freedesktop.org/wlroots/wlr-protocols/-/blob/master/unstable/wlr-foreign-toplevel-management-unstable-v1.xml)
