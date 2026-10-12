self: { config, lib, pkgs, ... }:
let cfg = config.programs.oxilock;
in {
  options.programs.oxilock = {
    enable = lib.mkEnableOption "the OxiLock user application (system PAM policy is separate)";
    package = lib.mkOption { type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.oxilock; };
  };
  config = lib.mkIf cfg.enable { home.packages = [ cfg.package ]; };
}
