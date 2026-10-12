self: { config, lib, pkgs, ... }:
let
  system = pkgs.stdenv.hostPlatform.system;
  lock = config.programs.oxilock;
  session = config.services.oxisession;
in {
  options = {
    programs.oxilock = {
      enable = lib.mkEnableOption "the protocol-native OxiLock locker and PAM policy";
      package = lib.mkOption { type = lib.types.package; default = self.packages.${system}.oxilock; };
    };
    services.oxisession = {
      enable = lib.mkEnableOption "the greetd-backed OxiSession kiosk greeter";
      package = lib.mkOption { type = lib.types.package; default = self.packages.${system}.oxisession; };
      backend = lib.mkOption { type = lib.types.enum [ "cage" "hyprland" ]; default = "cage"; };
      sessionPackages = lib.mkOption { type = lib.types.listOf lib.types.package;
        default = config.services.displayManager.sessionPackages;
        description = "Administrator-installed Wayland session desktop entries and executables."; };
    };
  };
  config = lib.mkMerge [
    (lib.mkIf lock.enable {
      environment.systemPackages = [ lock.package ];
      security.pam.services.oxilock = { allowNullPassword = false; };
    })
    (lib.mkIf session.enable {
      environment.systemPackages = [ session.package ] ++ session.sessionPackages;
      environment.pathsToLink = [ "/share/wayland-sessions" ];
      users.users.oxisession = { isSystemUser = true; group = "oxisession";
        home = "/var/lib/oxisession"; createHome = true; };
      users.groups.oxisession = {};
      services.greetd = { enable = true; settings.default_session = {
        user = "oxisession";
        command = "${session.package}/bin/oxisession-kiosk --backend ${session.backend}";
      }; };
      security.pam.services.greetd.allowNullPassword = lib.mkForce false;
      systemd.services.greetd.serviceConfig.LimitCORE = 0;
    })
  ];
}
