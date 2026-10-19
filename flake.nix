{
  description = "Kotodama: a fast, extensible command menu for Wayland";

  # nixos-unstable is the first channel with Quickshell 0.3.2.
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      # The only toolchain definition in the repo. Docker, CI and `just` all enter this shell.
      devShells = forAllSystems (
        pkgs:
        let
          # One Qt prefix with every module, so qmake (and so cxx-qt-build) finds them all.
          qt = pkgs.qt6.env "kotodama-qt-${pkgs.qt6.qtbase.version}" (
            with pkgs.qt6;
            [
              qtdeclarative
              qtwayland
            ]
          );
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              # Rust
              cargo
              rustc
              clippy
              rustfmt
              rust-analyzer

              # Qt 6 and what cxx-qt needs to build against it
              qt
              libGL # QtGui links libGLX and libOpenGL
              pkg-config
              cmake

              # Front end and headless Wayland for e2e tests
              quickshell
              sway-unwrapped
              grim
              imagemagick
              wtype

              # The shell example Extension
              jq

              just
              nixfmt
              shellcheck
              actionlint
            ];

            # cxx-qt-build finds Qt through qmake.
            QMAKE = "${qt}/bin/qmake";
            # Fortify needs optimisation, so it only warns on every C++ file of a debug build.
            hardeningDisable = [ "fortify" ];
            # One font, so text renders the same on every machine.
            FONTCONFIG_FILE = pkgs.makeFontsConf { fontDirectories = [ pkgs.dejavu_fonts ]; };
            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          };
        }
      );

      formatter = forAllSystems (pkgs: pkgs.nixfmt);

      checks = forAllSystems (
        pkgs:
        {
          devShell = self.devShells.${pkgs.stdenv.hostPlatform.system}.default;
        }
        # Placeholder for the live Hyprland tests (#13). Hyprland needs a DRM device,
        # so they run in a NixOS VM, the way Hyprland tests itself.
        // nixpkgs.lib.optionalAttrs (pkgs.stdenv.hostPlatform.system == "x86_64-linux") {
          nixos-vm = pkgs.testers.runNixOSTest {
            name = "kotodama-vm";
            nodes.machine = { };
            testScript = ''
              machine.wait_for_unit("multi-user.target")
            '';
          };
        }
      );
    };
}
