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
  services.oxisession = {
    enable = true;
    backend = "cage"; # Or "hyprland".
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

## Home Manager / existing desktop

```nix
{ inputs, ... }: {
  imports = [
    inputs.oxibar.homeManagerModules.default
    inputs.oxibar.homeManagerModules.oxilock
  ];
  programs.oxilock.enable = true;
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
session. Locker readiness-aware sleep integration belongs to the following stage.

## Reproducible verification

```sh
nix build .#oxilock .#oxisession
nix build .#checks.x86_64-linux.authentication-clippy
nix build .#checks.x86_64-linux.authentication-cage
nix build .#checks.x86_64-linux.authentication-hyprland
```

Both packages are defined for `x86_64-linux` and `aarch64-linux`; the real seat/VT
VM checks run on x86-64. They use disposable accounts and credentials, virtio DRM,
real greetd/PAM, and assert UID, active seat/VT, kiosk retirement, rejected login,
logout and fresh-PID crash recovery. They never use the developer's desktop.
