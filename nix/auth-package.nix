{ craneLib, src, cargoVendorDir, cargoArtifacts, lockFile, package, binary,
  lib, pkg-config, makeWrapper, libGL, libglvnd, libxkbcommon, wayland, libdrm,
  pam, vulkan-loader, xorg, mesa, makeFontsConf, adwaita-fonts, cage, hyprland }:
let
  runtime = lib.makeLibraryPath [ libGL libglvnd libxkbcommon wayland libdrm pam vulkan-loader
    xorg.libX11 xorg.libXrandr xorg.libXi xorg.libXcursor ];
  fonts = makeFontsConf { fontDirectories = [ adwaita-fonts ]; };
  args = {
  pname = binary;
  version = "0.1.0";
  inherit src cargoVendorDir cargoArtifacts;
  cargoLock = lockFile;
  cargoBuildExtraArgs = "-p ${package}";
  cargoTestExtraArgs = "-p ${package}";
  buildInputs = [ libGL libglvnd libxkbcommon wayland libdrm pam vulkan-loader
    xorg.libX11 xorg.libXrandr xorg.libXi xorg.libXcursor ];
  nativeBuildInputs = [ pkg-config makeWrapper ];
  LD_LIBRARY_PATH = runtime;
  FONTCONFIG_FILE = fonts;
  postFixup = ''
    for program in "$out"/bin/*; do
      args=(--prefix LD_LIBRARY_PATH : "${runtime}" --set FONTCONFIG_FILE "${fonts}"
        --set-default VK_ICD_FILENAMES "${mesa}/share/vulkan/icd.d/lvp_icd.${if lib.hasInfix "aarch64" mesa.system then "aarch64" else "x86_64"}.json")
      ${lib.optionalString (binary == "oxisession") ''
        if [[ "$program" == */oxisession-kiosk ]]; then
          args+=(--set OXI_CAGE "${cage}/bin/cage" --set OXI_HYPRLAND "${hyprland}/bin/Hyprland" --set OXI_HYPRCTL "${hyprland}/bin/hyprctl")
        fi
      ''}
      wrapProgram "$program" "''${args[@]}"
    done
  '';
  meta = { description = "Rust/Iced ${binary} authentication application";
    license = lib.licenses.gpl3Only; platforms = lib.platforms.linux; mainProgram = binary; };
  };
in craneLib.buildPackage (args // {
  passthru.clippy = craneLib.cargoClippy (args // {
    cargoClippyExtraArgs = "-p oxi-lock -p oxi-session -p oxi-auth-ui -p oxi-pam --all-targets -- -D warnings";
  });
})
