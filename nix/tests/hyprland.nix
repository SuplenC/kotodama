# A NixOS VM running Hyprland, with Kotodama installed through the
# home-manager module, for the Hyprland Extension's live behavior. Hyprland
# needs a DRM device, which a container doesn't have, so it runs here the way
# Hyprland tests itself: a virtio GPU, rendered in software, and Hyprland
# started for a user logged in on a tty. It checks that an opened window is an
# Item whose focus Action works, and that a bind added at runtime is in the
# Shortcuts View that `kotodama open --view hyprland/shortcuts` opens.
{
  pkgs,
  self,
  home-manager,
}:

pkgs.testers.runNixOSTest {
  name = "kotodama-hyprland";

  nodes.machine = {
    imports = [ home-manager.nixosModules.home-manager ];

    users.users.alice = {
      isNormalUser = true;
      uid = 1000;
      # The user manager starts at boot, so kotodamad.socket is there.
      linger = true;
    };
    # Hyprland takes the seat of alice's session on tty1.
    services.getty.autologinUser = "alice";

    programs.hyprland.enable = true;
    # Nothing here needs portals, and they slow the boot down.
    xdg.portal.enable = pkgs.lib.mkForce false;
    services.speechd.enable = false;
    # Windows to search for: foot needs a font.
    fonts.enableDefaultPackages = true;
    environment.systemPackages = [
      pkgs.python3
      pkgs.foot
    ];

    virtualisation = {
      cores = 2;
      memorySize = 4096;
      qemu.options = [
        "-vga none"
        "-device virtio-gpu-pci"
      ];
    };

    home-manager.useGlobalPkgs = true;
    home-manager.users.alice = {
      imports = [ self.homeModules.default ];
      home.stateVersion = "25.11";
      programs.kotodama.enable = true;
      # The Daemon, a systemd user service, finds Hyprland through the user
      # manager's environment, as a desktop's own Hyprland config would set it.
      xdg.configFile."hypr/hyprland.conf".text = ''
        exec-once = systemctl --user import-environment HYPRLAND_INSTANCE_SIGNATURE WAYLAND_DISPLAY
        misc {
          disable_hyprland_logo = true
          disable_splash_rendering = true
        }
        bindd = SUPER, F1, Show the keyboard shortcuts, exec, kotodama open --view hyprland/shortcuts
      '';
    };
  };

  testScript = ''
    front_end = "python3 ${./front-end.py}"

    def as_alice(command):
        return f"su -l alice -c 'XDG_RUNTIME_DIR=/run/user/1000 {command}'"

    def user(command):
        return machine.succeed(as_alice(command))

    def hyprctl(command):
        return user(f"hyprctl -i 0 {command}")

    def active_window():
        return as_alice("hyprctl -i 0 activewindow -j")

    machine.wait_for_unit("multi-user.target")
    machine.wait_for_unit("home-manager-alice.service")
    machine.wait_for_unit("kotodamad.socket", "alice")
    machine.wait_until_succeeds("loginctl --no-legend list-sessions | grep -q 'alice.*tty1'")

    user("nohup Hyprland >/tmp/hyprland.log 2>&1 &")
    try:
        machine.wait_until_succeeds(as_alice("hyprctl -i 0 version"), timeout=120)
        machine.wait_until_succeeds(
            as_alice("systemctl --user show-environment") + " | grep -q HYPRLAND_INSTANCE_SIGNATURE"
        )
    finally:
        print(machine.execute("cat /tmp/hyprland.log")[1])

    with subtest("the Daemon starts inside Hyprland, and finds its binds"):
        user("kotodama toggle")
        machine.wait_for_unit("kotodamad.service", "alice")
        machine.wait_until_succeeds(
            as_alice(f"{front_end} show the keyboard")
            + " | grep -P '^hyprland\t/64/F1\tShow the keyboard shortcuts$'"
        )

    with subtest("an opened window appears as an Item, and its focus Action works"):
        # `--` ends hyprctl's own flags, so foot's are passed on.
        hyprctl("-- dispatch exec foot --title vm-one sleep 1001")
        machine.wait_until_succeeds(
            as_alice(f"{front_end} vm-one") + " | grep -P '^hyprland\t0x[0-9a-f]+\tvm-one$'"
        )
        hyprctl("-- dispatch exec foot --title vm-two sleep 1002")
        machine.wait_until_succeeds(
            as_alice(f"{front_end} vm-two") + " | grep -P '^hyprland\t0x[0-9a-f]+\tvm-two$'"
        )
        machine.wait_until_succeeds(active_window() + " | grep -q '\"title\": \"vm-two\"'")
        user(f"{front_end} --run vm-one")
        machine.wait_until_succeeds(active_window() + " | grep -q '\"title\": \"vm-one\"'")

    with subtest("a closed window is gone"):
        hyprctl("dispatch closewindow title:vm-two")
        machine.wait_until_succeeds(
            as_alice(f"{front_end} vm-two") + " | (! grep -q vm-two)"
        )

    with subtest("a bind added at runtime is in the Shortcuts View when it next opens"):
        user(f"nohup {front_end} --listen >/tmp/menu 2>&1 &")
        machine.wait_until_succeeds("grep -q '^ready$' /tmp/menu")
        user("kotodama open --view hyprland/shortcuts")
        machine.wait_until_succeeds("grep -qP '^open\thyprland/shortcuts$' /tmp/menu")
        machine.succeed("grep -qP '^view\thyprland\tshortcuts$' /tmp/menu")
        machine.succeed("grep -qP '^SUPER \\+ F1\tShow the keyboard shortcuts$' /tmp/menu")
        machine.fail("grep -qP '^SUPER \\+ F9\t' /tmp/menu")

        hyprctl("keyword bind SUPER, F9, exec, true")
        user("kotodama open --view hyprland/shortcuts")
        machine.wait_until_succeeds("grep -qP '^SUPER \\+ F9\texec true$' /tmp/menu")
  '';
}
