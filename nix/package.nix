# Kotodama, built once from the whole Cargo workspace and split into outputs:
# - out: the `kotodama` command, the Daemon and its systemd user units;
# - qml: the `Kotodama` QML module, in lib/qt-6/qml;
# - extensions: the Bundled extensions, in share/kotodama/extensions.
{
  lib,
  stdenv,
  rustPlatform,
  runCommand,
  qt6,
  pkg-config,
  libGL,
  makeBinaryWrapper,
  curl,
  curlMinimal,
  runtimeShellPackage,
}:

let
  # cxx-qt-build finds Qt through qmake, and needs one prefix with every module.
  qtModules = [ qt6.qtdeclarative ];
  qt = qt6.env "kotodama-qt-${qt6.qtbase.version}" qtModules;
  # Where the QML module finds those modules and QtCore at run time.
  qtRunpath = lib.makeLibraryPath ([ qt6.qtbase ] ++ qtModules);

  # The calculator fetches exchange rates with curl, by this path. HTTPS is all
  # it needs, so curlMinimal, without HTTP/3, brotli, zstd or libpsl. Only the
  # `curl` binary is copied: curl's bin output also holds wcurl, a shell script
  # that would bring in bash.
  fetcher = runCommand "kotodama-curl-${curlMinimal.version}" { } ''
    install -D -m 555 ${lib.getExe curlMinimal} $out/bin/curl
  '';

  # A Rust binary needs libgcc_s and nothing else from GCC, but the linker
  # leaves a runpath to GCC's `lib` output, which also holds libstdc++ (10 MiB).
  # GCC's `libgcc` output holds libgcc_s alone.
  gccLib = "${lib.getLib stdenv.cc.cc}/lib";
  libgcc = "${stdenv.cc.cc.libgcc}/lib";

  # What the command, the Daemon and the Bundled extensions must never need at
  # run time: the build-time Qt, libstdc++, a shell, or the full curl.
  notAtRunTime = [
    qt
    (lib.getLib stdenv.cc.cc)
    runtimeShellPackage
    curl.bin
    curlMinimal.bin
  ];
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
    makeBinaryWrapper
  ];
  buildInputs = [
    qt
    libGL
  ];
  env = {
    QMAKE = "${qt}/bin/qmake";
    KOTODAMA_CURL = "${fetcher}/bin/curl";
  };
  # Quickshell loads the QML module as a plugin: there is no Qt app to wrap.
  dontWrapQtApps = true;
  # outputChecks, which checks each output on its own, needs structured attributes.
  __structuredAttrs = true;
  outputChecks = {
    out.disallowedRequisites = notAtRunTime;
    extensions.disallowedRequisites = notAtRunTime;
    # The QML module runs in Quickshell, whose Qt already brings libstdc++ and
    # bash. It must just not hold the build-time Qt environment (see postInstall).
    qml.disallowedReferences = [ qt ];
  };
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
    for name in apps calculator clipboard emoji folders hyprland web-search; do
      dir=$extensions/share/kotodama/extensions/$name
      mkdir -p $dir
      cp crates/kotodama-$name/manifest.toml $dir/
      mv $out/bin/kotodama-$name $dir/
    done

    # The Daemon finds the Bundled extensions on XDG_DATA_DIRS. They go first,
    # before the caller's directories or, when it has none, the default ones,
    # which an Extension such as apps reads too. The wrapper is compiled, so it
    # needs no shell.
    wrapProgram $out/bin/kotodamad \
      --set-default XDG_DATA_DIRS /usr/local/share:/usr/share \
      --prefix XDG_DATA_DIRS : $extensions/share

    mkdir -p $out/lib/systemd/user
    cp crates/kotodamad/systemd/kotodamad.socket $out/lib/systemd/user/
    substitute crates/kotodamad/systemd/kotodamad.service $out/lib/systemd/user/kotodamad.service \
      --replace-fail @bindir@ $out/bin
  '';

  # After fixupPhase has shrunk each runpath to the directories it uses.
  postFixup = ''
    for bin in $out/bin/kotodama $out/bin/.kotodamad-wrapped $extensions/share/kotodama/extensions/*/kotodama-*; do
      rpath=$(patchelf --print-rpath "$bin")
      patchelf --set-rpath "''${rpath//"${gccLib}"/"${libgcc}"}" "$bin"
    done
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
