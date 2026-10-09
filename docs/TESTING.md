# Testing

## Strategy

- Unit tests cover host main-loop helpers and every plugin crate.
- Tests avoid live DBus, NetworkManager, Bluetooth, Hyprland, PulseAudio, Wayland, or notification daemon dependencies unless explicitly marked as integration tests.
- External command wrappers remain manually verified unless they are refactored behind injectable seams.
- Plugin system modules carry parser/helper tests next to the code they own.

## Current Unit Coverage

- Host/config: plugin allow-list parsing, plugin unavailable error formatting, anchor parsing, host-request message mapping, dynamic bar width/height fallback parsing, popup geometry/config/dynamic metrics, toast keyboard interactivity actions, toast output-option fallback mapping, panel output anchoring, bar section lookup, layer resize state updates, popup state setters, `[instance]` policy parsing (defaults, configured values, wrong value types), and single-instance lock acquisition (second acquire reports already-running; lock released when the guard drops).
- `audio`: percent parsing, file URL decoding, selected-device fallback, config poll interval, view/popup construction, and error draining.
- `battery`: sysfs battery availability/parsing, percentage and duration estimates, config poll interval, view construction, and error draining.
- `bluetooth`: `bluetoothctl` controller availability parsing, device parsing, noise filtering, pairing prompt detection, view/popup/modal construction, and error draining.
- `clock`: config parsing, invalid config defaults, format second-specifier detection, display-interval granularity, simulated-now arithmetic, month arithmetic, Thunderbird launcher mapping, CalDAV HTTPS guard, iCalendar/CalDAV parsing, recurrence/RDATE expansion, event tooltip details, ABI metadata, and error draining.
- `network`: `nmcli` field splitting, scan interval parsing, label/icon helpers, view/popup/modal construction, and error draining.
- `notifications`: markup stripping, notification action selection, notification timeout config, notification add/replace/remove/clear state transitions, DND toast suppression, toast-only expiry, hover/focus/draft expiry preservation, toast keyboard opt-in requests, panel rendering, and error draining.
- `tray`: StatusNotifier address normalization, tray label fallback, DBusMenu label cleanup, item/hover/context-menu state update, dynamic popup metrics, popup/action rendering, and error draining.
- `workspaces`: active workspace state update, empty launch behavior, view construction, and error draining without requiring Hyprland.
- Runner: XDG desktop-entry precedence/hidden masking, command argument expansion, terminal/keyword parsing, empty-query and bounded matching, stale query rejection, worker execution without Tokio, timer cancellation, global ranking/local indexes, exact-provider activation, retained bar-plugin state, duplicate-activation prevention and window-scoped keyboard handling.
- IPC: command validation, configured socket path, a real command/acknowledgement round trip and stale-socket recovery.
- Settings: sparse merge/reset/source behavior, explicit equal-to-base intent,
  readonly Nix symlink regeneration, unknown-field preservation, private atomic
  writes, managed-symlink refusal, revision conflicts, invalid-input retention,
  live geometry/input-region actions, widget layout, retained plugin models and
  structured socket replies. Plugin tests cover live hook state preservation.

## Expected Commands

- `cargo fmt --all`
- `nix develop -c cargo test --workspace`
- `nix develop -c cargo clippy --workspace --all-targets`
- `nix build .#oxibar .#oxibar-audio` after Nix packaging changes, with more plugin package targets as needed.

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

## Coverage Goals

- Host: config-derived bar placement, plugin message mapping, popup geometry, and modal/panel state transitions where they can run without a GUI runtime.
- Plugins: ABI metadata, model defaults, parser helpers, update-side error draining, and helper functions for external data formats.

## Known Gaps

- Dynamic library loading from `$XDG_CONFIG_HOME/oxibar/plugins/` is not covered by an integration test yet.
- The opt-in runner dylib test exercises real library exports/tasks, but general availability-gate/load-order coverage through `load_plugins()` is still missing.
- System-service subscriptions are not exercised automatically because they start background threads and depend on live services. The runner timer stream is exercised by its opt-in dylib test.
- UI rendering tests only validate that view functions return elements for valid models; they do not assert pixel output.
- Keyboard search, text-field traversal/application, category shortcuts and Escape
  are verified; generic checkbox/button keyboard focus and screen-reader support
  remain a shared-control follow-up.
