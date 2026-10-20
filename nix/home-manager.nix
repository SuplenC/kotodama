# The home-manager module: `programs.kotodama`. It installs Kotodama, writes the
# user's settings into ~/.config/kotodama/, and enables the Daemon's socket, so
# the first `kotodama toggle` starts the Daemon. With `frontEnd.enable`, a
# systemd user service runs the ready Front end, the menu in a Quickshell of its
# own, with the graphical session.
self:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.kotodama;
  toml = pkgs.formats.toml { };
  ai = cfg.extensions.ai;
  # `[extensions.ai]`: only what is set, so the rest keeps the Extension's defaults.
  aiSettings = lib.filterAttrs (_: value: value != null) {
    api_key_command =
      if ai.apiKeyFile != null then
        [
          "${pkgs.coreutils}/bin/cat"
          ai.apiKeyFile
        ]
      else
        ai.apiKeyCommand;
    inherit (ai) url model effort;
    debounce_ms = ai.debounceMs;
  };
  # A setting that, unset, is left out of the config.
  optional =
    type: description:
    lib.mkOption {
      type = lib.types.nullOr type;
      default = null;
      inherit description;
    };
  # The table's settings that are set; the rest keep their defaults.
  set = lib.filterAttrs (_: value: value != null);
  # A list of tables, as the TOML has it: each without its unset settings.
  tables = list: if list == null then null else map set list;
  command = lib.types.nonEmptyListOf lib.types.str;
  ext = cfg.extensions;
  # Every typed table, by its place in the config, with only what is set.
  typed = {
    history = set {
      picks_to_learn = cfg.history.picksToLearn;
      picks_to_take_over = cfg.history.picksToTakeOver;
      half_life_days = cfg.history.halfLifeDays;
      lift_threshold = cfg.history.liftThreshold;
      time_of_day = cfg.history.timeOfDay;
      usage_log = cfg.history.usageLog;
    };
    empty_query = set {
      most_used = cfg.emptyQuery.mostUsed;
      inherit (cfg.emptyQuery) menu;
    };
    ranking = set { inherit (cfg.ranking) kinds; };
    quicklink_defaults = set { inherit (cfg.quicklinkDefaults) opener; };
    extensions = nonEmpty {
      ai = aiSettings;
      apps = set {
        inherit (ext.apps) terminal hidden names;
        withdraw_within = ext.apps.withdrawWithin;
      };
      calculator = set {
        inherit (ext.calculator) network;
        rates_url = ext.calculator.ratesUrl;
        decimal_separator = ext.calculator.decimalSeparator;
      };
      clipboard = set {
        max_entries = ext.clipboard.maxEntries;
        max_age_days = ext.clipboard.maxAgeDays;
      };
      emoji = set { skin_tone = ext.emoji.skinTone; };
      folders = set {
        folders = tables ext.folders.folders;
        inherit (ext.folders)
          terminal
          open
          shell
          lazygit
          ;
        file_manager = ext.folders.fileManager;
        copy_path = ext.folders.copyPath;
      };
      web-search = set { inherit (ext.web-search) engines; };
    };
  };
  # Without its empty tables, so options left unset write nothing.
  nonEmpty = lib.filterAttrs (_: table: table != { });

  # Where Quickshell finds the `Kotodama` QML module.
  qmlImportPath = "${cfg.package.qml}/lib/qt-6/qml";
  frontEnd = cfg.frontEnd;
  # The look properties that are set; the rest keep KotodamaMenu's defaults.
  look = set frontEnd.look;
  # The package's ready Front end or, with a look set, a copy of it whose
  # KotodamaMenu sets those properties. The copy is made from the same file in
  # this flake's source, so writing it needs no build of the package.
  shell = ../crates/kotodama-qml/shell/shell.qml;
  properties = lib.concatStrings (
    lib.mapAttrsToList (name: value: "        ${name}: ${builtins.toJSON value}\n") look
  );
  shellWithLook =
    let
      text = builtins.readFile shell;
      themed =
        builtins.replaceStrings [ "KotodamaMenu {}" ] [ "KotodamaMenu {\n${properties}    }" ]
          text;
    in
    assert lib.assertMsg (
      themed != text
    ) "${toString shell} has no `KotodamaMenu {}` to set the look on.";
    pkgs.writeTextDir "shell.qml" themed;
  frontEndConfig =
    if look == { } then "${cfg.package.qml}/share/kotodama/quickshell" else "${shellWithLook}";
  # home-manager's own `programs.quickshell` service, for a shell of the user's.
  ownQuickshell =
    config.programs.quickshell.enable or false && config.programs.quickshell.systemd.enable;
in
{
  options.programs.kotodama = {
    enable = lib.mkEnableOption "Kotodama, a command menu for Wayland";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "kotodama.packages.\${system}.default";
      description = "The Kotodama package, with its `qml` and `extensions` outputs.";
    };

    settings = lib.mkOption {
      inherit (toml) type;
      default = { };
      example = lib.literalExpression ''
        {
          quicklinks.gh = {
            title = "GitHub";
            url = "https://github.com";
          };
          extensions.apps.terminal = [ "ghostty" "-e" ];
        }
      '';
      description = ''
        The Daemon's config, written to {file}`~/.config/kotodama/home-manager.toml`.
        The Daemon merges every `*.toml` in that directory in filename order,
        so files of the user's own still apply, and reloads it on every change.
      '';
    };

    # The Daemon's own tables, merged into `settings`. An option left unset is
    # left out, so the Daemon's default applies. The Quicklinks and Aliases,
    # keyed by the user's own names, stay in `settings`.
    history = {
      picksToLearn = optional (lib.types.ints.between 1 255) ''
        `picks_to_learn`: the picks of one Result for one query that make it
        the query's Learned choice. Unset, 3.
      '';
      picksToTakeOver = optional (lib.types.ints.between 1 255) ''
        `picks_to_take_over`: the picks of another Result, since the Learned
        choice was last picked, that replace it. Unset, 2.
      '';
      halfLifeDays = optional lib.types.numbers.positive ''
        `half_life_days`: how long a pick takes to count half as much. Unset, 14.
      '';
      liftThreshold = optional lib.types.numbers.positive ''
        `lift_threshold`: the Frecency that lifts a Result one tier, about a
        week of daily use. Unset, 5.0.
      '';
      timeOfDay = optional (lib.types.numbers.between 0 1) ''
        `time_of_day`: how much less picks count at a time of day the Result
        is never picked, from 0 to 1. Unset, 0.5.
      '';
      usageLog = optional lib.types.bool ''
        `usage_log`: log queries and picks to
        {file}`$XDG_STATE_HOME/kotodama/usage.jsonl`, to tune ranking from
        real use. Unset, false.
      '';
    };

    emptyQuery = {
      mostUsed = optional (lib.types.ints.between 0 50) ''
        `most_used`: how many of the Results most used at this time of day the
        Empty query shows first, from 0 to 50. Unset, 3.
      '';
      menu =
        optional
          (lib.types.listOf (
            lib.types.submodule {
              options = {
                extension = lib.mkOption {
                  type = lib.types.str;
                  description = "The name of the Extension that owns the Item.";
                };
                item = lib.mkOption {
                  type = lib.types.str;
                  description = "The Item's id.";
                };
              };
            }
          ))
          "`menu`: the Fixed menu, the Items shown after the most used, in this order, each unless it is shown already.";
    };

    ranking.kinds = optional (lib.types.listOf lib.types.str) ''
      `kinds`: on an equal match, kinds earlier in the list rank first. `*`
      stands for every kind not in it; without it, those rank last. Unset,
      `[ "app" "project" "url" "*" "file" ]`.
    '';

    quicklinkDefaults.opener = optional command ''
      `opener` of `[quicklink_defaults]`: the program, and its first
      arguments, that a URL Quicklink naming no opener opens with. Unset,
      `xdg-open`.
    '';

    extensions.apps = {
      terminal = optional command ''
        `terminal`: what starts apps with `Terminal=true`; the app's command
        follows it. Unset, `xdg-terminal-exec`.
      '';
      hidden = optional (lib.types.listOf lib.types.str) ''
        `hidden`: apps to leave out, by desktop id, with or without `.desktop`.
      '';
      names = optional (lib.types.attrsOf lib.types.str) ''
        `names`: names to show instead of the app's own, by desktop id; it is
        still found by its own name too.
      '';
      withdrawWithin = optional (lib.types.numbers.between 0 30) ''
        `withdraw_within`: an app that closes within this many seconds of
        starting has its pick taken back out of the History. 0 never does; at
        most 30. Unset, 5.
      '';
    };

    extensions.calculator = {
      network = optional lib.types.bool ''
        `network`: fetch exchange rates. Off, currency uses the cached rates,
        however old, and nothing without them. Unset, true.
      '';
      ratesUrl = optional lib.types.str ''
        `rates_url`: where the rates come from: frankfurter's API, or anything
        answering like it. Unset, `https://api.frankfurter.app/latest`.
      '';
      decimalSeparator =
        optional
          (lib.types.enum [
            "dot"
            "comma"
          ])
          "`decimal_separator`: how decimals are written, `dot` (3.5) or `comma` (3,5); the other separator groups thousands. Unset, the locale's.";
    };

    extensions.clipboard = {
      maxEntries = optional lib.types.ints.positive ''
        `max_entries`: past this many entries, the oldest go. Unset, 1000.
      '';
      maxAgeDays = optional lib.types.numbers.positive ''
        `max_age_days`: entries older than this many days go. Unset, 7.
      '';
    };

    extensions.emoji.skinTone = optional (lib.types.enum [
      "light"
      "medium-light"
      "medium"
      "medium-dark"
      "dark"
    ]) "`skin_tone`: the skin tone \"Type\" and \"Copy\" use. Unset, emoji are their default yellow.";

    extensions.folders = {
      folders = optional (lib.types.listOf (
        lib.types.submodule {
          options = {
            path = lib.mkOption {
              # A string, never a path: a path would be copied into the Nix store.
              type = lib.types.strMatching "~|~?/.*";
              example = "~/Projects";
              description = "The folder: absolute, or under `~/`.";
            };
            kind = lib.mkOption {
              type = lib.types.enum [
                "projects"
                "scripts"
              ];
              description = "`projects`, the git repositories in it, or `scripts`, its executables.";
            };
            depth = optional lib.types.ints.unsigned ''
              How far below the folder projects are looked for; `projects`
              only. Unset, 3.
            '';
          };
        }
      )) "`folders`: the folders, each with its kind.";
      terminal = optional command ''
        `terminal`: what a script runs in; the script and its arguments
        follow. It should keep the window open once the script exits. Unset,
        `[ "ghostty" "--wait-after-command=true" "-e" ]`.
      '';
      open = optional command ''
        `open`: what a project's "Open" runs. In every project command,
        `{path}` is the project's folder and `{session}` its tmux session.
        Unset, ghostty running tmux with nvim.
      '';
      shell = optional command ''
        `shell`: what "Open Terminal Here" runs. Unset,
        `[ "ghostty" "--working-directory={path}" ]`.
      '';
      lazygit = optional command ''
        `lazygit`: what "Open in Lazygit" runs. Unset,
        `[ "ghostty" "-e" "lazygit" "--path" "{path}" ]`.
      '';
      fileManager = optional command ''
        `file_manager`: what "Open in File Manager" runs. Unset,
        `[ "xdg-open" "{path}" ]`.
      '';
      copyPath = optional command ''
        `copy_path`: what "Copy Path" runs. Unset, `[ "wl-copy" "{path}" ]`.
      '';
    };

    extensions.web-search.engines =
      optional
        (lib.types.listOf (
          lib.types.submodule {
            options = {
              name = lib.mkOption {
                type = lib.types.strMatching ".*[^[:space:]].*";
                description = "What the engine is called.";
              };
              url = lib.mkOption {
                type = lib.types.strMatching ".*[{]query[}].*";
                description = "The URL template, with `{query}` where the query goes, percent-encoded.";
              };
            };
          }
        ))
        "`engines`: searched in this order; an empty list leaves only addresses. Unset, DuckDuckGo, Google and Wikipedia.";

    # The AI Extension's `[extensions.ai]`, merged into `settings`. With neither
    # a key nor `url` set, the Extension is off.
    extensions.ai = {
      apiKeyCommand = lib.mkOption {
        type = lib.types.nullOr (lib.types.nonEmptyListOf lib.types.str);
        default = null;
        example = [
          "pass"
          "show"
          "anthropic"
        ];
        description = ''
          `api_key_command`: the command that prints the API key on its first
          line. Its output is kept in memory only: never logged, never written
          to disk, and handed to curl on stdin.
        '';
      };
      apiKeyFile = lib.mkOption {
        # A string, never a path: a path would be copied into the Nix store.
        type = lib.types.nullOr (lib.types.strMatching "/.*");
        default = null;
        example = "/run/agenix/anthropic";
        description = ''
          The absolute path of a file whose first line is the API key, such as
          an agenix or sops-nix secret. Only the path goes into the Nix store:
          the key is read at runtime, through
          `api_key_command = [ "cat" path ]`. Not together with `apiKeyCommand`.
        '';
      };
      url = optional lib.types.str ''
        `url`: a Messages API endpoint. A local server that speaks the same API,
        such as Ollama (`http://localhost:11434/v1/messages`), works too, with
        no key. Unset, `https://api.anthropic.com/v1/messages`.
      '';
      model = optional lib.types.str ''
        `model`. Unset, `claude-haiku-5-5`, the fastest Claude model, as you
        wait on it with the menu open.
      '';
      effort = optional lib.types.str ''
        `effort`: `output_config.effort`; `""` leaves it out, for a server that
        doesn't take it. Unset, `low`.
      '';
      debounceMs = optional lib.types.ints.unsigned ''
        `debounce_ms`: how long, in milliseconds, a `?` query must be still
        before it is sent. Unset, 600.
      '';
    };

    qmlImportPath = lib.mkOption {
      type = lib.types.str;
      readOnly = true;
      default = qmlImportPath;
      defaultText = lib.literalExpression ''"''${config.programs.kotodama.package.qml}/lib/qt-6/qml"'';
      description = ''
        The QML import path that holds the `Kotodama` module, for a Quickshell
        config of the user's own that uses `KotodamaMenu` or `KotodamaSearch`:
        give it to that Quickshell as `QML_IMPORT_PATH`. home-manager's
        `programs.quickshell` service gets it without asking.
      '';
    };

    # The ready Front end: the menu as a Quickshell instance of its own.
    frontEnd = {
      enable = lib.mkEnableOption ''
        the ready Front end: a systemd user service, `kotodama-menu`, that runs
        the menu in a Quickshell instance of its own, beside whatever shell the
        desktop already runs, with the graphical session'';

      quickshellPackage = lib.mkOption {
        type = lib.types.package;
        default = cfg.package.quickshell or pkgs.quickshell;
        defaultText = lib.literalExpression "config.programs.kotodama.package.quickshell";
        description = ''
          The Quickshell that runs the menu. The default comes from the same
          nixpkgs as `package`, so it shares the QML module's Qt; one from
          another nixpkgs brings a second Qt.
        '';
      };

      target = lib.mkOption {
        type = lib.types.str;
        default = config.wayland.systemd.target;
        defaultText = lib.literalExpression "config.wayland.systemd.target";
        example = "hyprland-session.target";
        description = ''
          The systemd user target that starts the menu, and stops it when it
          stops. Something must start it once the compositor has handed
          `WAYLAND_DISPLAY` to the user manager: uwsm, or home-manager's
          `wayland.windowManager.hyprland` with its systemd integration.
        '';
      };

      # KotodamaMenu's look properties, by their own names.
      look = {
        background = optional lib.types.str ''The background color, such as `"#1e1e2e"`. Unset, `#1e1e2e`.'';
        foreground = optional lib.types.str "The text color. Unset, `#cdd6f4`.";
        dimmed = optional lib.types.str "The color of secondary text, such as subtitles. Unset, `#a6adc8`.";
        accent = optional lib.types.str "The color of matched characters. Unset, `#89b4fa`.";
        selection = optional lib.types.str "The selected row's background color. Unset, `#313244`.";
        border = optional lib.types.str "The border's color. Unset, `#45475a`.";
        error = optional lib.types.str "The color of a failure message. Unset, `#f38ba8`.";
        fontFamily = optional lib.types.str "The font family. Unset, `sans-serif`.";
        fontSize = optional lib.types.ints.positive "The font size, in pixels. Unset, 15.";
        radius = optional lib.types.number "The corner radius, in pixels. Unset, 12.";
        menuWidth = optional lib.types.ints.positive "The menu's width, in pixels. Unset, 640.";
        rowHeight = optional lib.types.ints.positive "A row's height, in pixels. Unset, 44.";
        maxRows = optional lib.types.ints.positive "How many rows show at most. Unset, 8.";
        top = optional lib.types.number "The menu's distance from the top of the screen, in pixels. Unset, a fifth of the screen's height.";
        placeholder = optional lib.types.str "What the empty search box says. Unset, `Search`.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      (lib.hm.assertions.assertPlatform "programs.kotodama" pkgs lib.platforms.linux)
      {
        assertion = ai.apiKeyFile == null || ai.apiKeyCommand == null;
        message = "programs.kotodama.extensions.ai: set apiKeyFile or apiKeyCommand, not both.";
      }
    ];

    programs.kotodama.settings = nonEmpty typed;

    # The command, the QML module and the ready Front end.
    home.packages = [ cfg.package ];

    xdg.configFile = {
      "kotodama/home-manager.toml" = lib.mkIf (cfg.settings != { }) {
        source = toml.generate "kotodama-settings.toml" cfg.settings;
      };
      # The package's own units. Its `kotodamad` puts the Bundled extensions on
      # XDG_DATA_DIRS for itself, so the session's stays as it was.
      "systemd/user/kotodamad.socket".source = "${cfg.package}/lib/systemd/user/kotodamad.socket";
      "systemd/user/kotodamad.service".source = "${cfg.package}/lib/systemd/user/kotodamad.service";
      "systemd/user/sockets.target.wants/kotodamad.socket".source =
        "${cfg.package}/lib/systemd/user/kotodamad.socket";
    };

    systemd.user.services = lib.mkMerge [
      (lib.mkIf frontEnd.enable {
        kotodama-menu = {
          Unit = {
            Description = "Kotodama's menu, a Quickshell Front end";
            Documentation = "https://github.com/SuplenC/kotodama";
            PartOf = [ frontEnd.target ];
            After = [ frontEnd.target ];
            # Without a compositor to draw on, it would only fail.
            ConditionEnvironment = "WAYLAND_DISPLAY";
          };
          Service = {
            ExecStart = "${lib.getExe frontEnd.quickshellPackage} --path ${frontEndConfig}";
            # Only this Quickshell's: the session's QML_IMPORT_PATH stays as it was.
            Environment = [ "QML_IMPORT_PATH=${qmlImportPath}" ];
            Restart = "on-failure";
          };
          Install.WantedBy = [ frontEnd.target ];
        };
      })
      # A Quickshell config of the user's own, run by home-manager's
      # `programs.quickshell` service, finds the module too.
      (lib.mkIf ownQuickshell {
        quickshell.Service.Environment = [ "QML_IMPORT_PATH=${qmlImportPath}" ];
      })
    ];
  };
}
