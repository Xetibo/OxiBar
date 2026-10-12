{ pkgs, self, backend }:
let
  sway = pkgs.sway.override {
    sway-unwrapped = pkgs.sway-unwrapped.overrideAttrs (old: {
      postPatch = (old.postPatch or "") + ''
        substituteInPlace sway/server.c --replace-fail '#define SWAY_XDG_SHELL_VERSION 5' '#define SWAY_XDG_SHELL_VERSION 6'
      '';
    });
  };
  swayConfig = pkgs.writeText "auth-user-sway.conf" ''
    output * bg #181825 solid_color
    exec ${pkgs.foot}/bin/foot
  '';
  userSession = pkgs.writeShellScript "auth-user-session" ''
    set -eu
    mkdir -p "$HOME/auth-fixture"
    id -u > "$HOME/auth-fixture/uid"
    printf '%s\n' "$XDG_SESSION_TYPE" > "$HOME/auth-fixture/type"
    printf '%s\n' "''${WAYLAND_DISPLAY-unset}" > "$HOME/auth-fixture/previous-wayland"
    exec ${sway}/bin/sway --unsupported-gpu -c ${swayConfig}
  '';
  desktop = pkgs.runCommand "auth-fixture-desktop" {} ''
    mkdir -p "$out/share/wayland-sessions"
    cat > "$out/share/wayland-sessions/auth-fixture.desktop" <<EOF
    [Desktop Entry]
    Type=Application
    Name=Private session fixture
    Exec=${userSession}
    TryExec=${userSession}
    DesktopNames=AuthFixture;
    EOF
  '';
in pkgs.testers.runNixOSTest {
  name = "oxisession-${backend}-handoff";
  enableOCR = true;
  nodes.machine = { ... }: {
    imports = [ self.nixosModules.authentication ];
    services.oxisession = { enable = true; inherit backend; sessionPackages = [ desktop ]; };
    programs.oxilock.enable = true;
    users.users.alice = { isNormalUser = true; uid = 1000; password = "testpass"; };
    security.pam.services.greetd.enableGnomeKeyring = false;
    hardware.graphics.enable = true;
    documentation.enable = false;
    fonts.enableDefaultPackages = false;
    fonts.packages = [ pkgs.adwaita-fonts ];
    environment.systemPackages = [ pkgs.procps pkgs.foot sway ];
    systemd.services.greetd.environment = {
      ICED_BACKEND = "tiny-skia";
      WLR_RENDERER_ALLOW_SOFTWARE = "1";
      WLR_NO_HARDWARE_CURSORS = "1";
      LIBGL_ALWAYS_SOFTWARE = "1";
    };
    virtualisation.memorySize = 2048;
    virtualisation.qemu.options = [ "-vga none -device virtio-gpu-pci" ];
  };
  testScript = ''
    import re, time
    def visible(pattern):
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            if re.search(pattern, machine.get_screen_text()):
                return
            time.sleep(0.5)
        machine.screenshot("missing-greeter-text")
        raise Exception("Greeter did not display " + pattern)

    start_all()
    machine.wait_for_unit("greetd.service")
    try:
        machine.wait_until_succeeds("pgrep -x .oxisession-wra", timeout=60)
    except Exception:
        print(machine.succeed("journalctl -u greetd --no-pager; ps -eo user,pid,comm,args"))
        machine.screenshot("greeter-startup-failed")
        raise
    visible("Username")
    machine.succeed("${self.packages.${pkgs.stdenv.hostPlatform.system}.oxisession}/bin/oxisession --list-sessions | grep -F auth-fixture")
    machine.screenshot("greeter-${backend}")
    old = machine.succeed("pgrep -x .oxisession-wra").strip()
    compositor = machine.succeed("cat /run/user/$(id -u oxisession)/oxisession-kiosk-*/pid").strip()

    with subtest("failed authentication does not start a user session"):
        machine.send_chars("alice\n")
        time.sleep(2)
        machine.screenshot("password-prompt")
        machine.send_chars("wrongpass\n")
        machine.wait_until_succeeds("journalctl -u greetd --no-pager | grep 'authentication failure'", timeout=60)
        time.sleep(2)
        machine.fail("test -e /home/alice/auth-fixture/uid")
        machine.succeed("kill -0 " + old)
        machine.screenshot("rejected-authentication")

    with subtest("accepted login releases kiosk seat before starting the selected UID"):
        machine.send_key("ret")
        time.sleep(2)
        machine.screenshot("retry-password-prompt")
        machine.send_chars("testpass\n")
        try:
            machine.wait_for_file("/home/alice/auth-fixture/uid", timeout=60)
        except Exception:
            print(machine.succeed("journalctl -u greetd --no-pager; ps -eo user,pid,comm,args"))
            machine.screenshot("handoff-failed")
            raise
        assert machine.succeed("cat /home/alice/auth-fixture/uid").strip() == "1000"
        assert machine.succeed("cat /home/alice/auth-fixture/type").strip() == "wayland"
        assert machine.succeed("cat /home/alice/auth-fixture/previous-wayland").strip() == "unset"
        machine.wait_until_succeeds("pgrep -u alice -x sway", timeout=60)
        machine.fail("kill -0 " + old)
        machine.fail("kill -0 " + compositor)
        machine.wait_until_succeeds("loginctl list-sessions --no-legend | grep alice", timeout=60)
        active = machine.succeed("loginctl show-seat seat0 -p ActiveSession --value").strip()
        assert machine.succeed("loginctl show-session " + active + " -p User --value").strip() == "1000"
        assert machine.succeed("loginctl show-session " + active + " -p Seat --value").strip() == "seat0"
        assert machine.succeed("loginctl show-session " + active + " -p Active --value").strip() == "yes"
        assert machine.succeed("loginctl show-session " + active + " -p VTNr --value").strip() == "1"
        machine.screenshot("user-session-${backend}")

    with subtest("logout returns to a fresh greeter"):
        machine.succeed("pkill -u alice -x sway")
        machine.wait_until_succeeds("pgrep -x .oxisession-wra", timeout=60)
        time.sleep(3)
        machine.screenshot("returned-greeter")

    with subtest("greeter crash restarts the kiosk without an authenticated session"):
        crashed = machine.succeed("pgrep -x .oxisession-wra").strip()
        machine.succeed("pkill -KILL -x .oxisession-wra")
        machine.wait_until_succeeds("! kill -0 " + crashed + " 2>/dev/null && pgrep -x .oxisession-wra", timeout=60)
        assert machine.succeed("pgrep -x .oxisession-wra").strip() != crashed
        visible("Username")
        machine.fail("pgrep -u alice -x sway")
        machine.screenshot("crash-recovery")
  '';
}
