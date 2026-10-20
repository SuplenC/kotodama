# The home-manager module's typed options, besides `extensions.ai` (see
# home-manager-ai.nix): evaluates home configurations that set every option,
# and none, and checks the TOML each writes. No VM: the file is all there is
# to check.
{
  pkgs,
  self,
  home-manager,
}:

let
  # The home configuration one `programs.kotodama` makes.
  home =
    kotodama:
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
          // kotodama;
        }
      ];
    }).config;

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
      };
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
        };
        emoji.skinTone = "medium-dark";
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
      };
    }).xdg.configFile."kotodama/home-manager.toml".source;

  # An empty engine list turns searching off: it is written, not left out.
  noEngines =
    (home { extensions.web-search.engines = [ ]; }).xdg.configFile."kotodama/home-manager.toml".source;

  # Nothing set: no config file at all, so every default applies.
  nothing = (home { }).xdg.configFile;
in
assert !(nothing ? "kotodama/home-manager.toml");
pkgs.runCommand "kotodama-home-manager-options-test"
  {
    nativeBuildInputs = [ pkgs.python3 ];
    inherit everything noEngines;
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
    })
    expect("empty_query", everything["empty_query"], {
        "most_used": 0,
        "menu": [{"extension": "apps", "item": "firefox"}],
    })
    expect("ranking", everything["ranking"], {"kinds": ["project", "*"]})
    expect("quicklink_defaults", everything["quicklink_defaults"], {"opener": ["firefox", "--new-tab"]})

    extensions = everything["extensions"]
    expect("extensions", sorted(extensions), [
        "ai", "apps", "calculator", "clipboard", "emoji", "folders", "web-search",
    ])
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
    expect("clipboard", extensions["clipboard"], {"max_entries": 50, "max_age_days": 0.5})
    expect("emoji", extensions["emoji"], {"skin_tone": "medium-dark"})
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
    expect("web-search", extensions["web-search"], {
        "engines": [{"name": "Kagi", "url": "https://kagi.com/search?q={query}"}],
    })

    expect("noEngines", load("noEngines"), {"extensions": {"web-search": {"engines": []}}})
    PY
    touch $out
  ''
