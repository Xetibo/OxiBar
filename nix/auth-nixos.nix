self: {
  config,
  lib,
  pkgs,
  ...
}: let
  authentication = self.lib.mkAuthenticationPackages pkgs;
  lock = config.programs.oxilock;
  session = config.services.oxisession;
  format = pkgs.formats.toml {};
in {
  options = {
    programs.oxilock = {
      enable = lib.mkEnableOption "the protocol-native OxiLock locker and PAM policy";
      package = lib.mkOption {
        type = lib.types.package;
        default = authentication.oxilock;
        description = "Locker built against the system package set, including its PAM ABI.";
      };
      sleepIntegration = lib.mkEnableOption "readiness-gated logind sleep locking for graphical user sessions";
    };
    services.oxisession = {
      enable = lib.mkEnableOption "the greetd-backed OxiSession kiosk greeter";
      package = lib.mkOption {
        type = lib.types.package;
        default = authentication.oxisession;
        description = "Greeter and kiosk compositors built against the system graphics ABI.";
      };
      backend = lib.mkOption {
        type = lib.types.enum ["hyprland" "cage"];
        default = "hyprland";
        description = "Owned kiosk compositor; Hyprland is the default, with Cage available explicitly.";
      };
      settings = lib.mkOption {
        type = format.type;
        default = {};
        description = "Administrator-owned greeter appearance and theme tokens.";
      };
      keyboardLayout = lib.mkOption {
        type = lib.types.strMatching "[a-zA-Z0-9_,+-]{1,64}";
        default = "us";
        description = "Enforced kiosk keyboard layout, also displayed in the greeter.";
      };
      sessionPackages = lib.mkOption {
        type = lib.types.listOf lib.types.package;
        default = config.services.displayManager.sessionPackages;
        description = "Administrator-installed Wayland session desktop entries and executables.";
      };
    };
  };
  config = lib.mkMerge [
    (lib.mkIf (session.enable || (lock.enable && lock.sleepIntegration)) {
      security.polkit.enable = true;
    })
    (lib.mkIf lock.enable {
      environment.systemPackages = [lock.package];
      security.pam.services.oxilock = {allowNullPassword = false;};
      systemd.user.services.oxilock-sleep = lib.mkIf lock.sleepIntegration {
        description = "OxiLock readiness-gated sleep locking";
        wantedBy = ["graphical-session.target"];
        after = ["graphical-session.target"];
        partOf = ["graphical-session.target"];
        serviceConfig = {
          ExecStart = "${lock.package}/bin/oxilock-sleep --watch";
          Restart = "on-failure";
          RestartSec = 2;
          LimitCORE = 0;
          KillMode = "process";
        };
      };
    })
    (lib.mkIf session.enable {
      hardware.graphics.enable = true;
      environment.systemPackages = [session.package] ++ session.sessionPackages;
      environment.pathsToLink = ["/share/wayland-sessions"];
      users.users.oxisession = {
        isSystemUser = true;
        group = "oxisession";
        home = "/var/lib/oxisession";
        createHome = true;
      };
      users.groups.oxisession = {};
      services.greetd = {
        enable = true;
        settings.default_session = {
          user = "oxisession";
          command = "${pkgs.coreutils}/bin/env ${lib.escapeShellArg "OXI_KEYBOARD_LAYOUT=${session.keyboardLayout}"} ${lib.escapeShellArg "XKB_DEFAULT_LAYOUT=${session.keyboardLayout}"} ${session.package}/bin/oxisession-kiosk --backend ${session.backend}";
        };
      };
      security.pam.services.greetd.allowNullPassword = lib.mkForce false;
      systemd.services.greetd.serviceConfig.LimitCORE = 0;
      environment.etc."oxibar/auth.toml".source = format.generate "oxibar-greeter-auth.toml" session.settings;
    })
  ];
}
