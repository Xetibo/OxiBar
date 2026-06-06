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
    oxibar-battery
    oxibar-bluetooth
    oxibar-clock
    oxibar-network
    oxibar-notifications
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
        name = ".config/oxibar/plugins/${pluginLibraryName pkg}";
        value = {
          source = "${pkg}/lib/${pluginLibraryName pkg}";
        };
      })
      cfg.config.plugins);
  in
    lib.mkIf
    cfg.enable
    {
      home.packages = lib.optional (cfg.package != null) cfg.package ++ cfg.config.plugins;
      home.file = plugin_files;

      xdg.configFile."oxibar/config.toml".source =
        (pkgs.formats.toml {}).generate "oxibar"
        (lib.recursiveUpdate
          {
            plugins = fetchedPlugins;
          }
          cfg.config.plugin_config);
    };
}
