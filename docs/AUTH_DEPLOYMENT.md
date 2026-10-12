# Locker and kiosk greeter deployment

`oxilock` and `oxisession` are independently packaged Rust/Iced applications. They
do not load shell plugins. `oxilock` locks an existing Wayland session through
`ext-session-lock-v1`; `oxisession` is an ordinary fullscreen Wayland greeter whose
privileged authentication/session owner is greetd.

## NixOS

```nix
{ inputs, ... }: {
  imports = [ inputs.oxibar.nixosModules.authentication ];
  programs.oxilock.enable = true;
  programs.oxilock.sleepIntegration = true; # Optional graphical-session watcher.
  services.oxisession = {
    enable = true;
    backend = "cage"; # Or "hyprland".
    keyboardLayout = "us";
    settings = {
      appearance.clock_format = "%H:%M";
      theme.accent = "blue";
      theme.reduced_motion = true;
    };
  };
}
```

The locker module installs the fixed `/etc/pam.d/oxilock` policy with null-password
authentication disabled. The greeter module installs an unprivileged `oxisession`
account and runs `oxisession-kiosk` as greetd's default session on VT 1. greetd owns
PAM, UID changes, logind and the selected user's session. Choose one display
manager for that VT.

Session entries come from administrator-owned system Wayland desktop files.
`services.oxisession.sessionPackages` defaults to NixOS display-manager session
packages; it can also contain explicit packages with `share/wayland-sessions`.
Entries and their executables must be administrator-owned, and commands are passed
as validated argument arrays. Inspect the installed catalog with:

```sh
nix run .#oxisession -- --list-sessions
```

The owned kiosk supervisor uses absolute packaged compositor paths. Cage hosts
only the greeter. Hyprland gets a new private minimal configuration, with no user
startup hooks or desktop bindings. After greetd acknowledges the selected session,
the supervisor requires both greeter success and successful compositor teardown.
Hyprland exit is bound to the owned PID and instance signature. A failed or crashed
greeter cannot report an authenticated handoff; greetd restarts its default session.

The packaged Cage includes xdg-shell version-6 support and a guard for fullscreen
requests before the initial surface commit, required by Iced's fullscreen startup.
The keyboard layout is supplied to the owned compositor and displayed in the
greeter. It is passed in the actual default-session command because greetd clears
its daemon's environment. Power off/restart are available only when logind permits
them, require confirmation and recheck permission before execution.
`Ctrl+Shift+P` / `Ctrl+Shift+R` opens that confirmation; Escape cancels and restores
username focus. Authentication and confirmed session start remain separate.

## Home Manager / existing desktop

```nix
{ inputs, ... }: {
  imports = [
    inputs.oxibar.homeManagerModules.default
    inputs.oxibar.homeManagerModules.oxilock
  ];
  programs.oxilock = {
    enable = true;
    sleepIntegration = true;
    settings = {
      appearance.clock_format = "%H:%M";
      theme.accent = "blue";
      theme.reduced_motion = true;
    };
  };
  programs.oxibar.config.system.lock_command = "oxilock";
}
```

Home Manager installs the user application; the administrator must supply the
dedicated PAM policy, through the NixOS module above or an equivalent system setup.
It does not install a privileged display manager or a user-selected PAM policy.

```sh
oxilock --check             # Probe protocol support without locking.
oxilock                    # Lock and authenticate the current session user.
oxilock --ready-fd 3 3>ready # Writes "locked\n" only after protocol readiness.
```

Launching `oxisession` requires `GREETD_SOCK` from a root-owned greetd peer. Running
it inside an already logged-in desktop is not a substitute for a greetd login
session. Unsupported session-lock protocol or missing/untrusted locker PAM policy
is reported before interactive locking; a fullscreen overlay is not a lock fallback.

## Appearance and input

The user locker reads `$XDG_CONFIG_HOME/oxibar/auth.toml` (or
`~/.config/oxibar/auth.toml`). The greeter reads administrator-owned
`/etc/oxibar/auth.toml`; `services.oxisession.settings` generates that file.
These are read-only authentication settings, separate from the shell's GUI overrides.

```toml
[appearance]
clock = true
clock_format = "%H:%M"
date_format = "%A, %e %B"
overlay = 0.65
# wallpaper = "/absolute/path/wallpaper.png"
# avatar = "/absolute/path/avatar.png"

[theme]
accent = "blue"
font_scale = 1.0
reduced_motion = true
```

Existing `oxiced` theme tokens apply. Clock formats are bounded and validated;
overlay accepts `0.4`–`1.0`. PNG/JPEG/WebP assets use absolute paths, at most 16 MiB
input, 8192-pixel dimensions and 64 MiB decode allocation. Greeter assets and their
ancestor paths must also be administrator-owned. For NixOS-owned assets:

```nix
environment.etc."oxibar/wallpaper.png".source = ./wallpaper.png;
services.oxisession.settings.appearance.wallpaper = "/etc/oxibar/wallpaper.png";
```

Missing/invalid assets keep an opaque themed fallback. Cards remain centered with
a scroll fallback on small outputs or long prompts. Secret responses are masked,
bounded and zeroized; no password-bearing Iced text-input widget or clipboard is
used. Enter submits, Escape clears/cancels an attempt, and Ctrl+U clears the response.
Escape never unlocks. The locker reports native XKB Caps/group state. The greeter
shows the enforced kiosk layout and unknown Caps until observed keys establish it.

## Readiness-gated sleep

```sh
oxilock-sleep --watch   # Subscribe to PrepareForSleep with a bounded delay lease.
oxilock-sleep --suspend # Lock first; refuse explicit suspend on readiness failure.
```

Enable `programs.oxilock.sleepIntegration` through NixOS or Home Manager to install
the watcher user unit. Use one unit owner. The NixOS module supplies standard polkit;
Home Manager relies on the system's logind/polkit policy and dedicated PAM service.
The compositor must import its Wayland environment before the graphical-session
target, or start the unit explicitly after importing it:

```sh
systemctl --user import-environment WAYLAND_DISPLAY
systemctl --user start oxilock-sleep.service
```

The watcher subscribes before acquiring inhibition and holds the delay FD until
the live owned locker writes its complete protocol-ready token. Delay is bounded
by logind's advertised deadline; refusal/timeouts are reported and never called
ready. A delay inhibitor cannot indefinitely prevent system-forced sleep. Use
`--suspend` for a command that must fail instead of sleeping without confirmed lock.
Run it from the active desktop's permission context, such as a compositor keybind.
It holds a block lease through readiness, then requests non-interactive logind sleep.

On resume, the watcher reacquires inhibition and reuses a live owned locker.
Stopping/restarting the watcher leaves authentication available. Compositor crash
recovery remains compositor policy; the locker never unlocks on renderer/auth failure.

```ini
# Hyprland
bind = SUPER, L, exec, oxilock
bind = SUPER SHIFT, L, exec, oxilock-sleep --suspend
```

```conf
# Sway
bindsym $mod+l exec oxilock
bindsym $mod+Shift+l exec oxilock-sleep --suspend
```

## Reproducible verification

```sh
nix build .#oxilock .#oxisession
nix build .#checks.x86_64-linux.authentication-unit
nix build .#checks.x86_64-linux.authentication-clippy
nix build .#checks.x86_64-linux.authentication-cage
nix build .#checks.x86_64-linux.authentication-hyprland
```

Both packages and unit/Clippy checks are defined for `x86_64-linux` and
`aarch64-linux`; real seat/VT VM checks run on x86-64. They use disposable accounts
and credentials, real greetd/PAM/DRM, and assert UID, active seat/VT, kiosk retirement,
rejected login, logout and fresh-PID crash recovery. Native lockers cover assets,
fractional scale/hotplug, Caps, held-key editing, failed-password retry and crash
protection. Cage's VM additionally verifies both actual S3 paths, readiness before
inhibitor release, visible locked resume, lease reacquisition and watcher-stop child
preservation. Its legacy VGA/pixman fixture avoids pinned virtio-GPU S3 fence stalls;
Hyprland retains virtio-GPU coverage. They never use the developer's desktop.
