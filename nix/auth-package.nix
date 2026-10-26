{
  craneLib,
  src,
  cargoVendorDir,
  cargoArtifacts,
  lockFile,
  package,
  binary,
  lib,
  pkg-config,
  makeWrapper,
  libGL,
  libglvnd,
  libxkbcommon,
  wayland,
  libdrm,
  pam,
  vulkan-loader,
  libx11,
  libxrandr,
  libxi,
  libxcursor,
  makeFontsConf,
  adwaita-fonts,
  cage,
  hyprland,
}: let
  runtime = lib.makeLibraryPath [
    libGL
    libglvnd
    libxkbcommon
    wayland
    libdrm
    pam
    vulkan-loader
    libx11
    libxrandr
    libxi
    libxcursor
  ];
  fonts = makeFontsConf {fontDirectories = [adwaita-fonts];};
  args = {
    pname = binary;
    version = "0.1.0";
    inherit src cargoVendorDir cargoArtifacts;
    cargoLock = lockFile;
    cargoBuildExtraArgs = "-p ${package}";
    cargoTestExtraArgs = "-p ${package}";
    buildInputs = [
      libGL
      libglvnd
      libxkbcommon
      wayland
      libdrm
      pam
      vulkan-loader
      libx11
      libxrandr
      libxi
      libxcursor
    ];
    nativeBuildInputs = [pkg-config makeWrapper];
    LD_LIBRARY_PATH = runtime;
    FONTCONFIG_FILE = fonts;
    postFixup = ''
      for program in "$out"/bin/*; do
        patchelf --set-rpath "${runtime}" "$program"
        args=(--set FONTCONFIG_FILE "${fonts}")
        ${lib.optionalString (binary == "oxisession") ''
        if [[ "$program" == */oxisession-kiosk ]]; then
          args+=(--set OXI_CAGE "${cage}/bin/cage" --set OXI_HYPRLAND "${hyprland}/bin/Hyprland" --set OXI_HYPRCTL "${hyprland}/bin/hyprctl")
        fi
      ''}
        wrapProgram "$program" "''${args[@]}"
      done
    '';
    meta = {
      description = "Rust/Iced ${binary} authentication application";
      license = lib.licenses.gpl3Only;
      platforms = lib.platforms.linux;
      mainProgram = binary;
    };
  };
in
  craneLib.buildPackage (args
    // {
      passthru.clippy = craneLib.cargoClippy (args
        // {
          cargoClippyExtraArgs = "-p oxi-lock -p oxi-session -p oxi-auth-ui -p oxi-pam --all-targets -- -D warnings";
        });
      passthru.authenticationTests = craneLib.cargoTest ((builtins.removeAttrs args ["postFixup" "meta"])
        // {
          cargoTestExtraArgs = "-p oxi-lock -p oxi-session -p oxi-auth-ui -p oxi-pam";
        });
    })
