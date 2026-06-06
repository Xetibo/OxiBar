# Testing

## Strategy

- Unit tests cover host main-loop helpers and every plugin crate.
- Tests avoid live DBus, NetworkManager, Bluetooth, Hyprland, PulseAudio, Wayland, or notification daemon dependencies unless explicitly marked as integration tests.
- External command wrappers remain manually verified unless they are refactored behind injectable seams.
- Plugin system modules carry parser/helper tests next to the code they own.

## Current Unit Coverage

- Host/config: plugin allow-list parsing, anchor parsing, host-request message mapping, popup geometry/config/dynamic metrics, bar section lookup, and popup state setters.
- `audio`: percent parsing, file URL decoding, selected-device fallback, config poll interval, view/popup construction, and error draining.
- `bluetooth`: `bluetoothctl` device parsing, noise filtering, pairing prompt detection, view/popup/modal construction, and error draining.
- `clock`: config parsing, invalid config defaults, month arithmetic, Thunderbird launcher mapping, CalDAV HTTPS guard, iCalendar/CalDAV parsing, recurrence/RDATE expansion, event tooltip details, ABI metadata, and error draining.
- `network`: `nmcli` field splitting, scan interval parsing, label/icon helpers, view/popup/modal construction, and error draining.
- `notifications`: markup stripping, notification action selection, notification add/replace/remove/clear state transitions, DND toast suppression, hover expiry preservation, panel rendering, and error draining.
- `tray`: StatusNotifier address normalization, tray label fallback, DBusMenu label cleanup, item/hover/context-menu state update, dynamic popup metrics, popup/action rendering, and error draining.
- `workspaces`: active workspace state update, empty launch behavior, view construction, and error draining without requiring Hyprland.

## Expected Commands

- `cargo fmt --all`
- `nix develop -c cargo test --workspace`
- `nix develop -c cargo clippy --workspace --all-targets`

Oxibar patches the `oxiced` git dependency to `../oxiced` for local shared-helper development, so run oxiced checks too when changing shared UI helpers:

- `cd ../oxiced && nix develop -c cargo test`
- `cd ../oxiced && nix develop -c cargo clippy --all-targets`

Run commands through `nix develop` when the ambient Rust toolchain was installed through Nix or rustup with a Nix linker wrapper.

## Coverage Goals

- Host: config-derived bar placement, plugin message mapping, popup geometry, and modal/panel state transitions where they can run without a GUI runtime.
- Plugins: ABI metadata, model defaults, parser helpers, update-side error draining, and helper functions for external data formats.

## Known Gaps

- Dynamic library loading from `$XDG_CONFIG_HOME/oxibar/plugins/` is not covered by an integration test yet.
- Live subscriptions are not exercised automatically because they start background threads and depend on system services.
- UI rendering tests only validate that view functions return elements for valid models; they do not assert pixel output.
