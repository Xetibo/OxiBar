self: { config, lib, pkgs, ... }:
let cfg = config.programs.oxilock; format = pkgs.formats.toml {};
in {
  options.programs.oxilock = {
    enable = lib.mkEnableOption "the OxiLock user application (system PAM policy is separate)";
    package = lib.mkOption { type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.oxilock; };
    settings = lib.mkOption { type = format.type; default = {}; description = "User-owned authentication appearance and theme tokens."; };
    sleepIntegration = lib.mkEnableOption "readiness-gated logind sleep locking";
  };
  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];
    xdg.configFile."oxibar/auth.toml".source = format.generate "oxibar-auth.toml" cfg.settings;
    systemd.user.services.oxilock-sleep = lib.mkIf cfg.sleepIntegration {
      Unit = { Description = "OxiLock readiness-gated sleep locking"; After = [ "graphical-session.target" ]; PartOf = [ "graphical-session.target" ]; };
      Service = { ExecStart = "${cfg.package}/bin/oxilock-sleep --watch"; Restart = "on-failure"; RestartSec = 2; LimitCORE = 0; KillMode = "process"; };
      Install.WantedBy = [ "graphical-session.target" ];
    };
  };
}
