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
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      (lib.hm.assertions.assertPlatform "programs.kotodama" pkgs lib.platforms.linux)
    ];

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
