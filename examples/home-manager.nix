{ ... }: {
  programs.oxibar = {
    enable = true;
    systemd.enable = true;
    systemd.target = "hyprland-session.target";
    config.bar.outputs = [ "*" ];
    config.dock.pinned = [ "firefox.desktop" ];
  };

  wayland.windowManager.hyprland = {
    enable = true;
    systemd.enable = true;
    settings.bind = [
      "SUPER, SPACE, exec, oxibar launcher toggle"
      "SUPER, COMMA, exec, oxibar settings open"
      "SUPER, N, exec, oxibar control-center toggle"
      ", XF86AudioRaiseVolume, exec, oxibar action Audio volume +5"
      ", XF86AudioLowerVolume, exec, oxibar action Audio volume -5"
      ", XF86AudioMute, exec, oxibar action Audio mute toggle"
      ", XF86MonBrightnessUp, exec, oxibar action System brightness +5"
      ", XF86MonBrightnessDown, exec, oxibar action System brightness -5"
    ];
  };
}
