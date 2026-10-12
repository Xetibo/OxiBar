{
  homeManagerPath,
  system ? builtins.currentSystem,
}: let
  source = builtins.path {
    path = ../.;
    name = "oxibar-hm-test";
    filter = path: type: !(builtins.elem (builtins.baseNameOf path) [".git" ".direnv" "target" "tmp"]);
  };
  flake = builtins.getFlake (builtins.unsafeDiscardStringContext (toString source));
  pkgs = import flake.inputs.nixpkgs {inherit system;};
  packages = flake.packages.${system};
  evaluate = {
    startup ? false,
    settings ? {},
    module ? flake.homeManagerModules.default,
  }:
    import (homeManagerPath + "/modules") {
      inherit pkgs;
      configuration = {
        imports = [module];
        home.username = "oxibar-test";
        home.homeDirectory = "/home/oxibar-test";
        home.stateVersion = "25.11";
        xdg.configHome = "/home/oxibar-test/custom-config";
        programs.oxibar =
          {
            enable = true;
          }
          // settings
          // pkgs.lib.optionalAttrs startup {
            systemd.enable = true;
            systemd.target = "hyprland-session.target";
          };
      };
    };
  customSettings = {
    config.plugins = [packages.oxibar-clock];
    extraPlugins = [packages.oxibar-example];
  };
  disabled = (evaluate {settings = customSettings;}).config;
  enabled =
    (evaluate {
      startup = true;
      settings = customSettings;
    }).config;
  defaults = (evaluate {module = flake.nixosModules.home-manager;}).config;
  base = builtins.fromTOML (builtins.readFile disabled.xdg.configFile."oxibar/config.toml".source);
  defaultBase = builtins.fromTOML (builtins.readFile defaults.xdg.configFile."oxibar/config.toml".source);
  checks = {
    allModuleAssertions = builtins.all (item: item.assertion) (disabled.assertions ++ enabled.assertions ++ defaults.assertions);
    startupIsOptIn =
      builtins.all (configuration:
        !configuration.programs.oxibar.systemd.enable && !(builtins.hasAttr "oxibar" configuration.systemd.user.services)) [disabled defaults];
    defaultClipboardEnabled =
      builtins.elem "liboxipaste.so" defaultBase.plugins
      && builtins.elem (toString packages.oxibar-oxipaste) (builtins.map toString defaults.home.packages);
    defaultClipboardInstalled =
      defaults.xdg.configFile."oxibar/plugins/liboxipaste.so".target
      == "custom-config/oxibar/plugins/liboxipaste.so"
      && toString defaults.xdg.configFile."oxibar/plugins/liboxipaste.so".source == "${packages.oxibar-oxipaste}/lib/liboxipaste.so";
    enabledBaseOnly = base.plugins == ["libclock.so"];
    disabledCatalogInstalled = builtins.hasAttr "oxibar/plugins/libexample.so" disabled.xdg.configFile;
    overridesUnmanaged = !(builtins.hasAttr "oxibar/overrides.toml" disabled.xdg.configFile);
    ownedSessionTarget =
      enabled.systemd.user.services.oxibar.Unit.PartOf
      == ["hyprland-session.target"]
      && enabled.systemd.user.services.oxibar.Install.WantedBy == ["hyprland-session.target"];
    requiresWayland = enabled.systemd.user.services.oxibar.Unit.ConditionEnvironment == "WAYLAND_DISPLAY";
    shellHasOwnCgroup =
      enabled.systemd.user.services.oxibar.Service.KillMode
      == "control-group"
      && enabled.systemd.user.services.oxibar.Service.Slice == "app.slice";
  };
in
  assert builtins.all (value: value) (builtins.attrValues checks); checks
