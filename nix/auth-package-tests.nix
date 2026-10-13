{
  pkgs,
  self,
}: let
  host = pkgs.extend (_: previous: {
    pam = previous.pam.overrideAttrs (_: {pname = "host-auth-pam-fixture";});
    libglvnd = previous.libglvnd.overrideAttrs (_: {pname = "host-auth-egl-fixture";});
  });
  expected = self.lib.mkAuthenticationPackages host;
  system = import (host.path + "/nixos/lib/eval-config.nix") {
    pkgs = host;
    system = host.stdenv.hostPlatform.system;
    modules = [
      self.nixosModules.authentication
      {
        programs.oxilock.enable = true;
        services.oxisession.enable = true;
      }
    ];
  };
  cage = system.extendModules {
    modules = [{services.oxisession.backend = "cage";}];
  };
in
  assert system.config.programs.oxilock.package.drvPath == expected.oxilock.drvPath;
  assert system.config.services.oxisession.package.drvPath == expected.oxisession.drvPath;
  assert expected.oxilock.drvPath != self.packages.${host.stdenv.hostPlatform.system}.oxilock.drvPath;
  assert expected.oxisession.drvPath != self.packages.${host.stdenv.hostPlatform.system}.oxisession.drvPath;
  assert system.config.hardware.graphics.enable;
  assert system.config.services.oxisession.backend == "hyprland";
  assert pkgs.lib.hasSuffix "--backend hyprland" system.config.services.greetd.settings.default_session.command;
  assert pkgs.lib.hasSuffix "--backend cage" cage.config.services.greetd.settings.default_session.command;
    pkgs.runCommand "authentication-system-package-set" {} ''touch "$out"''
