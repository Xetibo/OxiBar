# OxiPaste provider

ABI 6 provider with a plugin-owned persistent clipboard backend. Enabling OxiPaste
starts native text/image capture with Oxibar; it continues while all clipboard
views are closed. One shared model serves the shell picker, bar popup, control
center and interactive desktop widget. Disabling the plugin stops capture and
retains saved history. No separate daemon or systemd service is required.

## Install

Build host and plugin from the same pinned workspace:

```sh
nix build .#oxibar .#oxibar-oxipaste
```

Install the plugin package's `lib/liboxipaste.so` in
`$XDG_CONFIG_HOME/oxibar/plugins/`. For Cargo development, build the workspace and
use `scripts/install-plugins.sh`. Enable the library to start its backend; bar,
control-center and desktop placement are optional.

```toml
plugins = ["liboxipaste.so"] # Include your other enabled libraries.

[bar]
end = ["OxiPaste"]

[control_center]
widgets = ["OxiPaste"]

[oxipaste]
paste_shortcut = "ctrl+v"
poll_seconds = 2
max_items = 100
```

The Home Manager module, also exported as `nixosModules.home-manager`, installs
and enables OxiPaste by default. Configure optional layout placement and the paste
shortcut alongside the shell:

```nix
programs.oxibar = {
  enable = true;
  config.plugin_config = {
    bar.end = [ "OxiPaste" ]; # Optional for the shell picker command.
    oxipaste.paste_shortcut = "ctrl+v";
  };
};
```

An explicit `programs.oxibar.config.plugins` list replaces the defaults; include
`inputs.oxibar.packages.${pkgs.system}.oxibar-oxipaste` in that list to keep the
provider enabled. `extraPlugins` installs disabled catalog entries, which can be
enabled through Settings → Plugins.

Import the compositor environment into the user manager before starting the
configured Oxibar graphical session target.
Restart Oxibar after replacing loaded libraries or changing restart-only plugin
settings. Place a desktop widget through Oxibar's desktop layout editor.

## Backend and persistence

The enabled provider owns one resident subscription, native Wayland connection
and joined worker. It prefers ext-data-control and falls back to wlr-data-control.
Capture does not depend on a visible bar or picker. Unsupported protocol/session
errors appear in the shared view instead of silently showing an empty history.
The worker resolves a live Wayland socket independently instead of consuming the
host's inherited `WAYLAND_SOCKET` descriptor, including Hyprland endpoint recovery.
Capture makes only the pipe's read end nonblocking: producers keep a blocking
write end so images larger than a pipe buffer are captured completely.

History uses `$XDG_CONFIG_HOME/oxipaste/items` (or `~/.config/oxipaste/items`), reads
existing binary and legacy text history, and saves atomically with mode `0600`.
Corrupt files are reported and preserved. The bundled backend uses Oxibar's
`[oxipaste]` settings, including `max_items` (default 100, range 1–5,000); encoded
data is also bounded to 16 MiB per item and 64 MiB total.

The backend serves `org.Xetibo.OxiPasteDaemon` with payload-addressed
`CopyContent`/`DeleteContent` and the standalone OxiPaste compatibility methods.
An already running updated external daemon retains ownership; the provider reads
it using `poll_seconds` (default 2, range 1–3,600). Bundled capture is event-driven.
The standalone daemon's `ShutDown` method cannot terminate the plugin-owned
backend or Oxibar: disable the provider to stop it.

## Interaction

Open directly from the shell or a compositor keybind:

```sh
oxibar oxipaste        # Toggle the picker.
oxibar oxipaste open   # Open, or keep the current picker open.
oxibar oxipaste close
oxibar oxipaste toggle
```

The resident shell opens a centered, keyboard-focused overlay on the focused
output, with search focused immediately. Bar/control-center/desktop placement is
optional; the command works with the bar disabled. The provider must be installed
and enabled. Escape (or configured `shortcuts.close`) and the footer's Close button
dismiss the picker. Other
exclusive overlays dismiss it, and disabling the provider closes it. Command
status includes `oxipaste_open` and `oxipaste_output`.

- Click the content preview to paste into the application receiving focus below.
  The backend confirms a successful clipboard write before Oxibar dismisses its
  interactive surfaces, waits for native keyboard focus release and sends the
  configured paste shortcut.
- Inline **Copy** and **Delete** icon buttons share one row. Copy only changes the
  clipboard and retains the picker.
- Delete targets the exact displayed payload; concurrent history reordering
  cannot retarget a row. **Clear all** clears the persistent history.
- Filter **All**, **Text**, **Addresses** or **Images**, combined with text/MIME
  search. Addresses include single-line URLs and paths; Text excludes addresses.
  Inspect image previews or request a fresh history snapshot.
  Service/action errors remain visible and failed copies never inject a shortcut.

`paste_shortcut` accepts `ctrl+v` or `ctrl+shift+v`. Paste requires compositor
virtual-keyboard support; the Nix host includes `wtype`. Copies preserve binary bytes/MIME and
newlines independently of display previews. Backend writes do not recapture or
reorder their own copied items; PasteAndDelete removes an item only after a
successful clipboard acknowledgement.

See [Testing](../../docs/TESTING.md#persistent-clipboard-and-native-paste) for the
private-service, actual-dylib and real native receiver regressions.
