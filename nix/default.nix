{
  rustPlatform,
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
  vulkan-loader,
  xorg,
  mesa,
  makeWrapper,
  makeFontsConf,
  fontconfig,
  adwaita-fonts,
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
    xorg.libX11
    xorg.libXrandr
    xorg.libXi
    xorg.libXcursor
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
  rustPlatform.buildRustPackage rec {
    pname = cargoToml.package.name;
    inherit (cargoToml.package) version;

    src = ../.;
    cargoBuildFlags = ["-p" pname];
    cargoTestFlags = ["-p" pname];

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

    cargoLock = {
      inherit lockFile;
      outputHashes = {
        "oxiced-0.5.1" = "sha256-z7Dl9G6qBF5KNzlNccnFZOh7HkAid7nPoS2AEDhzG5c=";
      };
    };

    nativeBuildInputs = [
      pkg-config
      makeWrapper
    ];

    preCheck = ''
      export XDG_CONFIG_HOME="$TMPDIR/xdg-config"
      mkdir -p "$XDG_CONFIG_HOME"
    '';

    LIBCLANG_PATH = "${libclang.lib}/lib";
    VK_ICD_FILENAMES = vkIcdFiles;

    postFixup = ''
      patchelf --set-rpath "${runtimeLibPath}" "$out/bin/oxibar"
      wrapProgram "$out/bin/oxibar" \
        --prefix PATH : "${lib.makeBinPath [fontconfig]}" \
        --set FONTCONFIG_FILE "${fontsConf}" \
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
