# The home-manager module's `extensions.<name>.enable`: evaluates home
# configurations with the defaults and with some switched, and checks which
# Bundled extensions the installed package holds, that the Daemon its units
# start finds those and no other, and the warning for settings of an Extension
# left off. nix/tests/vm.nix checks an Extension left off isn't started.
{
  pkgs,
  self,
  home-manager,
}:

let
  inherit (pkgs) lib;
  kotodama = self.packages.${pkgs.stdenv.hostPlatform.system}.default;

  # The home configuration one `programs.kotodama` makes.
  home =
    options:
    (home-manager.lib.homeManagerConfiguration {
      inherit pkgs;
      modules = [
        self.homeModules.default
        {
          home.username = "alice";
          home.homeDirectory = "/home/alice";
          home.stateVersion = "25.11";
          programs.kotodama = {
            enable = true;
          }
          // options;
        }
      ];
    }).config;

  # The Bundled extensions a configuration installs, by their packages' names.
  installed = config: map lib.getName config.programs.kotodama.finalPackage.installedExtensions;

  defaults = home { };
  switched = home {
    extensions = {
      apps.enable = false;
      folders.enable = true;
      hyprland.enable = true;
    };
  };
  # Folders set up, but left off.
  unused = home {
    extensions.folders.folders = [
      {
        path = "~/Projects";
        kind = "projects";
      }
    ];
  };
  # A package with no `withExtensions`, as one built before the split, is
  # installed as it is.
  ownPackage = home { package = kotodama.core; };
in
assert
  installed defaults == [
    "kotodama-apps"
    "kotodama-calculator"
    "kotodama-clipboard"
    "kotodama-emoji"
    "kotodama-web-search"
  ];
assert
  installed switched == [
    "kotodama-calculator"
    "kotodama-clipboard"
    "kotodama-emoji"
    "kotodama-folders"
    "kotodama-hyprland"
    "kotodama-web-search"
  ];
# Only the package with the enabled Extensions is installed, never the full one.
assert lib.elem defaults.programs.kotodama.finalPackage defaults.home.packages;
assert !(lib.elem kotodama defaults.home.packages);
assert defaults.warnings == [ ];
assert
  unused.warnings == [
    "programs.kotodama: `[extensions.folders]` is set, but the folders Extension isn't installed; set programs.kotodama.extensions.folders.enable = true to install it."
  ];
assert ownPackage.programs.kotodama.finalPackage == kotodama.core;
# The QML module comes from `package` whatever the Extensions.
assert switched.programs.kotodama.qmlImportPath == "${kotodama.qml}/lib/qt-6/qml";
pkgs.runCommand "kotodama-home-manager-extensions-test"
  {
    daemon = defaults.programs.kotodama.finalPackage;
    service = defaults.xdg.configFile."systemd/user/kotodamad.service".source;
  }
  ''
    # extensions PACKAGE NAME...: PACKAGE's extensions output holds these, each
    # with its manifest, and nothing else.
    extensions() {
      local dir=$1/share/kotodama/extensions
      shift
      local got
      got=$(ls "$dir" | tr '\n' ' ')
      if [ "$got" != "$* " ]; then
        echo "$dir holds $got, not $*"
        exit 1
      fi
      for name in "$@"; do
        test -f "$dir/$name/manifest.toml"
        test -x "$dir/$name/kotodama-$name"
      done
    }
    extensions ${defaults.programs.kotodama.finalPackage.extensions} apps calculator clipboard emoji web-search
    extensions ${switched.programs.kotodama.finalPackage.extensions} calculator clipboard emoji folders hyprland web-search

    # The units start the wrapped Daemon, whose XDG_DATA_DIRS leads with the
    # Extensions installed beside it.
    grep -qx "ExecStart=$daemon/bin/kotodamad" "$service"
    grep -qaF "${defaults.programs.kotodama.finalPackage.extensions}/share" "$daemon/bin/kotodamad"
    touch $out
  ''
