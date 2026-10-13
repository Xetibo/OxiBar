# Release checks and reference measurements

## Build and source checks

Build host and libraries from the same pinned flake/workspace. Native ABI 6 passes
Rust trait objects across the boundary; the version gate does not replace matching
compiler/dependency graphs.

```sh
nix develop -c cargo fmt --all -- --check
nix develop -c rustfmt --edition 2024 --check crates/layershellev/src/lib.rs crates/layershellev/src/performance.rs
nix develop -c cargo test --workspace
nix develop -c cargo clippy --workspace --all-targets -- -D warnings
nix build .#oxibar .#oxibar-example .#oxibar-applications .#oxibar-commands .#oxibar-tools
```

The preceding overview release verification built all fourteen outputs: host and thirteen
individually selectable plugins. The example is not in the default enabled set.
268 normal tests/doctests and strict workspace Clippy passed; fourteen environment-
dependent tests are opt-in. No real hardware/service access is needed by normal tests.

The subsequent dock/session/label follow-up also passed the full normal workspace
suite/doctests, formatting, strict all-targets Clippy and all fourteen pinned packages.
The host was rebuilt after the logical-bounds identity fix; thirteen shared-widget
tests and two complete final isolated UI runs passed. These prove icon-only/live
host and provider labels, centered 800×480 session tiles/native confirmation,
application indexing/owned launch and post-drag button anchoring. The overview smoke
proves actual SVG dock pixels with Applications absent from XDG and supplied through
the configured library fallback. Current private installer fixtures verify all-fourteen
preflight, no partial write on missing Applications/Commands/OxiPaste and custom build/config roots.
See `TESTING.md` and the current continuity entry in `DECISIONS.md` for reproduction
and the earlier unlocated-panic diagnostic limitation.

For the real example and loader contracts:

```sh
EXAMPLE="$(nix build .#oxibar-example --no-link --print-out-paths)/lib/libexample.so"
nix develop -c env OXIBAR_EXAMPLE_PLUGIN="$EXAMPLE" cargo test -p oxibar --test example_dylib -- --ignored
nix develop -c env OXIBAR_EXAMPLE_PLUGIN="$EXAMPLE" cargo test -p oxibar real_loader_catalog_and_availability_skip_models_and_duplicate_names -- --ignored
```

The contract runs 25 model/view/task/subscription/config/theme/modal cycles without
a host Tokio reactor. Loader inspection does not initialize models; availability
rejection happens before initialization, and duplicate plugin names initialize once.
Successful provider code stays process-resident until every possible deferred Rust
vtable/callback is no longer in use.

## Isolated compositor smoke

`nix/gui-tests.nix` provides pinned test tools, Mesa and a headless Sway parent.
Its test-only Sway xdg-shell advertisement patch permits a nested Hyprland fixture.
`scripts/run-isolated.py` owns its temporary runtime, private bus and compositors;
cleanup terminates only those processes. It does not use the actual desktop's
clipboard, user-manager environment, network, calendar or hardware services.

```sh
HOST="$(nix build .#oxibar --no-link --print-out-paths)/bin/oxibar"
APPS="$(nix build .#oxibar-applications --no-link --print-out-paths)/lib/libapplications.so"
TOOLS="$(nix build .#oxibar-tools --no-link --print-out-paths)/lib/libtools.so"
EXAMPLE="$(nix build .#oxibar-example --no-link --print-out-paths)/lib/libexample.so"
CLOCK="$(nix build .#oxibar-clock --no-link --print-out-paths)/lib/libclock.so"
NETWORK="$(nix build .#oxibar-network --no-link --print-out-paths)/lib/libnetwork.so"
NOTIFICATIONS="$(nix build .#oxibar-notifications --no-link --print-out-paths)/lib/libnotifications.so"
TRAY="$(nix build .#oxibar-tray --no-link --print-out-paths)/lib/libtray.so"
WORKSPACES="$(nix build .#oxibar-workspaces --no-link --print-out-paths)/lib/libworkspaces.so"
nix develop --impure --file nix/gui-tests.nix -c python scripts/run-isolated.py -- \
  python scripts/smoke.py "$HOST" "$APPS" "$TOOLS" "$EXAMPLE" "$CLOCK" \
  "$NETWORK" "$NOTIFICATIONS" "$TRAY" "$WORKSPACES"
```

The smoke exercises installed/disabled catalog state, SDK modal Escape/replacement,
actual settings application while an OSD is visible, launcher activation, CalDAV
fixture/day launch, secure network password save, native tray dbusmenu selection,
notification toast/panel/inline reply/DND, two desktop instances with actual drag/
resize, stale revision retention, output hotplug and ten reopen cycles. Network and
HTTPS calendar calls use owned fixture executables; tray and notifications use the
private bus. Screenshots are optional and should remain private.

Native service/clipboard/private user-manager opt-in commands are in `TESTING.md`.
The user-manager mock confirms a launched app survives caller exit and an uncertain
accepted RPC never causes a second direct launch.

Home Manager evaluation without activation:

```sh
nix eval --impure --json --expr \
  '(import ./nix/hm-test.nix) { homeManagerPath = "/path/to/home-manager"; }'
```

Use a Home Manager release aligned with the pinned nixpkgs. The test checks all
module assertions, no startup unit by default, deliberate session-target startup,
default clipboard provider installation/enablement through the NixOS module alias,
custom XDG library paths, disabled installed example catalog, explicit enabled-only
base generation, unmanaged overrides and separate shell cgroup ownership.

## Actual presentation/performance gate

```sh
nix develop --impure --file nix/gui-tests.nix -c python scripts/run-isolated.py --parent -- \
  python scripts/performance.py "$HOST" "$APPS" "$TOOLS" --output /tmp/oxibar-performance.json
```

The parent Sway fixture advertises `CLOCK_MONOTONIC` for `wp_presentation`.
Optional `OXIBAR_PERF_TRACE` instrumentation requests feedback before Iced draws
the next committed buffer. The benchmark waits for that surface's first actual
`Presented` event and native retirement. Configure/IPC acknowledgment timestamps
are not substituted for presentation. Normal runtime does not bind this measurement
protocol or write trace data.

Query latency starts when the host accepts the complete input revision and ends at
its first accepted result set. The script uses a documented local 2,000-desktop-
entry catalog, 50 warm opens, 20 queries and ten seconds of closed-overlay idle.
CPU is process user/system time as a percentage of one core; all-thread voluntary
context switches are a **wakeup proxy**, not an exact hardware wakeup counter.
RSS uses first/last ten-cycle medians with a 2 MiB noise budget plus a no-worker-
growth gate. Trace files live outside watched fixture paths.

### Recorded reference — 2026-10-11

AMD Ryzen 9 7950X, AMD GPU PCI IDs `1002:744c` and `1002:164e`, kernel
`7.2.8-xanmod1`, pinned Sway 1.11/headless GLES2. Release profile: Applications and
Tools, launcher/settings bar, no dock. [Machine-readable record](performance-reference.json).

| Measurement | Result | Gate |
| --- | ---: | --- |
| Cold control endpoint | 122.68 ms | Recorded |
| Cold bar actual presentation | 122.04 ms | Recorded |
| Warm launcher median / p95 / max | 20.56 / 20.83 / 21.12 ms | p95 < 50 ms |
| Query median / p95 / max | 0.529 / 0.810 / 0.826 ms | All < 16.67 ms |
| Idle CPU | 0.30% of one core | < 1% |
| Idle draws | 0 in 10 seconds | No continuous rendering |
| All-thread wakeup proxy | 202.60 voluntary switches/s | Recorded |
| Reopen RSS median change | +130 KiB | < 2 MiB |
| Initial / final / maximum threads | 66 / 64 / 66 | No worker growth |

All six gates passed. The reference is an isolated compositor workload, not a claim
for every GPU, plugin set, remote index or compositor. Nested Hyprland 0.51.1 omitted
the presentation clock event and returned stale timestamps, so it was rejected as
a performance reference rather than treating configure timing as presentation.

During profiling, filesystem watchers were corrected to retain unchanged watches,
track directory identity and ignore unrelated sibling events under missing-root
parent watches. The regression tests assert quiet idle and correct real updates.

## Overview and control-center references

Build the native window-fixture test binary through the same pinned workspace and
run only under the owned coordinator:

```sh
FIXTURE="$(nix develop -c cargo test -p oxibar --lib --no-run --message-format=json | \
  jq -r 'select(.reason == "compiler-artifact" and .profile.test and .target.kind == ["lib"]) | .executable')"
nix develop --impure --file nix/gui-tests.nix -c python scripts/run-isolated.py -- \
  python scripts/overview-smoke.py "$HOST" "$FIXTURE" "$APPS" "$CLOCK"
nix develop --impure --file nix/gui-tests.nix -c python scripts/run-isolated.py --parent -- \
  python scripts/overview-performance.py "$HOST" "$FIXTURE" \
    --output /tmp/oxibar-overview-performance.json
nix develop --impure --file nix/gui-tests.nix -c python scripts/run-isolated.py --parent -- \
  python scripts/control-center-performance.py "$HOST" "$CLOCK" \
    --output /tmp/oxibar-control-center-performance.json
```

The Hyprland 0.51.1 smoke verifies actual distinct/changing content for two
identical-title native windows, correct output grouping, exact keyboard and
cross-monitor pointer activation, hotplug, SVG/fallback dock icons and ten closes
with zero capture workers/caches. The separate native opt-in test proves changed
occluded/other-workspace content. Modern ext capture is tested through a normal
hermetic Wayland server, including constraint ordering, resize/denial and exact
activation across recycled addresses. It is not a claimed actual 0.56.2 desktop run.

The UI/closed-resource benchmark uses the same named hardware/kernel and pinned
Sway 1.11/headless GLES2 reference as above, with two native source windows and
30 warm opens. Sway truthfully reports capture unsupported: these measurements do
not substitute for the separate real-pixel capability test. Actual `Presented`
feedback and advertised clock 1 are required, never configure timestamps.

| Overview measurement | Result | Gate |
| --- | ---: | --- |
| First actual presentation | 31.73 ms | < 50 ms |
| Warm median / p95 / max | 36.76 / 37.25 / 38.18 ms | p95 < 50 ms |
| Closed capture workers / draws | 0 / 0 in 5 seconds | Zero |
| Closed CPU | 0.40% of one core | < 1% |
| All-thread wakeup proxy | 204.40 voluntary switches/s | Recorded |
| Reopen RSS median change | +138 KiB | < 2 MiB |
| Initial / final / maximum sampled threads | 57 / 55 / 55 | No growth |

All seven gates pass. The real-pixel Hyprland smoke separately records thread
count 64 for every final closed cycle and no capture worker; its closed RSS samples stay
bounded. [Overview JSON record](overview-performance-reference.json).

Control-center opening is measured with a private two-second CalDAV request still
in flight. Twenty warm opens follow the first:

| Control-center measurement | Result | Gate |
| --- | ---: | --- |
| First actual presentation | 31.36 ms | < 50 ms |
| Warm median / p95 / max | 4.41 / 4.94 / 30.05 ms | p95 < 50 ms |
| Concurrent status during blocked sync | 3.85 ms | < 50 ms |

All three gates pass; opening did not wait for sync. Fixed child-layout reveal and
bounded off-executor calendar jobs remove those blocking paths. No percentage
comparison against an older binary is inferred.
[Control-center JSON record](control-center-performance-reference.json).

Current all-fourteen-output builds, full source checks and both retained UI smoke
suites pass alongside these new checks. Counter semantics/noise budgets/private
fixture ownership remain the same as the original reference above.

## Shortcuts, live providers and presentation refinement

Full normal workspace tests/doctests, formatting and strict all-target workspace
Clippy pass for the subsequent refinement. All fourteen Nix outputs built from the
same pinned graph. After GUI-discovered drag tree/layout corrections, the final
host was rebuilt and focused host/shared-widget regressions plus the complete
refined UI smoke passed with those verified provider libraries.

The owned fixtures prove live Applications/Tools enablement, actual private desktop-
entry search and launch, three-column drag with a pointer-following content preview,
session confirmation, full-height calendar and retained native monitor controls.
The separate real-pixel overview smoke proves centered modal geometry, Tab within
a monitor, spatial monitor navigation, live remapping and native retirement before
keyboard/cross-monitor mouse activation.

The control-center workload remains twenty warm opens after a first open during a
private two-second CalDAV request:

| Control-center measurement | Result | Gate |
| --- | ---: | --- |
| First actual presentation | 31.25 ms | < 50 ms |
| Warm median / p95 / max | 5.46 / 6.38 / 21.33 ms | p95 < 50 ms |
| Concurrent status during blocked sync | 4.77 ms | < 50 ms |

All gates pass and first opening does not wait for sync. The prior reference is
preserved; no old-binary speedup is inferred from these separate samples.
[Refinement JSON record](control-center-refinements-reference.json).
