{
  craneLib,
  pkg-config,
  libGL,
  libglvnd,
  libxkbcommon,
  wayland,
  wayland-protocols,
  libdrm,
  libclang,
  lib,
  lockFile,
  cargoArtifacts,
  cargoVendorDir,
  src,
  vulkan-loader,
  xorg,
  mesa,
  plugin,
  ...
}: let
  pluginDir = ../plugins + "/${plugin}";
  cargoToml = builtins.fromTOML (builtins.readFile (pluginDir + "/Cargo.toml"));
  pluginLibName = "lib${lib.replaceStrings ["-"] ["_"] cargoToml.package.name}.so";
  runtimeLibPath = lib.makeLibraryPath [
    libGL
    libglvnd
    vulkan-loader
    wayland
    wayland-protocols
    libxkbcommon
    libdrm
    xorg.libX11
    xorg.libXrandr
    xorg.libXi
    xorg.libXcursor
    libclang.lib
    mesa
  ];
in
  craneLib.buildPackage rec {
    pname = cargoToml.package.name;
    inherit (cargoToml.package) version;

    inherit src cargoArtifacts cargoVendorDir;
    cargoLock = lockFile;
    cargoBuildExtraArgs = "-p ${pname}";
    cargoTestExtraArgs = "-p ${pname}";

    buildInputs = [
      pkg-config
      libGL
      libglvnd
      libxkbcommon
      wayland
      wayland-protocols
      libdrm
      libclang.lib
      vulkan-loader
      xorg.libX11
      xorg.libXrandr
      xorg.libXi
      xorg.libXcursor
      mesa
    ];

    nativeBuildInputs = [pkg-config];
    preCheck = ''
      export XDG_CONFIG_HOME="$TMPDIR/xdg-config"
      mkdir -p "$XDG_CONFIG_HOME"
    '';

    LIBCLANG_PATH = "${libclang.lib}/lib";

    installPhaseCommand = ''
      plugin_lib=$(find target -name "${pluginLibName}" -type f -print -quit)
      if [ -z "$plugin_lib" ]; then
        echo "could not find ${pluginLibName}"
        exit 1
      fi
      install -Dm755 "$plugin_lib" "$out/lib/${pluginLibName}"
    '';

    postFixup = ''
      patchelf --set-rpath "${runtimeLibPath}" "$out/lib/${pluginLibName}"
    '';

    meta = with lib; {
      description = "${pname} plugin for Oxibar";
      homepage = "https://github.com/Xetibo/oxibar";
      changelog = "https://github.com/Xetibo/oxibar/releases/tag/${version}";
      license = licenses.gpl3Only;
      maintainers = with maintainers; [DashieTM];
    };
  }
