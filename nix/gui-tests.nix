let
  source = builtins.path {
    path = ../.;
    name = "oxibar-gui-verification";
    filter = path: type: !(builtins.elem (builtins.baseNameOf path) [ ".git" ".direnv" "target" "tmp" ]);
  };
  flake = builtins.getFlake (builtins.unsafeDiscardStringContext (toString source));
  pkgs = import flake.inputs.nixpkgs { system = builtins.currentSystem; };
  parent = pkgs.sway.override {
    sway-unwrapped = pkgs.sway-unwrapped.overrideAttrs (old: {
      postPatch = (old.postPatch or "") + ''
        substituteInPlace sway/server.c --replace-fail '#define SWAY_XDG_SHELL_VERSION 5' '#define SWAY_XDG_SHELL_VERSION 6'
      '';
    });
  };
in pkgs.mkShell {
  packages = [
    (pkgs.python3.withPackages (ps: [ ps.dbus-next ]))
    pkgs.dbus pkgs.wlrctl pkgs.wtype pkgs.wl-clipboard pkgs.pulseaudio
    pkgs.grim parent pkgs.mesa pkgs.hyprland pkgs.kitty
    pkgs.wayland-scanner pkgs.pkg-config pkgs.gcc pkgs.wlr-protocols
  ];
  buildInputs = [ pkgs.wayland ];
  OXIBAR_TEST_HYPRLAND = "${pkgs.hyprland}/bin/Hyprland";
  OXIBAR_TEST_VIRTUAL_POINTER_XML = "${pkgs.wlr-protocols}/share/wlr-protocols/unstable/wlr-virtual-pointer-unstable-v1.xml";
  __EGL_VENDOR_LIBRARY_FILENAMES = "${pkgs.mesa}/share/glvnd/egl_vendor.d/50_mesa.json";
  GBM_BACKENDS_PATH = "${pkgs.mesa}/lib/gbm";
  LIBGL_DRIVERS_PATH = "${pkgs.mesa}/lib/dri";
}
