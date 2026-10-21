{
  description = "Kotodama: a fast, extensible command menu for Wayland";

  # nixos-unstable is the first channel with Quickshell 0.3.2.
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  # Only for the NixOS VM test of the home-manager module.
  inputs.home-manager = {
    url = "github:nix-community/home-manager";
    inputs.nixpkgs.follows = "nixpkgs";
  };
  # Builds the package's dependencies in a derivation of their own, which
  # changes only with the lockfile, so Cachix serves it and a change to the
  # workspace rebuilds only the workspace's crates.
  inputs.crane.url = "github:ipetkov/crane";

  outputs =
    {
      self,
      nixpkgs,
      home-manager,
      crane,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (
        pkgs:
        let
          kotodama = pkgs.callPackage ./nix/package.nix { craneLib = crane.mkLib pkgs; };
        in
        {
          default = kotodama;
          inherit kotodama;
          # Each part on its own: the command and the Daemon with its systemd
          # units, the QML module, and the Bundled extensions.
          kotodamad = kotodama.out;
          kotodama-qml = kotodama.qml;
          kotodama-extensions = kotodama.extensions;
          # Development only: `scripts/fmt` fetches it on demand. The dev shell's
          # rustfmt, built in nightly mode, so it reads the unstable options in
          # .config/rustfmt.toml. It comes from the same source as the shell's
          # rustc, so flake.lock pins it and no second toolchain is fetched. Out
          # of the dev shell, so only the jobs that format pay for it.
          rustfmt-nightly = pkgs.rustfmt.override { asNightly = true; };
        }
      );

      # `pkgs.kotodama`, built with the nixpkgs it is applied to: with the
      # nixpkgs a Quickshell comes from, the QML module uses that Quickshell's Qt.
      overlays.default = final: _prev: {
        kotodama = final.callPackage ./nix/package.nix { craneLib = crane.mkLib final; };
      };

      homeModules.default = import ./nix/home-manager.nix self;
      # The name most home-manager configurations still use.
      homeManagerModules = self.homeModules;

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
          # cxx-qt-build links the QML module with lld when it finds an `lld`
          # command, else with the deprecated gold; gcc's -fuse-ld=lld then runs
          # `ld.lld`. Both names point at Nix's wrapped lld, so the binary keeps
          # its runpath to Qt (bare pkgs.lld drops it, #32). Only these two
          # names: `ld`, `ar` and the rest stay GNU.
          wrappedLld = pkgs.runCommand "kotodama-lld" { } ''
            mkdir -p $out/bin
            ln -s ${pkgs.llvmPackages.bintools}/bin/ld.lld $out/bin/ld.lld
            ln -s ld.lld $out/bin/lld
          '';
        in
        rec {
          default = pkgs.mkShell {
            packages = with pkgs; [
              # Rust
              cargo
              rustc
              clippy
              rustfmt
              rust-analyzer
              cargo-nextest
              # The instruction-counting benchmarks: gungraun runs them under Valgrind.
              valgrind
              (callPackage ./nix/gungraun-runner.nix { })

              # Qt 6 and what cxx-qt needs to build against it
              qt
              libGL # QtGui links libGLX and libOpenGL
              pkg-config
              cmake
              wrappedLld

              # Front end and headless Wayland for e2e tests
              quickshell
              sway-unwrapped
              grim
              imagemagick
              wtype
              # A session bus, for the fake notification service
              dbus

              # The shell example Extension
              jq

              # The calculator: exchange rates through curl, and a time zone
              # database the tests read the same everywhere
              curl
              tzdata

              just
              nixfmt
              shellcheck
              actionlint
            ];

            # cxx-qt-build finds Qt through qmake.
            QMAKE = "${qt}/bin/qmake";
            # Fortify needs optimisation, so it only warns on every C++ file of a debug build.
            hardeningDisable = [ "fortify" ];
            # One font, rendered the same on every machine. makeFontsConf would also
            # read the host's /etc/fonts and /usr/share/fonts, which differ between
            # the dev container and a CI runner.
            FONTCONFIG_FILE = pkgs.writeText "kotodama-fonts.conf" ''
              <?xml version="1.0"?>
              <!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
              <fontconfig>
                <dir>${pkgs.dejavu_fonts}/share/fonts</dir>
                <cachedir prefix="xdg">fontconfig</cachedir>
                <match target="font">
                  <edit name="antialias" mode="assign"><bool>true</bool></edit>
                  <edit name="hinting" mode="assign"><bool>true</bool></edit>
                  <edit name="hintstyle" mode="assign"><const>hintslight</const></edit>
                  <edit name="rgba" mode="assign"><const>none</const></edit>
                  <edit name="lcdfilter" mode="assign"><const>lcdnone</const></edit>
                  <edit name="embeddedbitmap" mode="assign"><bool>false</bool></edit>
                </match>
              </fontconfig>
            '';
            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
            TZDIR = "${pkgs.tzdata}/share/zoneinfo";
          };

          # `just demo` (scripts/demo): the dev shell, plus a VNC server to see
          # the headless desktop from outside, and a few apps to find and
          # launch. A shell of its own, so CI's shell doesn't carry them.
          demo = default.overrideAttrs (old: {
            nativeBuildInputs =
              old.nativeBuildInputs
              ++ (with pkgs; [
                wayvnc
                # A terminal, a Qt editor, and a `Terminal=true` app that runs in the terminal.
                foot
                featherpad
                htop
                # The theme the apps' icons are in, which they ship no index for.
                hicolor-icon-theme
              ]);
            # The test font by its generic names, which the menu and foot ask
            # for, plus color emoji for the emoji Extension's Results.
            FONTCONFIG_FILE = pkgs.writeText "kotodama-demo-fonts.conf" ''
              <?xml version="1.0"?>
              <!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
              <fontconfig>
                <dir>${pkgs.dejavu_fonts}/share/fonts</dir>
                <dir>${pkgs.noto-fonts-color-emoji}/share/fonts</dir>
                <cachedir prefix="xdg">fontconfig</cachedir>
                <alias><family>sans-serif</family><prefer><family>DejaVu Sans</family><family>Noto Color Emoji</family></prefer></alias>
                <alias><family>serif</family><prefer><family>DejaVu Serif</family><family>Noto Color Emoji</family></prefer></alias>
                <alias><family>monospace</family><prefer><family>DejaVu Sans Mono</family><family>Noto Color Emoji</family></prefer></alias>
                <match target="pattern">
                  <test qual="all" name="family" compare="not_eq"><string>sans-serif</string></test>
                  <test qual="all" name="family" compare="not_eq"><string>serif</string></test>
                  <test qual="all" name="family" compare="not_eq"><string>monospace</string></test>
                  <edit name="family" mode="append_last"><string>sans-serif</string></edit>
                </match>
              </fontconfig>
            '';
          });
        }
      );

      formatter = forAllSystems (pkgs: pkgs.nixfmt);

      checks = forAllSystems (pkgs: {
        devShell = self.devShells.${pkgs.stdenv.hostPlatform.system}.default;
        package = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        # The same derivation as `package` on this nixpkgs: checks the overlay evaluates.
        overlay = (pkgs.extend self.overlays.default).kotodama;
        qml-module = import ./nix/tests/qml-module.nix {
          inherit pkgs;
          kotodama = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        };
        # Socket activation and the home-manager module, in a NixOS VM. CI runs
        # it on x86_64, which has KVM.
        nixos-vm = import ./nix/tests/vm.nix { inherit pkgs self home-manager; };
        # The `[extensions.ai]` the home-manager module's typed options write.
        home-manager-ai = import ./nix/tests/home-manager-ai.nix { inherit pkgs self home-manager; };
        # The tables the rest of the module's typed options write.
        home-manager-options = import ./nix/tests/home-manager-options.nix {
          inherit pkgs self home-manager;
        };
        # The Hyprland Extension live: Hyprland needs a DRM device, so it runs
        # in a NixOS VM with a virtio GPU, the way Hyprland tests itself.
        hyprland-vm = import ./nix/tests/hyprland.nix { inherit pkgs self home-manager; };
      });
    };
}
