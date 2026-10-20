# The home-manager module's `extensions.ai` options: evaluates two home
# configurations and checks the `[extensions.ai]` table each writes, next to
# the user's own `settings`. No VM: the file is all there is to check.
{
  pkgs,
  self,
  home-manager,
}:

let
  # The config file one `programs.kotodama` writes.
  configFile =
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
    }).config.xdg.configFile."kotodama/home-manager.toml".source;

  # Every setting, the key from a file.
  everything = configFile {
    settings.extensions.apps.terminal = [ "foot" ];
    extensions.ai = {
      apiKeyFile = "/run/agenix/anthropic";
      url = "http://localhost:11434/v1/messages";
      model = "llama3.2";
      effort = "";
      debounceMs = 250;
    };
  };
  # Only a key command: the rest is left out, for the Extension's defaults.
  keyCommand = configFile {
    extensions.ai.apiKeyCommand = [
      "pass"
      "show"
      "anthropic"
    ];
  };
in
pkgs.runCommand "kotodama-home-manager-ai-test"
  {
    nativeBuildInputs = [ pkgs.python3 ];
    inherit everything keyCommand;
    cat = "${pkgs.coreutils}/bin/cat";
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
    expect("everything", everything["extensions"]["ai"], {
        "api_key_command": [os.environ["cat"], "/run/agenix/anthropic"],
        "url": "http://localhost:11434/v1/messages",
        "model": "llama3.2",
        "effort": "",
        "debounce_ms": 250,
    })
    expect("everything, settings", everything["extensions"]["apps"], {"terminal": ["foot"]})

    key_command = load("keyCommand")
    expect("keyCommand", key_command, {
        "extensions": {"ai": {"api_key_command": ["pass", "show", "anthropic"]}},
    })
    PY
    touch $out
  ''
