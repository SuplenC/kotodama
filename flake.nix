{
  description = "Kotodama: a fast, extensible command menu for Wayland";

  # nixos-unstable is the first channel with Quickshell 0.3.2.
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  # Only for the NixOS VM test of the home-manager module.
  inputs.home-manager = {
    url = "github:nix-community/home-manager";
    inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      home-manager,
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
          kotodama = pkgs.callPackage ./nix/package.nix { };
        in
        {
          default = kotodama;
          inherit kotodama;
          # Each part on its own: the command and the Daemon with its systemd
          # units, the QML module, and the Bundled extensions.
          kotodamad = kotodama.out;
          kotodama-qml = kotodama.qml;
          kotodama-extensions = kotodama.extensions;
        }
      );

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
              cargo-nextest

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
              # A session bus, for the fake notification service
              dbus

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
          };
        }
      );

      formatter = forAllSystems (pkgs: pkgs.nixfmt);

      checks = forAllSystems (pkgs: {
        devShell = self.devShells.${pkgs.stdenv.hostPlatform.system}.default;
        package = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        # Socket activation, the home-manager module, and later the live Hyprland
        # tests (#13): Hyprland needs a DRM device, so they run in a NixOS VM, the
        # way Hyprland tests itself. CI runs it on x86_64, which has KVM.
        nixos-vm = import ./nix/tests/vm.nix { inherit pkgs self home-manager; };
      });
    };
}
