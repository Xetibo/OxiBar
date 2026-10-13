{
  description = "A Rust/Iced Wayland widget shell with resident plugins";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs = inputs @ {
    self,
    flake-parts,
    ...
  }:
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      perSystem = {
        self',
        pkgs,
        system,
        ...
      }: {
        _module.args.pkgs = import self.inputs.nixpkgs {
          inherit system;
        };
        devShells.default = let
          commonPkgs = with pkgs; [
            cargo
            cargo-watch
            rustc
            rust-analyzer
            clippy
            rustfmt
            curl
            wtype
          ];
          # Fonts shipped into the dev shell. Fontconfig is the canonical
          # discovery mechanism on Linux; `adwaita-fonts` provides the
          # `Adwaita Sans` family that is the host's default in
          # `[bar] font`. Without these, fontdb's hardcoded scan paths
          # (`/usr/share/fonts`, `~/.fonts`, ...) miss anything in
          # `~/.nix-profile/share/fonts/` or home-manager's
          # `home-manager-path/share/fonts/`, and the bar silently falls
          # back to whatever cosmic-text picks as a substitute.
          fontPkgs = with pkgs; [
            fontconfig
            adwaita-fonts
          ];
          # Runtime libs needed for iced/wgpu to come up. Mirrors the
          # `postFixup` rpath in OxiShut's `nix/default.nix` (commit reference
          # in `/home/dashie/gits/OxiShut/nix/default.nix`): `vulkan-loader`
          # provides `libvulkan.so.1` (wgpu's preferred backend) and the X
          # libs are needed because winit / wayland-client still link against
          # them indirectly. Without `vulkan-loader` here the dev-shell run
          # logs `Available adapters: []` from `iced_wgpu` and the bar
          # silently renders as a blank surface — which looks like "no font
          # loaded" but is really "no GPU adapter".
          libPath = with pkgs;
            lib.makeLibraryPath ([
                libGL
                libglvnd
                vulkan-loader
                libxkbcommon
                wayland
                wayland-protocols
                libdrm
                libpulseaudio
                pam
                xorg.libX11
                xorg.libXrandr
                xorg.libXi
                xorg.libXcursor
                libclang.lib
              ]
              ++ commonPkgs);
          # Vulkan ICD JSONs. wgpu's Vulkan backend reads this to locate the
          # actual driver. On NixOS the system also exposes them under
          # `/run/opengl-driver/share/vulkan/icd.d`, but pinning to `mesa`
          # makes the dev shell work on non-NixOS hosts that have flakes +
          # nix-shell but no `/run/opengl-driver` tree.
          icdArch =
            if system == "x86_64-linux"
            then "x86_64"
            else "aarch64";
          vkIcdFiles = with pkgs;
            lib.concatStringsSep ":" [
              "${mesa}/share/vulkan/icd.d/radeon_icd.${icdArch}.json"
              "${mesa}/share/vulkan/icd.d/intel_icd.${icdArch}.json"
              "${mesa}/share/vulkan/icd.d/nouveau_icd.${icdArch}.json"
              "${mesa}/share/vulkan/icd.d/lvp_icd.${icdArch}.json"
            ];
          # Build a fontconfig config that knows about `fontPkgs`'s
          # `share/fonts` directories. `makeFontsConf` writes a fonts.conf
          # whose `<dir>` entries point at each input package, so
          # fontconfig (and fontdb's `fontconfig-parser`) resolve our
          # bundled fonts even on non-NixOS hosts where `/etc/fonts/fonts.conf`
          # may be missing or wrong.
          fontsConf = pkgs.makeFontsConf {
            fontDirectories = fontPkgs;
          };
        in
          pkgs.mkShell {
            buildInputs = [pkgs.pam];
            inputsFrom = builtins.attrValues self'.packages;
            packages = commonPkgs ++ fontPkgs;
            LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
            shellHook =
              (import ./nix/application-environment.nix)
              + ''
                export LD_LIBRARY_PATH="${libPath}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
                export FONTCONFIG_FILE="${fontsConf}"
                export VK_ICD_FILENAMES="${vkIcdFiles}"
              '';
          };

        packages = let
          craneLib = inputs.crane.mkLib pkgs;
          lockFile = ./Cargo.lock;
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              craneLib.filterCargoSources path type
              || builtins.match ".*/src/overview/protocols/[^/]+\\.xml" path != null;
          };
          layershellevSource = builtins.path {
            path = ./crates/layershellev/src;
            name = "oxibar-layershellev-source";
          };
          cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
          cargoVendorDir = craneLib.vendorCargoDeps {
            inherit src;
            cargoLock = lockFile;
          };
          commonCraneArgs = {
            pname = "oxibar-workspace";
            inherit (cargoToml.package) version;
            inherit src cargoVendorDir;
            cargoLock = lockFile;
            buildInputs = with pkgs; [
              libGL
              libglvnd
              libxkbcommon
              wayland
              wayland-protocols
              libdrm
              libpulseaudio
              pam
              libclang.lib
              vulkan-loader
              xorg.libX11
              xorg.libXrandr
              xorg.libXi
              xorg.libXcursor
              mesa
            ];
            nativeBuildInputs = [pkgs.pkg-config];
            LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
          };
          cargoArtifacts = craneLib.buildDepsOnly (commonCraneArgs
            // {
              # Registry iced_layershell consumes this local patched dependency.
              # Keep its API source in Crane's otherwise stubbed dependency build.
              postPatch = ''
                cp -R ${layershellevSource}/. crates/layershellev/src/
              '';
              cargoBuildExtraArgs = "--workspace";
              cargoCheckExtraArgs = "--workspace --all-targets";
              cargoTestExtraArgs = "--workspace --no-run";
            });
          pluginPackage = plugin:
            pkgs.callPackage ./nix/plugin.nix {
              inherit craneLib src cargoVendorDir cargoArtifacts lockFile plugin;
            };
          authentication = self.lib.mkAuthenticationPackages pkgs;
        in rec {
          oxibar = pkgs.callPackage ./nix/default.nix {
            inherit inputs craneLib src cargoVendorDir cargoArtifacts lockFile;
            applicationsPlugin = oxibar-applications;
          };
          inherit (authentication) oxilock oxisession;
          oxibar-audio = pluginPackage "audio";
          oxibar-applications = pluginPackage "applications";
          oxibar-battery = pluginPackage "battery";
          oxibar-bluetooth = pluginPackage "bluetooth";
          oxibar-clock = pluginPackage "clock";
          oxibar-commands = pluginPackage "commands";
          oxibar-network = pluginPackage "network";
          oxibar-notifications = pluginPackage "notifications";
          oxibar-oxipaste = pluginPackage "oxipaste";
          oxibar-system = pluginPackage "system";
          oxibar-stats = pluginPackage "stats";
          oxibar-tools = pluginPackage "tools";
          oxibar-example = pluginPackage "example";
          oxibar-tray = pluginPackage "tray";
          oxibar-workspaces = pluginPackage "workspaces";
          default = oxibar;
        };
        checks =
          {
            authentication-platform = import ./nix/auth-package-tests.nix {inherit pkgs self;};
            authentication-clippy = self'.packages.oxisession.clippy;
            authentication-unit = self'.packages.oxisession.authenticationTests;
          }
          // pkgs.lib.optionalAttrs (system == "x86_64-linux") {
            authentication-cage = import ./nix/auth-vm-tests.nix {
              inherit pkgs;
              self = inputs.self;
              backend = "cage";
            };
            authentication-hyprland = import ./nix/auth-vm-tests.nix {
              inherit pkgs;
              self = inputs.self;
              backend = "hyprland";
            };
          };
      };
      flake = _: rec {
        lib.mkAuthenticationPackages = pkgs:
          import ./nix/auth-packages.nix {
            inherit pkgs;
            crane = inputs.crane;
            source = self.outPath;
          };
        nixosModules.home-manager = homeManagerModules.default;
        nixosModules.authentication = import ./nix/auth-nixos.nix self;
        homeManagerModules = rec {
          oxibar = import ./nix/hm.nix inputs.self;
          oxilock = import ./nix/auth-hm.nix self;
          default = oxibar;
        };
      };
    };
}
