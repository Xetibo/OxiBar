{
  pkgs,
  crane,
  source,
}: let
  craneLib = crane.mkLib pkgs;
  lockFile = builtins.path {
    path = source + "/Cargo.lock";
    name = "Cargo.lock";
  };
  src = pkgs.lib.cleanSourceWith {
    src = source;
    filter = path: type:
      craneLib.filterCargoSources path type
      || builtins.match ".*/src/overview/protocols/[^/]+\\.xml" path != null;
  };
  layershellevSource = builtins.path {
    path = source + "/crates/layershellev/src";
    name = "oxibar-layershellev-source";
  };
  cargoToml = builtins.fromTOML (builtins.readFile (source + "/Cargo.toml"));
  cargoVendorDir = craneLib.vendorCargoDeps {
    inherit src;
    cargoLock = lockFile;
  };
  cargoArtifacts = craneLib.buildDepsOnly {
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
    postPatch = ''cp -R ${layershellevSource}/. crates/layershellev/src/'';
    cargoBuildExtraArgs = "-p oxi-lock -p oxi-session";
    cargoTestExtraArgs = "-p oxi-lock -p oxi-session --no-run";
  };
  cage =
    if pkgs.lib.versionOlder pkgs.cage.version "0.3"
    then
      pkgs.cage.overrideAttrs (old: {
        patches = (old.patches or []) ++ [./cage-initial-fullscreen.patch];
        postPatch =
          (old.postPatch or "")
          + ''
            substituteInPlace cage.c --replace-fail 'wlr_xdg_shell_create(server.wl_display, 5)' 'wlr_xdg_shell_create(server.wl_display, 6)'
          '';
      })
    else pkgs.cage;
  package = package: binary:
    pkgs.callPackage ./auth-package.nix {
      inherit craneLib src cargoVendorDir cargoArtifacts lockFile cage package binary;
    };
in {
  oxilock = package "oxi-lock" "oxilock";
  oxisession = package "oxi-session" "oxisession";
}
