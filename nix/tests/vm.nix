# A NixOS VM with Kotodama installed through the home-manager module, for a
# user whose systemd user manager runs from boot. It checks that the socket
# starts the Daemon, that home-manager's settings reach it, that it reaches
# the session bus and finds apps wherever NixOS installs them, that the
# Folder source lists projects and scripts, that the programs it starts
# outlive it, that a Bundled extension left off is neither installed nor
# started, and that apps and Extensions a switch installs show up without a
# restart.
{
  pkgs,
  self,
  home-manager,
}:

let
  # An Extension that reports a finished job as soon as it starts, so the
  # Daemon sends a desktop notification titled `title` over the session bus.
  jobManifest = name: title: ''
    name = "${name}"
    command = ["${pkgs.writeShellScript name ''
      export LC_ALL=C
      send() { printf '%d\n%s' "''${#1}" "$1"; }
      read_frame() { IFS= read -r len && IFS= read -r -N "$len" frame; }
      send '{"version":1,"minor":0,"formats":["json"]}'
      read_frame # the Daemon's hello
      read_frame # the config
      send '{"job_progress":{"id":"${name}","title":"${title}","state":"succeeded"}}'
      while read_frame; do :; done
    ''}"]
  '';

  # A package holding a job Extension, as one from nixpkgs would.
  jobExtension =
    name: title:
    pkgs.writeTextDir "share/kotodama/extensions/${name}/manifest.toml" (jobManifest name title);

  app =
    name: desktopName: exec:
    pkgs.makeDesktopItem { inherit name desktopName exec; };
in
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
      # An app in the system profile, which the user manager's XDG_DATA_DIRS
      # holds on NixOS.
      (app "dozer" "Dozer" "sleep 1003")
    ];
    # Extensions in the system profile, which is on XDG_DATA_DIRS too.
    environment.pathsToLink = [ "/share/kotodama" ];

    # What a switch to it installs: an app and an Extension in the system
    # profile, and the same in the home-manager profile, ~/.nix-profile.
    specialisation.switched.configuration = {
      environment.systemPackages = [
        (app "loader" "Loader" "sleep 1004")
        (jobExtension "vm-system-job" "System job")
      ];
      home-manager.users.alice.home.packages = [
        (app "hauler" "Hauler" "sleep 1005")
        (jobExtension "vm-home-job" "Home job")
      ];
    };

    home-manager.useGlobalPkgs = true;
    home-manager.users.alice = {
      imports = [ self.homeModules.default ];
      home.stateVersion = "25.11";
      programs.kotodama = {
        enable = true;
        # Off by default, unlike apps, calculator, clipboard, emoji and
        # web-search. hyprland, system and ai stay off.
        extensions.folders.enable = true;
        settings.quicklinks.nap = {
          title = "Nap";
          command = [
            "sleep"
            "1001"
          ];
        };
        settings.extensions.folders.folders = [
          {
            path = "~/Projects";
            kind = "projects";
          }
          {
            path = "~/Scripts";
            kind = "scripts";
          }
        ];
      };
      # A project for the Folder source: a folder with a `.git` is a
      # repository.
      home.file."Projects/demo/.git/HEAD".text = "ref: refs/heads/main\n";
      # A script it runs in the background, which notifies when it is done.
      home.file."Scripts/vm-sync.sh" = {
        executable = true;
        text = ''
          #!/bin/sh
          # kotodama: terminal=false name=VM sync
          echo synced
        '';
      };
      # An app in the user's own data directory, which the apps Extension
      # reads whatever XDG_DATA_DIRS holds.
      xdg.dataFile."applications/sleeper.desktop".text = ''
        [Desktop Entry]
        Type=Application
        Name=Sleeper
        Exec=sleep 1002
      '';
      xdg.configFile."kotodama/extensions/vm-job/manifest.toml".text = jobManifest "vm-job" "VM job";
    };
  };

  testScript = ''
    front_end = "python3 ${./front-end.py}"

    def as_alice(command):
        return f"su -l alice -c 'XDG_RUNTIME_DIR=/run/user/1000 {command}'"

    def user(command):
        return machine.succeed(as_alice(command))

    def is_active(unit):
        return machine.execute(as_alice(f"systemctl --user is-active {unit}"))[0] == 0

    def pgrep(program):
        # The program may be named by its full path.
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
            as_alice(f"{front_end} nap") + " | grep -P '^kotodama\tquicklink:nap\t'"
        )

    with subtest("the Daemon reaches the user's session bus for notifications"):
        machine.wait_until_succeeds("grep -q 'string \"VM job\"' /tmp/notifications")

    with subtest("the Bundled extensions are found: the user's apps and the system's, the calculator and emoji"):
        machine.wait_until_succeeds(
            as_alice(f"{front_end} sleeper") + " | grep -P '^apps\tsleeper\tSleeper$'"
        )
        machine.wait_until_succeeds(
            as_alice(f"{front_end} dozer") + " | grep -P '^apps\tdozer\tDozer$'"
        )
        machine.wait_until_succeeds(
            as_alice(f"{front_end} clipboard") + " | grep -P '^kotodama\tprefix:clipboard\tClipboard$'"
        )
        # What was copied is in the clipboard's app alone, the History first.
        machine.wait_until_succeeds(
            as_alice(f"{front_end} --app clipboard") + " | grep -P '^clipboard\thistory\tClipboard History$'"
        )
        machine.wait_until_succeeds(
            as_alice(f"{front_end} --wait 6\\*7") + " | grep -P '^calculator\t6\\*7\t42$'"
        )
        machine.wait_until_succeeds(
            as_alice(f"{front_end} --app emoji thumbs") + " | grep -P '^emoji\t👍\t👍 Thumbs up$'"
        )

    with subtest("the Folder source lists the configured projects and scripts"):
        machine.wait_until_succeeds(
            as_alice(f"{front_end} demo") + " | grep -P '^folders\t/home/alice/Projects/demo\tdemo$'"
        )
        machine.wait_until_succeeds(
            as_alice(f"{front_end} vm sync") + " | grep -P '^folders\t/home/alice/Scripts/vm-sync.sh\tVM sync$'"
        )

    with subtest("a Bundled extension left off is neither installed nor started"):
        # Every Extension that is on has started by now: each answered above.
        extensions = user("ls ~/.nix-profile/share/kotodama/extensions").split()
        assert extensions == ["apps", "calculator", "clipboard", "emoji", "folders", "web-search"], extensions
        for name in ["hyprland", "system", "ai"]:
            machine.fail(pgrep(f"kotodama-{name}"))

    with subtest("a background script notifies the desktop when it is done"):
        user(f"{front_end} --run vm sync")
        machine.wait_until_succeeds("grep -q 'string \"VM sync\"' /tmp/notifications")

    with subtest("programs started from the menu get scopes of their own, named for the app where it has a desktop id"):
        # The Daemon starts the Quicklink, which has no desktop id, so its
        # scope is named for its program; the apps Extension starts
        # sleeper.desktop.
        user(f"{front_end} --run nap")
        user(f"{front_end} --run sleeper")
        scopes = {"sleep 1001": "app-kotodama-sleep-", "sleep 1002": "app-kotodama-sleeper-"}
        for program, scope in scopes.items():
            machine.wait_until_succeeds(pgrep(program))
            cgroup = cgroup_of(program)
            assert "kotodamad.service" not in cgroup, f"{program} runs in the Daemon's cgroup: {cgroup}"
            assert f"/app.slice/{scope}" in cgroup, f"{program} isn't in a scope named {scope}*: {cgroup}"

    with subtest("stopping the Daemon leaves them running, and the next toggle brings it back"):
        user("systemctl --user stop kotodamad.service")
        assert not is_active("kotodamad.service")
        assert is_active("kotodamad.socket")
        for program in ["sleep 1001", "sleep 1002"]:
            machine.succeed(pgrep(program))
        user("kotodama toggle")
        machine.wait_for_unit("kotodamad.service", "alice")
        user(f"{front_end} nap")

    with subtest("apps and Extensions a system switch and a home-manager switch install show up without a restart"):
        machine.wait_until_succeeds(
            as_alice(f"{front_end} dozer") + " | grep -P '^apps\tdozer\tDozer$'"
        )
        daemon = user("systemctl --user show -P MainPID kotodamad.service").strip()
        apps = machine.succeed(pgrep("kotodama-apps")).strip()
        # Replaces the /run/current-system link, and restarts
        # home-manager-alice.service, which replaces the profile link behind
        # ~/.nix-profile.
        machine.succeed("/run/booted-system/specialisation/switched/bin/switch-to-configuration test")
        machine.wait_until_succeeds(
            as_alice(f"{front_end} loader") + " | grep -P '^apps\tloader\tLoader$'"
        )
        machine.wait_until_succeeds(
            as_alice(f"{front_end} hauler") + " | grep -P '^apps\thauler\tHauler$'"
        )
        machine.wait_until_succeeds("grep -q 'string \"System job\"' /tmp/notifications")
        machine.wait_until_succeeds("grep -q 'string \"Home job\"' /tmp/notifications")
        assert user("systemctl --user show -P MainPID kotodamad.service").strip() == daemon, "the Daemon restarted"
        assert machine.succeed(pgrep("kotodama-apps")).strip() == apps, "the apps Extension restarted"
  '';
}
