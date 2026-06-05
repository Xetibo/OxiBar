{
  description = "A simple bar made with Iced";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
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
            inputsFrom = builtins.attrValues self'.packages;
            packages = commonPkgs ++ fontPkgs;
            LD_LIBRARY_PATH = libPath;
            LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
            # Force fontdb / fontconfig-parser to read our shell-local
            # fonts.conf instead of guessing at the host system's.
            FONTCONFIG_FILE = fontsConf;
            # Point wgpu's Vulkan loader at Mesa's ICDs. See `vkIcdFiles`
            # above for rationale.
            VK_ICD_FILENAMES = vkIcdFiles;
          };

        packages = let
          lockFile = ./Cargo.lock;
          pluginPackage = plugin:
            pkgs.callPackage ./nix/plugin.nix {
              inherit lockFile plugin;
            };
        in rec {
          oxibar = pkgs.callPackage ./nix/default.nix {inherit inputs lockFile;};
          oxibar-audio = pluginPackage "audio";
          oxibar-bluetooth = pluginPackage "bluetooth";
          oxibar-clock = pluginPackage "clock";
          oxibar-network = pluginPackage "network";
          oxibar-notifications = pluginPackage "notifications";
          oxibar-tray = pluginPackage "tray";
          oxibar-workspaces = pluginPackage "workspaces";
          default = oxibar;
        };
      };
      flake = _: rec {
        nixosModules.home-manager = homeManagerModules.default;
        homeManagerModules = rec {
          oxibar = import ./nix/hm.nix inputs.self;
          default = oxibar;
        };
      };
    };
}
