# Kotodama, built once from the whole Cargo workspace and split into outputs:
# - out: the `kotodama` command, the Daemon and its systemd user units;
# - qml: the `Kotodama` QML module, in lib/qt-6/qml;
# - extensions: the Bundled extensions, in share/kotodama/extensions.
{
  lib,
  rustPlatform,
  qt6,
  pkg-config,
  libGL,
  makeWrapper,
}:

let
  # cxx-qt-build finds Qt through qmake, and needs one prefix with every module.
  qt = qt6.env "kotodama-qt-${qt6.qtbase.version}" [ qt6.qtdeclarative ];
in
rustPlatform.buildRustPackage {
  pname = "kotodama";
  version = (lib.importTOML ../Cargo.toml).workspace.package.version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
    ];
  };
  cargoLock.lockFile = ../Cargo.lock;

  outputs = [
    "out"
    "qml"
    "extensions"
  ];

  nativeBuildInputs = [
    pkg-config
    makeWrapper
  ];
  buildInputs = [
    qt
    libGL
  ];
  env.QMAKE = "${qt}/bin/qmake";
  # Quickshell loads the QML module as a plugin: there is no Qt app to wrap.
  dontWrapQtApps = true;
  # CI runs the tests in the dev shell, where they have the processes and the
  # headless compositor they need.
  doCheck = false;

  postInstall = ''
    # The QML module, laid out as a QML import path expects (as scripts/qml-module does).
    module=$qml/lib/qt-6/qml/Kotodama
    mkdir -p $module
    qmldir=$(find target -path '*/qml_modules/Kotodama/qmldir' | head -n 1)
    cp "$qmldir" "$(dirname "$qmldir")/plugin.qmltypes" $module/
    mv $out/lib/libkotodama_qml.so $module/libKotodama.so
    rmdir $out/lib

    # Each Bundled extension's manifest runs its binary, beside it.
    for name in apps clipboard; do
      dir=$extensions/share/kotodama/extensions/$name
      mkdir -p $dir
      cp crates/kotodama-$name/manifest.toml $dir/
      mv $out/bin/kotodama-$name $dir/
    done

    # The Daemon finds the Bundled extensions on XDG_DATA_DIRS. They go first,
    # before the caller's directories or, when it has none, the default ones,
    # which an Extension such as apps reads too.
    wrapProgram $out/bin/kotodamad \
      --run 'export XDG_DATA_DIRS='"$extensions"'/share:''${XDG_DATA_DIRS:-/usr/local/share:/usr/share}'

    mkdir -p $out/lib/systemd/user
    cp crates/kotodamad/systemd/kotodamad.socket $out/lib/systemd/user/
    substitute crates/kotodamad/systemd/kotodamad.service $out/lib/systemd/user/kotodamad.service \
      --replace-fail @bindir@ $out/bin
  '';

  meta = {
    description = "A fast, extensible command menu for Wayland";
    homepage = "https://github.com/SuplenC/kotodama";
    license = lib.licenses.asl20;
    platforms = lib.platforms.linux;
    mainProgram = "kotodama";
    outputsToInstall = [
      "out"
      "qml"
      "extensions"
    ];
  };
}
