# A NixOS VM with Kotodama installed through the home-manager module, for a
# user whose systemd user manager runs from boot. It checks that the socket
# starts the Daemon, that home-manager's settings reach it, and that the
# programs it starts outlive it, and that it reaches the session bus.
{
  pkgs,
  self,
  home-manager,
}:

pkgs.testers.runNixOSTest {
  name = "kotodama-vm";

  nodes.machine = {
    imports = [ home-manager.nixosModules.home-manager ];

    users.users.alice = {
      isNormalUser = true;
      uid = 1000;
      # The user manager starts at boot, with no login.
      linger = true;
    };
    environment.systemPackages = [
      pkgs.python3
      pkgs.dbus # dbus-monitor
    ];

    home-manager.useGlobalPkgs = true;
    home-manager.users.alice = {
      imports = [ self.homeModules.default ];
      home.stateVersion = "25.11";
      programs.kotodama = {
        enable = true;
        settings.quicklinks.nap = {
          title = "Nap";
          command = [
            "sleep"
            "1001"
          ];
        };
      };
      # An app in the user's own data directory, which the apps Extension
      # reads whatever XDG_DATA_DIRS holds.
      xdg.dataFile."applications/sleeper.desktop".text = ''
        [Desktop Entry]
        Type=Application
        Name=Sleeper
        Exec=sleep 1002
      '';
      # An Extension that reports a finished job as soon as it starts, so the
      # Daemon sends a desktop notification over the session bus.
      xdg.configFile."kotodama/extensions/vm-job/manifest.toml".text = ''
        name = "vm-job"
        command = ["${pkgs.writeShellScript "vm-job" ''
          export LC_ALL=C
          send() { printf '%d\n%s' "''${#1}" "$1"; }
          read_frame() { IFS= read -r len && IFS= read -r -N "$len" frame; }
          send '{"version":1,"formats":["json"]}'
          read_frame # the Daemon's hello
          read_frame # the config
          send '{"job_progress":{"id":"vm","title":"VM job","state":"succeeded"}}'
          while read_frame; do :; done
        ''}"]
      '';
    };
  };

  testScript = ''
    front_end = "python3 ${./front-end.py}"

    def user(command):
        return machine.succeed(f"su -l alice -c 'XDG_RUNTIME_DIR=/run/user/1000 {command}'")

    def is_active(unit):
        return machine.execute(
            f"su -l alice -c 'XDG_RUNTIME_DIR=/run/user/1000 systemctl --user is-active {unit}'"
        )[0] == 0

    def pgrep(program):
        # systemd-run starts the program by its full path.
        return f"pgrep -u alice -f '(^|/){program}$'"

    def cgroup_of(program):
        pid = machine.succeed(pgrep(program)).strip()
        return machine.succeed(f"cat /proc/{pid}/cgroup")

    machine.wait_for_unit("multi-user.target")
    machine.wait_for_unit("home-manager-alice.service")
    machine.wait_for_unit("kotodamad.socket", "alice")
    # Records the calls to the notification service on alice's session bus.
    user("nohup dbus-monitor --session interface=org.freedesktop.Notifications >/tmp/notifications 2>&1 &")
    machine.wait_until_succeeds("test -s /tmp/notifications")

    with subtest("the first kotodama toggle starts the Daemon through the socket"):
        assert not is_active("kotodamad.service"), "the Daemon started before anything connected"
        user("kotodama toggle")
        machine.wait_for_unit("kotodamad.service", "alice")

    with subtest("a setting written through the home-manager module reaches the Daemon"):
        machine.wait_until_succeeds(
            f"su -l alice -c 'XDG_RUNTIME_DIR=/run/user/1000 {front_end} nap' | grep -P '^kotodama\tquicklink:nap\t'"
        )

    with subtest("the Daemon reaches the user's session bus for notifications"):
        machine.wait_until_succeeds("grep -q 'string \"VM job\"' /tmp/notifications")

    with subtest("the Bundled extensions are found, and their apps run"):
        machine.wait_until_succeeds(
            f"su -l alice -c 'XDG_RUNTIME_DIR=/run/user/1000 {front_end} sleeper' | grep -P '^apps\tsleeper\tSleeper$'"
        )

    with subtest("programs started from the menu get scopes of their own"):
        user(f"{front_end} --run nap")
        user(f"{front_end} --run sleeper")
        for program in ["sleep 1001", "sleep 1002"]:
            machine.wait_until_succeeds(pgrep(program))
            cgroup = cgroup_of(program)
            assert "kotodamad.service" not in cgroup, f"{program} runs in the Daemon's cgroup: {cgroup}"

    with subtest("stopping the Daemon leaves them running, and the next toggle brings it back"):
        user("systemctl --user stop kotodamad.service")
        assert not is_active("kotodamad.service")
        assert is_active("kotodamad.socket")
        for program in ["sleep 1001", "sleep 1002"]:
            machine.succeed(pgrep(program))
        user("kotodama toggle")
        machine.wait_for_unit("kotodamad.service", "alice")
        user(f"{front_end} nap")
  '';
}
