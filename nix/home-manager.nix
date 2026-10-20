# The home-manager module: `programs.kotodama`. It installs Kotodama, writes the
# user's settings into ~/.config/kotodama/, and enables the Daemon's socket, so
# the first `kotodama toggle` starts the Daemon.
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
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      (lib.hm.assertions.assertPlatform "programs.kotodama" pkgs lib.platforms.linux)
      {
        assertion = ai.apiKeyFile == null || ai.apiKeyCommand == null;
        message = "programs.kotodama.extensions.ai: set apiKeyFile or apiKeyCommand, not both.";
      }
    ];

    programs.kotodama.settings = lib.mkIf (aiSettings != { }) { extensions.ai = aiSettings; };

    # The command, and the QML module for Quickshell to import.
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
  };
}
