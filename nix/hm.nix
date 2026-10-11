self: {
  config,
  pkgs,
  lib,
  ...
}: let
  cfg = config.programs.oxibar;
  system = pkgs.stdenv.hostPlatform.system;
  defaultPackage = self.packages.${system}.default;
  defaultPlugins = with self.packages.${system}; [
    oxibar-audio
    oxibar-applications
    oxibar-battery
    oxibar-bluetooth
    oxibar-clock
    oxibar-network
    oxibar-notifications
    oxibar-system
    oxibar-stats
    oxibar-tools
    oxibar-tray
    oxibar-workspaces
  ];
  pluginLibraryName = pkg: "lib${lib.replaceStrings ["-"] ["_"] pkg.pname}.so";
in {
  meta.maintainers = with lib.maintainers; [DashieTM];
  options.programs.oxibar = with lib; {
    enable = mkEnableOption "oxibar";

    package = mkOption {
      type = with types; nullOr package;
      default = defaultPackage;
      defaultText = lib.literalExpression ''
        oxibar.packages.''${pkgs.stdenv.hostPlatform.system}.default
      '';
      description = mdDoc ''
        Package to run.
      '';
    };

    extraPlugins = mkOption {
      type = with types; listOf package;
      default = [];
      description = mdDoc ''
        Additional plugin libraries installed in the GUI catalog without enabling
        them in the immutable base configuration. GUI enablement requires restart.
      '';
    };

    systemd = {
      enable = mkEnableOption "the Oxibar user service";
      target = mkOption {
        type = types.str;
        default = "graphical-session.target";
        description = "Session target that owns the opt-in user service.";
      };
    };

    config = {
      plugins = mkOption {
        type = with types; listOf package;
        default = defaultPlugins;
        example = [];
        description = mdDoc ''
          List of plugins to use, represented as a list of packages.
        '';
      };

      plugin_config = mkOption {
        type = with types; attrs;
        default = {};
        description = mdDoc ''
          TOML values passed to the configuration for plugins to use.
        '';
      };
    };
  };
  config = let
    fetchedPlugins = builtins.map pluginLibraryName cfg.config.plugins;
    plugin_files = builtins.listToAttrs (builtins.map
      (pkg: {
        name = "oxibar/plugins/${pluginLibraryName pkg}";
        value = {
          source = "${pkg}/lib/${pluginLibraryName pkg}";
        };
      })
      (lib.unique (cfg.config.plugins ++ cfg.extraPlugins)));
  in
    lib.mkIf
    cfg.enable
    {
      assertions = [{
        assertion = !cfg.systemd.enable || cfg.package != null;
        message = "programs.oxibar.systemd.enable requires a non-null package.";
      }];
      home.packages = lib.optional (cfg.package != null) cfg.package ++ cfg.config.plugins ++ cfg.extraPlugins;
      xdg.configFile = plugin_files // {
        "oxibar/config.toml".source =
        (pkgs.formats.toml {}).generate "oxibar"
        (lib.recursiveUpdate
          {
            plugins = fetchedPlugins;
          }
          cfg.config.plugin_config);
      };
      systemd.user.services.oxibar = lib.mkIf cfg.systemd.enable {
        Unit = {
          Description = "Oxibar Rust/Iced widget shell";
          After = [cfg.systemd.target];
          PartOf = [cfg.systemd.target];
          ConditionEnvironment = "WAYLAND_DISPLAY";
        };
        Service = {
          ExecStart = lib.getExe cfg.package;
          Restart = "on-failure";
          RestartSec = 2;
          TimeoutStopSec = 10;
          KillMode = "control-group";
          Slice = "app.slice";
        };
        Install.WantedBy = [cfg.systemd.target];
      };
    };
}
