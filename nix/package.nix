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
  curl,
}:

let
  # cxx-qt-build finds Qt through qmake, and needs one prefix with every module.
  qtModules = [ qt6.qtdeclarative ];
  qt = qt6.env "kotodama-qt-${qt6.qtbase.version}" qtModules;
  # Where the QML module finds those modules and QtCore at run time.
  qtRunpath = lib.makeLibraryPath ([ qt6.qtbase ] ++ qtModules);
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
  env = {
    QMAKE = "${qt}/bin/qmake";
    # The calculator fetches exchange rates with curl, by this path.
    KOTODAMA_CURL = lib.getExe curl;
  };
  # Quickshell loads the QML module as a plugin: there is no Qt app to wrap.
  dontWrapQtApps = true;
  # The build-time Qt environment stays out of every output (see postInstall).
  disallowedReferences = [ qt ];
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
    # The linker found Qt in the one-prefix environment above, so the module's
    # runpath names it, and the environment holds every Qt module's dev output
    # (#52). Point the runpath at the Qt libraries themselves: a Quickshell from
    # the same nixpkgs already has those store paths.
    rpath=$(patchelf --print-rpath $module/libKotodama.so)
    patchelf --set-rpath "''${rpath//"${qt}/lib"/"${qtRunpath}"}" $module/libKotodama.so

    # Each Bundled extension's manifest runs its binary, beside it.
    for name in apps calculator clipboard folders; do
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
