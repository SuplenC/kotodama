# The home-manager module's typed options, besides `extensions.ai` (see
# home-manager-ai.nix): evaluates home configurations that set every option,
# and none, and checks the TOML each writes, and the `kotodama-menu` service
# and shell.qml `frontEnd` makes. No VM: the files are all there is to check;
# nix/tests/hyprland.nix runs the menu.
{
  pkgs,
  self,
  home-manager,
}:

let
  inherit (pkgs) lib;
  kotodama = self.packages.${pkgs.stdenv.hostPlatform.system}.default;

  # The home configuration one `programs.kotodama` makes, with `extra` beside it.
  homeWith =
    extra: options:
    (home-manager.lib.homeManagerConfiguration {
      inherit pkgs;
      modules = [
        self.homeModules.default
        extra
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
  home = homeWith { };

  # Every option, next to the user's own Quicklinks and Aliases.
  everything =
    (home {
      settings = {
        quicklinks.gh.url = "https://github.com";
        aliases.ff = {
          extension = "apps";
          item = "firefox";
        };
      };
      history = {
        picksToLearn = 4;
        picksToTakeOver = 1;
        halfLifeDays = 7.5;
        liftThreshold = 3;
        timeOfDay = 0.25;
        usageLog = true;
        undoSeconds = 30;
        maxResults = 500;
        maxQueries = 200;
      };
      paths = {
        extensionDirs = [ "~/src/extensions" ];
        defaultExtensionDirs = false;
        stateDir = "/var/lib/alice/kotodama";
        cacheDir = "~/.cache/kotodama-test";
        socket = "/run/user/1000/kd.sock";
      };
      search = {
        rows = 8;
        nearMisses = false;
        fallbacks = false;
      };
      desktop = {
        settleMs = 120;
        paste = "ctrl+shift+v";
      };
      log.level = "debug";
      emptyQuery = {
        mostUsed = 0;
        menu = [
          {
            extension = "apps";
            item = "firefox";
          }
        ];
      };
      ranking.kinds = [
        "project"
        "*"
      ];
      quicklinkDefaults.opener = [
        "firefox"
        "--new-tab"
      ];
      extensions = {
        retryMaxSeconds = 10;
        giveUpAfter = 5;
        apps = {
          terminal = [
            "foot"
            "-e"
          ];
          hidden = [ "qt5ct" ];
          names.nvim = "Neovim";
          withdrawWithin = 0;
        };
        calculator = {
          network = false;
          ratesUrl = "http://localhost:8080/latest";
          decimalSeparator = "comma";
        };
        clipboard = {
          maxEntries = 50;
          maxAgeDays = 0.5;
          inSearch = true;
        };
        emoji = {
          skinTone = "medium-dark";
          word = "e";
        };
        folders = {
          folders = [
            {
              path = "~/Projects";
              kind = "projects";
              depth = 2;
            }
            {
              path = "/srv/scripts";
              kind = "scripts";
            }
          ];
          terminal = [ "foot" ];
          open = [ "foot" ];
          shell = [ "foot" ];
          lazygit = [ "lazygit" ];
          fileManager = [ "nautilus" ];
          copyPath = [ "wl-copy" ];
        };
        web-search.engines = [
          {
            name = "Kagi";
            url = "https://kagi.com/search?q={query}";
          }
        ];
        ai.url = "http://localhost:11434/v1/messages";
        system.inSearch = false;
        # `prefix`, the old name of `word`, is still read.
        hyprland.prefix = "hypr";
      };
    }).xdg.configFile."kotodama/home-manager.toml".source;

  # An empty engine list turns searching off: it is written, not left out.
  noEngines =
    (home { extensions.web-search.engines = [ ]; }).xdg.configFile."kotodama/home-manager.toml".source;

  # Nothing set: no config file at all, so every default applies.
  nothing = (home { }).xdg.configFile;

  # The ready Front end, on by default with the module's look: the package's
  # own shell.qml, run by the package's Quickshell, with the module on its
  # import path only.
  menu = (home { }).systemd.user.services.kotodama-menu;
  # Off when turned off.
  noMenu = (home { frontEnd.enable = false; }).systemd.user.services;
  # Off by default beside a Quickshell of the user's that home-manager runs,
  # where the user places the menu.
  ownQuickshellServices =
    (homeWith {
      programs.quickshell = {
        enable = true;
        systemd.enable = true;
      };
    } { }).systemd.user.services;
  importPath = "QML_IMPORT_PATH=${kotodama.qml}/lib/qt-6/qml";
  # With a look, and a stand-in Quickshell, so the check builds neither the
  # package nor Quickshell: ExecStart names the shell.qml written for it.
  themedMenu =
    (home {
      frontEnd = {
        enable = true;
        quickshellPackage = pkgs.writeShellScriptBin "quickshell" "";
        target = "hyprland-session.target";
        look = {
          accent = "#ff8800";
          fontFamily = "Inter";
          fontSize = 13;
          radius = 0;
          top = 80.5;
        };
      };
    }).systemd.user.services.kotodama-menu;
  # home-manager's own Quickshell service finds the module too.
  userQuickshell = ownQuickshellServices.quickshell;
in
assert !(nothing ? "kotodama/home-manager.toml");
assert !(noMenu ? kotodama-menu);
assert !(ownQuickshellServices ? kotodama-menu);
assert
  menu.Service.ExecStart
  == [ "${lib.getExe kotodama.quickshell} --path ${kotodama.qml}/share/kotodama/quickshell" ];
assert menu.Service.Environment == [ importPath ];
assert menu.Unit.PartOf == [ "graphical-session.target" ];
assert menu.Install.WantedBy == [ "graphical-session.target" ];
assert themedMenu.Service.Environment == [ importPath ];
assert themedMenu.Unit.PartOf == [ "hyprland-session.target" ];
assert themedMenu.Install.WantedBy == [ "hyprland-session.target" ];
assert userQuickshell.Service.Environment == [ importPath ];
pkgs.runCommand "kotodama-home-manager-options-test"
  {
    nativeBuildInputs = [ pkgs.python3 ];
    inherit everything noEngines;
    themedExecStart = builtins.head themedMenu.Service.ExecStart;
    shipped = ../../crates/kotodama-qml/shell/shell.qml;
  }
  ''
    python3 - <<'PY'
    import os, tomllib

    def load(name):
        with open(os.environ[name], "rb") as f:
            return tomllib.load(f)

    def expect(name, got, want):
        assert got == want, f"{name}: got {got!r}, want {want!r}"

    everything = load("everything")
    expect("quicklinks", everything["quicklinks"], {"gh": {"url": "https://github.com"}})
    expect("aliases", everything["aliases"], {"ff": {"extension": "apps", "item": "firefox"}})
    expect("history", everything["history"], {
        "picks_to_learn": 4,
        "picks_to_take_over": 1,
        "half_life_days": 7.5,
        "lift_threshold": 3,
        "time_of_day": 0.25,
        "usage_log": True,
        "undo_seconds": 30,
        "max_results": 500,
        "max_queries": 200,
    })
    expect("paths", everything["paths"], {
        "extension_dirs": ["~/src/extensions"],
        "default_extension_dirs": False,
        "state_dir": "/var/lib/alice/kotodama",
        "cache_dir": "~/.cache/kotodama-test",
        "socket": "/run/user/1000/kd.sock",
    })
    expect("search", everything["search"], {"rows": 8, "near_misses": False, "fallbacks": False})
    expect("desktop", everything["desktop"], {"settle_ms": 120, "paste": "ctrl+shift+v"})
    expect("log", everything["log"], {"level": "debug"})
    expect("empty_query", everything["empty_query"], {
        "most_used": 0,
        "menu": [{"extension": "apps", "item": "firefox"}],
    })
    expect("ranking", everything["ranking"], {"kinds": ["project", "*"]})
    expect("quicklink_defaults", everything["quicklink_defaults"], {"opener": ["firefox", "--new-tab"]})

    extensions = everything["extensions"]
    expect("extensions", sorted(extensions), [
        "ai", "apps", "calculator", "clipboard", "emoji", "folders", "give_up_after", "hyprland",
        "retry_max_seconds", "system", "web-search",
    ])
    expect("restart", (extensions["retry_max_seconds"], extensions["give_up_after"]), (10, 5))
    expect("ai", extensions["ai"], {"url": "http://localhost:11434/v1/messages"})
    expect("apps", extensions["apps"], {
        "terminal": ["foot", "-e"],
        "hidden": ["qt5ct"],
        "names": {"nvim": "Neovim"},
        "withdraw_within": 0,
    })
    expect("calculator", extensions["calculator"], {
        "network": False,
        "rates_url": "http://localhost:8080/latest",
        "decimal_separator": "comma",
    })
    expect("clipboard", extensions["clipboard"], {"max_entries": 50, "max_age_days": 0.5, "in_search": True})
    expect("emoji", extensions["emoji"], {"skin_tone": "medium-dark", "word": "e"})
    expect("folders", extensions["folders"], {
        "folders": [
            {"path": "~/Projects", "kind": "projects", "depth": 2},
            {"path": "/srv/scripts", "kind": "scripts"},
        ],
        "terminal": ["foot"],
        "open": ["foot"],
        "shell": ["foot"],
        "lazygit": ["lazygit"],
        "file_manager": ["nautilus"],
        "copy_path": ["wl-copy"],
    })
    expect("system", extensions["system"], {"in_search": False})
    expect("hyprland", extensions["hyprland"], {"word": "hypr"})
    expect("web-search", extensions["web-search"], {
        "engines": [{"name": "Kagi", "url": "https://kagi.com/search?q={query}"}],
    })

    expect("noEngines", load("noEngines"), {"extensions": {"web-search": {"engines": []}}})

    # The themed menu: the shipped shell.qml, with only the set look properties.
    program, flag, config = os.environ["themedExecStart"].split(" ")
    expect("themed program", (os.path.basename(program), flag), ("quickshell", "--path"))
    with open(os.path.join(config, "shell.qml")) as f:
        themed = f.read()
    with open(os.environ["shipped"]) as f:
        shipped = f.read()
    expect("themed shell.qml", themed, shipped.replace("KotodamaMenu {}", """KotodamaMenu {
            accent: "#ff8800"
            fontFamily: "Inter"
            fontSize: 13
            radius: 0
            top: 80.5
        }"""))
    PY
    touch $out
  ''
