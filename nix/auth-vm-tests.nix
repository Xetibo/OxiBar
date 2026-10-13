{
  pkgs,
  self,
  backend,
}: let
  sway = pkgs.sway.override {
    sway-unwrapped = pkgs.sway-unwrapped.overrideAttrs (old: {
      postPatch =
        (old.postPatch or "")
        + ''
          substituteInPlace sway/server.c --replace-fail '#define SWAY_XDG_SHELL_VERSION 5' '#define SWAY_XDG_SHELL_VERSION 6'
        '';
    });
  };
  swayConfig = pkgs.writeText "auth-user-sway.conf" ''
    output * bg #181825 solid_color
    exec ${pkgs.foot}/bin/foot
    exec sh -c 'printf "%s\n" "$WAYLAND_DISPLAY" > "$HOME/auth-fixture/socket"'
  '';
  avatar = pkgs.runCommand "private-auth-avatar.png" {nativeBuildInputs = [pkgs.python3];} ''
    python - "$out" <<'PY'
    import struct, sys, zlib
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    rows = []
    for y in range(96):
        pixels = [bytes((130, 170, 255, 255) if abs(x - 48) + abs(y - 48) < 34 else (24, 24, 37, 255)) for x in range(96)]
        rows.append(b'\0' + bytes().join(pixels))
    with open(sys.argv[1], 'wb') as image:
        image.write(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 96, 96, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(bytes().join(rows))) + chunk(b'IEND', bytes()))
    PY
  '';
  appearance = pkgs.writeText "private-auth-appearance.toml" ''
    [appearance]
    clock_format = "%H:%M:%S"
    date_format = "%Y-%m-%d"
    wallpaper = "/missing-private-wallpaper.png"
    avatar = "${avatar}"
    [theme]
    accent = "#82aaff"
    reduced_motion = true
    font_scale = 1.1
  '';
  userSession = pkgs.writeShellScript "auth-user-session" ''
    set -eu
    mkdir -p "$HOME/auth-fixture"
    id -u > "$HOME/auth-fixture/uid"
    printf '%s\n' "$XDG_SESSION_TYPE" > "$HOME/auth-fixture/type"
    printf '%s\n' "''${WAYLAND_DISPLAY-unset}" > "$HOME/auth-fixture/previous-wayland"
    ${pkgs.lib.optionalString (backend == "cage") "export WLR_RENDERER=pixman"}
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
in
  pkgs.testers.runNixOSTest {
    name = "oxisession-${backend}-handoff";
    enableOCR = true;
    nodes.machine = {...}: {
      imports = [self.nixosModules.authentication];
      services.oxisession =
        {
          enable = true;
          sessionPackages = [desktop];
          settings = {
            theme.reduced_motion = true;
            appearance = {
              clock_format = "%H:%M:%S";
              avatar = "${avatar}";
              wallpaper = "${avatar}";
            };
          };
        }
        // pkgs.lib.optionalAttrs (backend == "cage") {inherit backend;};
      programs.oxilock = {
        enable = true;
        sleepIntegration = true;
      };
      users.users.alice = {
        isNormalUser = true;
        uid = 1000;
        password = "testpass";
      };
      security.pam.services.greetd.enableGnomeKeyring = false;
      hardware.graphics.enable = true;
      documentation.enable = false;
      fonts.enableDefaultPackages = false;
      fonts.packages = [pkgs.adwaita-fonts];
      environment.systemPackages = [pkgs.procps pkgs.foot sway];
      systemd.services.greetd.environment =
        {
          ICED_BACKEND = "wgpu";
          VK_ICD_FILENAMES = "/missing-private-vulkan-icd.json";
          VK_DRIVER_FILES = "/missing-private-vulkan-icd.json";
          WLR_RENDERER_ALLOW_SOFTWARE = "1";
          WLR_NO_HARDWARE_CURSORS = "1";
          LIBGL_ALWAYS_SOFTWARE = "1";
        }
        // pkgs.lib.optionalAttrs (backend == "cage") {WLR_RENDERER = "pixman";};
      systemd.services.greetd.serviceConfig.StandardError = "journal";
      virtualisation.memorySize = 2048;
      virtualisation.useNixStoreImage = backend == "cage";
      virtualisation.writableStore = true;
      systemd.tmpfiles.rules = ["z /nix/store 1775 root nixbld - -"];
      # The virtio GPU fence queue stalls during QEMU S3 resume on this pinned kernel.
      virtualisation.qemu.options = [
        (
          if backend == "cage"
          then "-vga std"
          else "-vga none -device virtio-gpu-pci"
        )
      ];
    };
    testScript = ''
      import json, re, shlex, time
      def visible(pattern):
          deadline = time.monotonic() + 60
          while time.monotonic() < deadline:
              if re.search(pattern, machine.get_screen_text()):
                  return
              time.sleep(0.5)
          machine.screenshot("missing-greeter-text")
          print(machine.get_screen_text())
          print(machine.succeed("journalctl -u greetd --no-pager; ps -eo user,pid,comm,args"))
          raise Exception("Greeter did not display " + pattern)

      start_all()
      machine.wait_for_unit("greetd.service")
      try:
          machine.wait_until_succeeds("pgrep -x .oxisession-wra", timeout=60)
      except Exception:
          print(machine.succeed("journalctl -u greetd --no-pager; stat -Lc '%u %a %n' / /nix /nix/store /run /run/current-system/sw/share/wayland-sessions; ${self.packages.${pkgs.stdenv.hostPlatform.system}.oxisession}/bin/oxisession --list-sessions; ps -eo user,pid,comm,args"))
          machine.screenshot("greeter-startup-failed")
          raise
      visible("Sign in")
      machine.succeed("${self.packages.${pkgs.stdenv.hostPlatform.system}.oxisession}/bin/oxisession --list-sessions | grep -F auth-fixture")
      machine.screenshot("greeter-${backend}")
      with subtest("pre-login power requires confirmation and Escape cancels"):
          machine.send_key("ctrl-shift-p")
          visible("Cancel")
          machine.screenshot("power-confirmation")
          machine.send_key("esc")
          time.sleep(0.3)
          assert "Cancel" not in machine.get_screen_text()
          machine.send_key("ctrl-a")
          machine.send_key("backspace")
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
          visible("Tab.*focus")
          machine.screenshot("retry-password-prompt")
          machine.send_key("tab")
          machine.send_chars("ignored")
          machine.send_key("shift-tab")
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

      machine.wait_for_file("/home/alice/auth-fixture/socket", timeout=60)
      socket = machine.succeed("cat /home/alice/auth-fixture/socket").strip()
      sway_socket = machine.succeed("printf '%s' /run/user/1000/sway-ipc.*.sock").strip()
      def alice(command):
          return machine.succeed("runuser -u alice -- env XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=" + shlex.quote(socket) + " SWAYSOCK=" + shlex.quote(sway_socket) + " sh -c " + shlex.quote(command))
      locker_command = "${self.packages.${pkgs.stdenv.hostPlatform.system}.oxilock}/bin/oxilock"
      sleep_command = "${self.packages.${pkgs.stdenv.hostPlatform.system}.oxilock}/bin/oxilock-sleep"
      def locker_pid():
          return machine.succeed("pgrep -u alice -f '/bin/oxilock --ready-fd 3$'").strip()
      def launch_locker():
          alice("rm -f ~/auth-fixture/ready; " + locker_command + " --ready-fd 3 3>~/auth-fixture/ready >~/auth-fixture/locker.log 2>&1 </dev/null &")
          machine.wait_until_succeeds("grep -x locked /home/alice/auth-fixture/ready", timeout=60)
          return locker_pid()
      def unlock():
          machine.send_chars("testpass\n")
          try:
              machine.wait_until_succeeds("! pgrep -u alice -f '/bin/oxilock --ready-fd 3$'", timeout=60)
          except Exception:
              print(machine.succeed("cat /home/alice/auth-fixture/locker.log; ps -eo user,pid,ppid,comm,args; journalctl --no-pager -n 80"))
              machine.screenshot("locker-unlock-failed")
              raise
      alice("mkdir -p ~/.config/oxibar; ln -s ${appearance} ~/.config/oxibar/auth.toml")

      with subtest("native protocol lock, fractional scaling, hotplug and current-user PAM"):
          locked = launch_locker()
          time.sleep(1)
          machine.screenshot("locker-themed-${backend}")
          alice("swaymsg 'output * scale 1.25'")
          alice("swaymsg create_output")
          outputs = json.loads(alice("swaymsg -r -t get_outputs"))
          headless = next(output["name"] for output in outputs if output["name"].startswith("HEADLESS"))
          alice("swaymsg 'output " + headless + " mode 1024x768 scale 1.5'")
          time.sleep(1)
          machine.screenshot("locker-fractional-${backend}")
          alice("swaymsg 'output " + headless + " disable'")
          machine.send_key("caps_lock")
          time.sleep(0.3)
          machine.screenshot("locker-caps-lock")
          machine.send_key("caps_lock")
          machine.send_chars("wrongpass\n")
          machine.wait_until_succeeds("journalctl --no-pager | grep -F 'pam_unix(oxilock:auth): authentication failure'", timeout=60)
          machine.wait_until_succeeds("! pgrep -P " + locked, timeout=60)
          machine.succeed("kill -0 " + locked)
          machine.screenshot("locker-rejected-authentication")
          machine.send_key("ret")
          machine.wait_until_succeeds("pgrep -P " + locked, timeout=60)
          time.sleep(0.3)
          machine.send_chars("mistakemistakemistake")
          machine.send_monitor_command("sendkey backspace 1800")
          time.sleep(2)
          unlock()
          alice("swaymsg 'output * scale 1'")

      if "${backend}" == "cage":
          def resume_s3():
              deadline = time.monotonic() + 60
              while "suspended" not in machine.send_monitor_command("info status"):
                  if time.monotonic() >= deadline:
                      print(machine.succeed("cat /home/alice/auth-fixture/suspend.log 2>/dev/null; systemd-inhibit --list --no-pager; journalctl --no-pager -n 60; true"))
                      machine.screenshot("sleep-failed")
                      raise AssertionError("VM did not enter S3")
                  time.sleep(0.1)
              machine.send_monitor_command("system_wakeup")
              time.sleep(2)

          with subtest("readiness-gated logind sleep and resume retain the native lock"):
              assert "mem" in machine.succeed("cat /sys/power/state")
              alice("systemctl --user import-environment WAYLAND_DISPLAY SWAYSOCK; systemctl --user start oxilock-sleep.service")
              machine.wait_until_succeeds("systemd-inhibit --list --no-pager | grep OxiLock", timeout=60)
              machine.succeed("systemctl suspend --no-block")
              resume_s3()
              machine.succeed("journalctl --no-pager | grep -F 'protocol lock ready; releasing delay inhibitor'")
              machine.wait_until_succeeds("pgrep -u alice -f '/bin/oxilock --ready-fd 3$'", timeout=60)
              resumed_lock = machine.succeed("pgrep -u alice -f '/bin/oxilock --ready-fd 3$'").strip()
              time.sleep(1)
              machine.screenshot("locker-after-resume")
              machine.wait_until_succeeds("systemd-inhibit --list --no-pager | grep OxiLock", timeout=60)
              alice("systemctl --user stop oxilock-sleep.service")
              machine.succeed("kill -0 " + resumed_lock)
              unlock()

          with subtest("explicit suspend starts only after protocol readiness"):
              alice("swaymsg " + shlex.quote("exec ${self.packages.${pkgs.stdenv.hostPlatform.system}.oxilock}/bin/oxilock-sleep --suspend >/home/alice/auth-fixture/suspend.log 2>&1"))
              resume_s3()
              machine.wait_until_succeeds("pgrep -u alice -f '/bin/oxilock --ready-fd 3$'", timeout=60)
              time.sleep(0.5)
              machine.screenshot("locker-explicit-suspend-resume")
              unlock()

      with subtest("locker crash keeps compositor protected and retires authentication worker"):
          crashed_lock = launch_locker()
          machine.wait_until_succeeds("pgrep -P " + crashed_lock, timeout=60)
          worker = machine.succeed("pgrep -P " + crashed_lock).strip()
          machine.succeed("kill -KILL " + crashed_lock)
          machine.wait_until_succeeds("! kill -0 " + crashed_lock + " 2>/dev/null && ! kill -0 " + worker + " 2>/dev/null", timeout=60)
          machine.succeed("pgrep -u alice -x sway")
          time.sleep(1)
          machine.screenshot("locker-crash-protected")

      with subtest("logout returns to a fresh greeter"):
          machine.succeed("pkill -u alice -x sway")
          try:
              machine.wait_until_succeeds("! pgrep -u alice -x sway && pgrep -x .oxisession-wra", timeout=60)
          except Exception:
              print(machine.succeed("journalctl -u greetd --no-pager; timeout 5 loginctl list-sessions || true; ps -eo user,pid,ppid,stat,wchan:32,args; dmesg | tail -n 50"))
              machine.screenshot("logout-failed")
              raise
          time.sleep(3)
          machine.screenshot("returned-greeter")

      with subtest("greeter crash restarts the kiosk without an authenticated session"):
          crashed = machine.succeed("pgrep -x .oxisession-wra").strip()
          machine.succeed("pkill -KILL -x .oxisession-wra")
          machine.wait_until_succeeds("! kill -0 " + crashed + " 2>/dev/null && pgrep -x .oxisession-wra", timeout=60)
          assert machine.succeed("pgrep -x .oxisession-wra").strip() != crashed
          visible("Sign in")
          machine.fail("pgrep -u alice -x sway")
          machine.screenshot("crash-recovery")
    '';
  }
