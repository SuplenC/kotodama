# Kotodama as separate packages, each versioned by its own crate:
# - core: the `kotodama` command, the Daemon and its systemd user units;
# - qml: the `Kotodama` QML module, in lib/qt-6/qml, and the ready Front end,
#   a Quickshell config running the menu, in share/kotodama/quickshell;
# - extensions.<name>: each Bundled extension, in share/kotodama/extensions/<name>;
# - full: the three put together, with outputs out, qml and extensions, whose
#   `kotodamad` finds the Extensions given to `withExtensions`.
# Each package builds only its own crates. Their dependencies build first, in
# one derivation they all share (crane's buildDepsOnly). It changes only with
# the lockfile and the crates' manifests, so Cachix serves it, and a change to
# the workspace's code rebuilds only the packages whose crates changed.
{
  lib,
  stdenv,
  craneLib,
  runCommand,
  qt6,
  pkg-config,
  libGL,
  makeBinaryWrapper,
  curl,
  curlMinimal,
  runtimeShellPackage,
  quickshell,
}:

let
  # cxx-qt-build finds Qt through qmake, and needs one prefix with every module.
  qtModules = [ qt6.qtdeclarative ];
  qt = qt6.env "kotodama-qt-${qt6.qtbase.version}" qtModules;
  # Where the QML module finds those modules and QtCore at run time.
  qtRunpath = lib.makeLibraryPath ([ qt6.qtbase ] ++ qtModules);

  # The calculator and the AI Extension run curl by this path. HTTPS is all
  # they need, so curlMinimal, without HTTP/3, brotli, zstd or libpsl. Only the
  # `curl` binary is copied: curl's bin output also holds wcurl, a shell script
  # that would bring in bash.
  curlOnly = runCommand "kotodama-curl-${curlMinimal.version}" { } ''
    install -D -m 555 ${lib.getExe curlMinimal} $out/bin/curl
  '';

  # A Rust binary needs libgcc_s and nothing else from GCC, but the linker
  # leaves a runpath to GCC's `lib` output, which also holds libstdc++ (10 MiB).
  # GCC's `libgcc` output holds libgcc_s alone.
  gccLibDir = "${lib.getLib stdenv.cc.cc}/lib";
  libgccOnlyDir = "${stdenv.cc.cc.libgcc}/lib";

  # What the command, the Daemon and the Bundled extensions must never need at
  # run time: the build-time Qt, libstdc++, a shell, or the full curl.
  notAtRunTime = [
    qt
    (lib.getLib stdenv.cc.cc)
    runtimeShellPackage
    curl.bin
    curlMinimal.bin
  ];

  bundled = lib.attrNames (import ./extensions.nix);
  versionOf = crate: (lib.importTOML ../crates/${crate}/Cargo.toml).package.version;

  # The crates each package builds, as cargo's `-p` arguments. The shared
  # dependencies derivation builds the same sets, one after another: cargo
  # picks each dependency's features from the crates being built, so a
  # dependency built for the whole workspace at once would not match what one
  # package asks for, and would build again in every package.
  cargoPackages = {
    core = "-p kotodama -p kotodamad";
    qml = "-p kotodama-qml";
  }
  // lib.genAttrs bundled (name: "-p kotodama-${name}");

  # What the dependencies and every package build with alike.
  commonArgs = {
    src = lib.fileset.toSource {
      root = ../.;
      fileset = lib.fileset.unions [
        ../Cargo.toml
        ../Cargo.lock
        ../crates
      ];
    };
    strictDeps = true;
    nativeBuildInputs = [ pkg-config ];
    buildInputs = [
      qt
      libGL
    ];
    # cxx-qt-build finds Qt through qmake.
    env.QMAKE = "${qt}/bin/qmake";
    # Quickshell loads the QML module as a plugin: there is no Qt app to wrap.
    dontWrapQtApps = true;
    # CI runs the tests in the dev shell, where they have the processes and the
    # headless compositor they need.
    doCheck = false;
  };

  cargoArtifacts = craneLib.buildDepsOnly (
    commonArgs
    // {
      pname = "kotodama";
      version = versionOf "kotodamad";
      # Only `cargo build`, once per package: no `cargo check` pass first, as
      # the packages build nothing but release artifacts.
      buildPhaseCargoCommand = lib.concatMapStringsSep "\n" (
        args: "cargoWithProfile build --locked ${args}"
      ) (lib.attrValues cargoPackages);
      # Where it built, for the packages to find in the build scripts' outputs.
      postBuild = ''
        pwd >target/.deps-build-dir
      '';
    }
  );

  # One package: the crates `cargoPackages.${part}` names, built on the shared
  # dependencies. `outputChecks` and `postInstall` say what goes where.
  build =
    part:
    {
      pname,
      version,
      outputChecks,
      postInstall,
      meta,
      passthru ? { },
    }:
    craneLib.buildPackage (
      commonArgs
      // {
        inherit
          pname
          version
          cargoArtifacts
          passthru
          ;
        cargoExtraArgs = "--locked ${cargoPackages.${part}}";

        # The dependencies' build scripts left absolute paths to the directory
        # they built in: cargo's link paths, and cxx-qt's manifests and include
        # symlinks, which the QML module's build script follows. A sandboxed
        # build always runs in /build, but without the sandbox (as in the dev
        # container) each build has a directory of its own, so point those
        # paths here. Each file keeps its mtime: a newer one would make cargo
        # rebuild what reads it.
        preBuild = ''
          depsDir=$(<target/.deps-build-dir)
          if [ "$depsDir" != "$PWD" ]; then
            echo "pointing the dependencies' build outputs from $depsDir to $PWD"
            grep -rlIF --null "$depsDir" target |
              while IFS= read -r -d "" file; do
                sed "s|$depsDir|$PWD|g" "$file" >"$file.new"
                touch -r "$file" "$file.new"
                mv "$file.new" "$file"
              done
            find target -type l -lname "$depsDir/*" -print0 |
              while IFS= read -r -d "" link; do
                dest=$(readlink "$link")
                ln -sfn "$PWD''${dest#"$depsDir"}" "$link.new"
                touch -h -r "$link" "$link.new"
                mv -T "$link.new" "$link"
              done
          fi
        '';

        env = commonArgs.env // {
          KOTODAMA_CURL = "${curlOnly}/bin/curl";
        };
        # outputChecks needs structured attributes, so the build's variables
        # come from `env` above.
        __structuredAttrs = true;
        inherit outputChecks;

        postInstall = ''
          # replaceRunpath FILE FROM TO: replace the directory FROM in FILE's runpath with TO.
          replaceRunpath() {
            local rpath
            rpath=":$(patchelf --print-rpath "$1"):"
            rpath=''${rpath//":$2:"/":$3:"}
            rpath=''${rpath#:}
            patchelf --set-rpath "''${rpath%:}" "$1"
          }
        ''
        + postInstall;

        meta = {
          homepage = "https://github.com/SuplenC/kotodama";
          license = lib.licenses.asl20;
          platforms = lib.platforms.linux;
        }
        // meta;
      }
    );

  core = build "core" {
    pname = "kotodama-core";
    version = versionOf "kotodamad";
    outputChecks.out.disallowedRequisites = notAtRunTime;
    postInstall = ''
      for bin in $out/bin/*; do
        replaceRunpath "$bin" ${gccLibDir} ${libgccOnlyDir}
      done

      mkdir -p $out/lib/systemd/user
      cp crates/kotodamad/systemd/kotodamad.socket $out/lib/systemd/user/
      substitute crates/kotodamad/systemd/kotodamad.service $out/lib/systemd/user/kotodamad.service \
        --replace-fail @bindir@ $out/bin
    '';
    meta = {
      description = "Kotodama's `kotodama` command and Daemon, with no Extensions";
      mainProgram = "kotodama";
    };
  };

  qml = build "qml" {
    pname = "kotodama-qml";
    version = versionOf "kotodama-qml";
    # The QML module runs in Quickshell, whose Qt already brings libstdc++ and
    # bash. It must just not hold the build-time Qt environment.
    outputChecks.out.disallowedReferences = [ qt ];
    postInstall = ''
      # The QML module, laid out as a QML import path expects (as scripts/qml-module does).
      module=$out/lib/qt-6/qml/Kotodama
      mkdir -p $module
      qmldir=$(find target -path '*/qml_modules/Kotodama/qmldir' | head -n 1)
      cp "$qmldir" "$(dirname "$qmldir")/plugin.qmltypes" $module/
      mv $out/lib/libkotodama_qml.so $module/libKotodama.so
      # The linker found Qt in the one-prefix environment above, so the module's
      # runpath names it, and the environment holds every Qt module's dev output
      # (#52). Point the runpath at the Qt libraries themselves: a Quickshell from
      # the same nixpkgs already has those store paths.
      replaceRunpath $module/libKotodama.so ${qt}/lib ${qtRunpath}

      # The ready Front end: `quickshell --path $out/share/kotodama/quickshell`,
      # with the module's import path as QML_IMPORT_PATH.
      install -D -m 444 crates/kotodama-qml/shell/shell.qml $out/share/kotodama/quickshell/shell.qml
    '';
    # The Quickshell from the same nixpkgs, whose Qt the QML module links against.
    passthru = { inherit quickshell; };
    meta.description = "Kotodama's Quickshell Front end: the `Kotodama` QML module and the ready menu";
  };

  # Each Bundled extension's manifest runs its binary, beside it.
  extensions = lib.genAttrs bundled (
    name:
    build name {
      pname = "kotodama-${name}";
      version = versionOf "kotodama-${name}";
      outputChecks.out.disallowedRequisites = notAtRunTime;
      postInstall = ''
        dir=$out/share/kotodama/extensions/${name}
        mkdir -p $dir
        cp crates/kotodama-${name}/manifest.toml $dir/
        mv $out/bin/kotodama-${name} $dir/
        rmdir $out/bin
        replaceRunpath $dir/kotodama-${name} ${gccLibDir} ${libgccOnlyDir}
      '';
      meta.description = "Kotodama's ${name} Extension";
    }
  );

  # The core, the Front end and the Extensions `select` picks from the Bundled
  # ones (`exts: [ exts.apps exts.emoji ]`; any package with
  # share/kotodama/extensions/<name> will do), as the outputs out, qml and
  # extensions. Its `kotodamad` finds those Extensions and no other Bundled one.
  withExtensions =
    select:
    let
      chosen = select extensions;
    in
    runCommand "kotodama-${core.version}"
      {
        pname = "kotodama";
        inherit (core) version;
        outputs = [
          "out"
          "qml"
          "extensions"
        ];
        nativeBuildInputs = [ makeBinaryWrapper ];
        __structuredAttrs = true;
        outputChecks = {
          out.disallowedRequisites = notAtRunTime;
          extensions.disallowedRequisites = notAtRunTime;
        };
        passthru = {
          inherit quickshell withExtensions;
          inherit core;
          # Every Bundled extension's package, by name, and those this one holds.
          extensionPackages = extensions;
          installedExtensions = chosen;
        };
        meta = core.meta // {
          description = "A fast, extensible command menu for Wayland";
          outputsToInstall = [
            "out"
            "qml"
            "extensions"
          ];
        };
      }
      ''
        mkdir -p $extensions/share/kotodama/extensions
        for package in ${lib.escapeShellArgs chosen}; do
          ln -s "$package"/share/kotodama/extensions/* $extensions/share/kotodama/extensions/
        done

        mkdir -p $qml
        ln -s ${qml}/lib ${qml}/share $qml/

        mkdir -p $out/bin $out/lib/systemd/user
        ln -s ${core}/bin/kotodama $out/bin/kotodama
        # The Daemon finds the Extensions on XDG_DATA_DIRS. These go first,
        # before the caller's directories or, when the variable is unset, the
        # default ones, which an Extension such as apps reads too. The wrapper
        # is compiled, so it needs no shell. Unlike the shell's `:-`, it keeps
        # an empty XDG_DATA_DIRS as it is.
        makeBinaryWrapper ${core}/bin/kotodamad $out/bin/kotodamad \
          --set-default XDG_DATA_DIRS /usr/local/share:/usr/share \
          --prefix XDG_DATA_DIRS : $extensions/share
        # A runCommand has no fixup phase to drop the runpath to GCC's
        # libraries, which the wrapper doesn't use.
        patchelf --shrink-rpath $out/bin/kotodamad
        cp ${core}/lib/systemd/user/kotodamad.socket $out/lib/systemd/user/
        substitute ${core}/lib/systemd/user/kotodamad.service $out/lib/systemd/user/kotodamad.service \
          --replace-fail ${core}/bin $out/bin
      '';
in
{
  inherit
    core
    qml
    extensions
    withExtensions
    ;
  full = withExtensions lib.attrValues;
}
