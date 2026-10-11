{ homeManagerPath, system ? builtins.currentSystem }:
let
  source = builtins.path {
    path = ../.;
    name = "oxibar-hm-test";
    filter = path: type: !(builtins.elem (builtins.baseNameOf path) [ ".git" ".direnv" "target" "tmp" ]);
  };
  flake = builtins.getFlake (builtins.unsafeDiscardStringContext (toString source));
  pkgs = import flake.inputs.nixpkgs { inherit system; };
  packages = flake.packages.${system};
  evaluate = startup: import (homeManagerPath + "/modules") {
    inherit pkgs;
    configuration = {
      imports = [ flake.homeManagerModules.default ];
      home.username = "oxibar-test";
      home.homeDirectory = "/home/oxibar-test";
      home.stateVersion = "25.11";
      xdg.configHome = "/home/oxibar-test/custom-config";
      programs.oxibar = {
        enable = true;
        config.plugins = [ packages.oxibar-clock ];
        extraPlugins = [ packages.oxibar-example ];
      } // pkgs.lib.optionalAttrs startup {
        systemd.enable = true;
        systemd.target = "hyprland-session.target";
      };
    };
  };
  disabled = (evaluate false).config;
  enabled = (evaluate true).config;
  base = builtins.fromTOML (builtins.readFile disabled.xdg.configFile."oxibar/config.toml".source);
  checks = {
    allModuleAssertions = builtins.all (item: item.assertion) (disabled.assertions ++ enabled.assertions);
    startupIsOptIn = !disabled.programs.oxibar.systemd.enable && !(builtins.hasAttr "oxibar" disabled.systemd.user.services);
    enabledBaseOnly = base.plugins == [ "libclock.so" ];
    disabledCatalogInstalled = builtins.hasAttr "oxibar/plugins/libexample.so" disabled.xdg.configFile;
    overridesUnmanaged = !(builtins.hasAttr "oxibar/overrides.toml" disabled.xdg.configFile);
    ownedSessionTarget = enabled.systemd.user.services.oxibar.Unit.PartOf == [ "hyprland-session.target" ]
      && enabled.systemd.user.services.oxibar.Install.WantedBy == [ "hyprland-session.target" ];
    requiresWayland = enabled.systemd.user.services.oxibar.Unit.ConditionEnvironment == "WAYLAND_DISPLAY";
    shellHasOwnCgroup = enabled.systemd.user.services.oxibar.Service.KillMode == "control-group"
      && enabled.systemd.user.services.oxibar.Service.Slice == "app.slice";
  };
in
assert builtins.all (value: value) (builtins.attrValues checks);
checks
