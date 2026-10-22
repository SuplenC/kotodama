# The home-manager module's `extensions.<name>.enable` and `extraExtensions`:
# evaluates home configurations with the defaults, with some switched and with
# an Extension that isn't Bundled, and checks which Extensions the installed
# package holds, that the Daemon its units start finds those and no other, that
# a script Extension installs but is refused with `strictExtensions`, that one
# needing the build-time Qt is refused either way, and the warning for
# settings of an Extension left off.
# nix/tests/vm.nix checks an Extension left off isn't started.
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

  # An Extension that isn't Bundled, as a distro or a user packages one: its
  # directory under share/kotodama/extensions.
  outOfTree =
    name: files:
    pkgs.runCommand "kotodama-${name}" { } ''
      dir=$out/share/kotodama/extensions/${name}
      mkdir -p $dir
      printf '%s\n' 'name = "${name}"' 'command = ["./kotodama-${name}"]' >$dir/manifest.toml
      ${files}
    '';
  hello = outOfTree "hello" "";
  # A script, run by the shell no Extension of Kotodama's may need.
  script = outOfTree "script" ''
    printf '%s\n' '#!${pkgs.runtimeShell}' 'exit 0' >$dir/kotodama-script
    chmod +x $dir/kotodama-script
  '';
  # One whose files name the build-time Qt, a build environment.
  needsQt = outOfTree "needs-qt" "echo ${kotodama.buildQt} >$dir/qt";
  extra = home { extraExtensions = [ hello ]; };
  withScript = home { extraExtensions = [ script ]; };
  strictScript = home {
    extraExtensions = [ script ];
    strictExtensions = true;
  };
  qtRelaxed = home { extraExtensions = [ needsQt ]; };
  qtStrict = home {
    extraExtensions = [ needsQt ];
    strictExtensions = true;
  };
  # The build of a configuration's package, which the builder's own check
  # fails. The outputs' checks are left off: the log testBuildFailure keeps in
  # `out` names the path refused, and the builder's shell, which they refuse.
  refused =
    config:
    pkgs.testers.testBuildFailure (
      config.programs.kotodama.finalPackage.overrideAttrs { outputChecks = { }; }
    );
  # With a package that has no `withExtensions` to install it with, which
  # `ownPackage` shows evaluates alone.
  extraWithoutWithExtensions = home {
    package = kotodama.core;
    extraExtensions = [ hello ];
  };
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
# One that isn't Bundled comes after the enabled Bundled ones, in the package
# installed.
assert
  installed extra == [
    "kotodama-apps"
    "kotodama-calculator"
    "kotodama-clipboard"
    "kotodama-emoji"
    "kotodama-web-search"
    "kotodama-hello"
  ];
assert lib.elem extra.programs.kotodama.finalPackage extra.home.packages;
assert extra.warnings == [ ];
# home-manager throws on a failed assertion.
assert !(builtins.tryEval extraWithoutWithExtensions).success;
# The QML module comes from `package` whatever the Extensions.
assert switched.programs.kotodama.qmlImportPath == "${kotodama.qml}/lib/qt-6/qml";
pkgs.runCommand "kotodama-home-manager-extensions-test"
  {
    daemon = defaults.programs.kotodama.finalPackage;
    service = defaults.xdg.configFile."systemd/user/kotodamad.service".source;
    withHello = extra.programs.kotodama.finalPackage;
    scriptExtensions = withScript.programs.kotodama.finalPackage.extensions;
    strictScript = refused strictScript;
    qtRelaxed = refused qtRelaxed;
    qtStrict = refused qtStrict;
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

    # The Extension that isn't Bundled sits beside the Bundled ones, its
    # manifest where the Daemon looks for one: kotodama/extensions/<name> in
    # the directory its wrapper puts first on XDG_DATA_DIRS.
    share=${extra.programs.kotodama.finalPackage.extensions}/share
    got=$(ls "$share/kotodama/extensions" | tr '\n' ' ')
    if [ "$got" != "apps calculator clipboard emoji hello web-search " ]; then
      echo "$share/kotodama/extensions holds $got"
      exit 1
    fi
    test -f "$share/kotodama/extensions/hello/manifest.toml"
    grep -qaF "$share" "$withHello/bin/kotodamad"

    # A script Extension installs, its manifest where the Daemon looks, but
    # is refused, by name, with strictExtensions.
    test -f "$scriptExtensions/share/kotodama/extensions/script/manifest.toml"
    test -x "$scriptExtensions/share/kotodama/extensions/script/kotodama-script"
    grep -qF "refusing the Extension kotodama-script: it needs ${pkgs.runtimeShellPackage} at run time" "$strictScript/testBuildFailure.log"

    # One that needs the build-time Qt is refused either way.
    for log in "$qtRelaxed" "$qtStrict"; do
      grep -qF "refusing the Extension kotodama-needs-qt: it needs ${kotodama.buildQt} at run time" "$log/testBuildFailure.log"
    done
    touch $out
  ''
