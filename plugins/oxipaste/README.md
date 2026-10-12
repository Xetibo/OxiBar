# OxiPaste provider

Optional ABI 6 provider for the updated OxiPaste daemon's persistent clipboard
history. One shared model serves the shell picker, bar popup, control center and
interactive desktop widget. The daemon retains text and common images across sessions.

## Install

Build host and plugin from the same pinned workspace:

```sh
nix build .#oxibar .#oxibar-oxipaste
```

Install the plugin package's `lib/liboxipaste.so` in
`$XDG_CONFIG_HOME/oxibar/plugins/`. For Cargo development, build the workspace and
use `scripts/install-plugins.sh`. Run the updated, separately packaged
`oxipaste_daemon` in the same session; the provider uses its payload-addressed
`CopyContent`/`DeleteContent` D-Bus methods. An old daemon shows an actionable error.

```toml
plugins = ["liboxipaste.so"] # Include your other enabled libraries.

[bar]
end = ["OxiPaste"]

[control_center]
widgets = ["OxiPaste"]

[oxipaste]
paste_shortcut = "ctrl+v"
poll_seconds = 2
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

The daemon is configured separately. After importing OxiPaste's Home Manager
module, enable its package and opt-in service:

```nix
programs.oxipaste = {
  enable = true;
  systemd.enable = true;
};
```

Import the compositor environment into the user manager before starting the
configured graphical session target.
Restart Oxibar after replacing loaded libraries or changing restart-only plugin
settings. Place a desktop widget through Oxibar's desktop layout editor.

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
and enabled; the daemon must run separately. Escape (or configured
`shortcuts.close`) and the footer's Close button dismiss the picker. Other
exclusive overlays dismiss it, and disabling the provider closes it. Command
status includes `oxipaste_open` and `oxipaste_output`.

- Click the content preview to paste into the application receiving focus below.
  The daemon confirms a successful clipboard write before Oxibar dismisses its
  interactive surfaces, waits for native keyboard focus release and sends the
  configured paste shortcut.
- **Copy** only changes the clipboard and retains the picker.
- **Delete** targets the exact displayed payload; concurrent history reordering
  cannot retarget a row. **Clear all** clears the daemon's persistent history.
- Search text/MIME, inspect image previews, or Refresh after restarting the daemon.
  Service/action errors remain visible and failed copies never inject a shortcut.

`paste_shortcut` accepts `ctrl+v` or `ctrl+shift+v`. `poll_seconds` accepts 1–3,600,
default 2, and only runs while visible. Paste requires compositor virtual-keyboard
support; the Nix host includes `wtype`. Copies preserve exact binary bytes/MIME and
newlines independently of display previews. Encoded data is limited to 16 MiB per
item and 64 MiB aggregate, matching the daemon.

See [Testing](../../docs/TESTING.md#persistent-clipboard-and-native-paste) for the
private-service, actual-dylib and real native receiver regressions.
