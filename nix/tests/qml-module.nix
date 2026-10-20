# Quickshell loads the packaged QML module with nothing but the store, so the
# plugin has to find its Qt libraries through its own runpath, and makes a
# KotodamaSearch. The e2e tests load the module cargo builds instead.
{ pkgs, kotodama }:

pkgs.runCommand "kotodama-qml-module-test"
  {
    nativeBuildInputs = [ pkgs.quickshell ];
    QML_IMPORT_PATH = "${kotodama.qml}/lib/qt-6/qml";
    QT_QPA_PLATFORM = "offscreen";
  }
  ''
    export HOME=$TMPDIR XDG_RUNTIME_DIR=$TMPDIR
    cat >shell.qml <<'QML'
    import QtQml
    import Quickshell
    import Kotodama

    ShellRoot {
        KotodamaSearch { id: search }
        Component.onCompleted: console.log("loaded: " + (search !== null))
    }
    QML

    # Quickshell keeps running once the shell is up.
    quickshell --path shell.qml >log 2>&1 &
    for _ in $(seq 300); do
        if grep -q 'loaded: true' log || ! kill -0 $! 2>/dev/null; then
            break
        fi
        sleep 0.1
    done
    kill $! 2>/dev/null || true
    cat log
    grep -q 'loaded: true' log
    touch $out
  ''
