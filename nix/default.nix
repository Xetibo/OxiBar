{
  craneLib,
  stdenv,
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
  libx11,
  libxrandr,
  libxi,
  libxcursor,
  mesa,
  makeWrapper,
  makeFontsConf,
  fontconfig,
  xdg-utils,
  wtype,
  curl,
  adwaita-fonts,
  applicationsPlugin,
  ...
}: let
  cargoToml = builtins.fromTOML (builtins.readFile ../Cargo.toml);
  runtimeLibPath = lib.makeLibraryPath [
    libGL
    libglvnd
    vulkan-loader
    wayland
    wayland-protocols
    libxkbcommon
    libdrm
    libx11
    libxrandr
    libxi
    libxcursor
    libclang.lib
    mesa
  ];
  driverIcdPath = "${mesa}/share/vulkan/icd.d";
  icdArch =
    if stdenv.hostPlatform.system == "x86_64-linux"
    then "x86_64"
    else if stdenv.hostPlatform.system == "aarch64-linux"
    then "aarch64"
    else "x86_64";
  vkIcdFiles = lib.concatStringsSep ":" [
    "${driverIcdPath}/radeon_icd.${icdArch}.json"
    "${driverIcdPath}/intel_icd.${icdArch}.json"
    "${driverIcdPath}/nouveau_icd.${icdArch}.json"
    "${driverIcdPath}/lvp_icd.${icdArch}.json"
  ];
  fontsConf = makeFontsConf {
    fontDirectories = [adwaita-fonts];
  };
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
      libx11
      libxrandr
      libxi
      libxcursor
      mesa
    ];

    nativeBuildInputs = [
      pkg-config
      makeWrapper
    ];

    preCheck = ''
      export XDG_CONFIG_HOME="$TMPDIR/xdg-config"
      export LD_LIBRARY_PATH="${runtimeLibPath}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
      mkdir -p "$XDG_CONFIG_HOME"
    '';

    LIBCLANG_PATH = "${libclang.lib}/lib";
    VK_ICD_FILENAMES = vkIcdFiles;

    postFixup = ''
      patchelf --set-rpath "${runtimeLibPath}" "$out/bin/oxibar"
      wrapProgram "$out/bin/oxibar" \
        --run ${lib.escapeShellArg (import ./application-environment.nix)} \
        --prefix PATH : "${lib.makeBinPath [fontconfig xdg-utils wtype curl]}" \
        --set FONTCONFIG_FILE "${fontsConf}" \
        --set-default OXIBAR_PLUGIN_DIR "${applicationsPlugin}/lib" \
        --set VK_ICD_FILENAMES "${vkIcdFiles}"
    '';

    meta = with lib; {
      description = cargoToml.package.description;
      homepage = "https://github.com/Xetibo/oxibar";
      changelog = "https://github.com/Xetibo/oxibar/releases/tag/${version}";
      license = licenses.gpl3Only;
      maintainers = with maintainers; [DashieTM];
      mainProgram = "oxibar";
    };
  }
