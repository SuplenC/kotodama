# Packaging Kotodama

What a distro package installs, how to build it without Nix, and what it needs at run time. The Nix packages in `nix/package.nix` are the reference: this page describes the same layout. For which compositors Kotodama works on, see [Compositor support](try-it.md#compositor-support).

## The packages

Kotodama ships as separate packages, each versioned by its own crate ([Installing](../README.md#installing), #161). A distro can split it the same way:

| Package | Crates | Installs |
| --- | --- | --- |
| `kotodama-core` | `kotodama`, `kotodamad` | `bin/kotodama`, `bin/kotodamad`, `lib/systemd/user/kotodamad.{socket,service}` |
| `kotodama-qml` | `kotodama-qml` | `lib/qt-6/qml/Kotodama/{qmldir,plugin.qmltypes,libKotodama.so}`, `share/kotodama/quickshell/shell.qml` |
| `kotodama-<name>` | `kotodama-<name>` | `share/kotodama/extensions/<name>/{manifest.toml,kotodama-<name>}` |

The Bundled extensions are `apps`, `calculator`, `clipboard`, `emoji`, `web-search`, `ai`, `folders`, `hyprland` and `system` (`nix/extensions.nix`). Paths are relative to the install prefix, such as `/usr`.

## Building

Each package builds only its own crates:

```sh
cargo build --release --locked -p kotodama -p kotodamad   # core
cargo build --release --locked -p kotodama-qml            # QML module
cargo build --release --locked -p kotodama-<name>         # one Bundled extension
```

The QML module is built by cxx-qt (`crates/kotodama-qml/build.rs`), which finds Qt through `qmake`. Set `QMAKE` to the Qt 6 `qmake` if it isn't the first on `PATH`. The build needs Qt 6 with QtDeclarative, `pkg-config` and libGL headers.

`scripts/qml-module --release` builds the module and lays it out as a QML import path expects, then prints that path:

```
<target>/qml/release/Kotodama/{qmldir,plugin.qmltypes,libKotodama.so}
```

To do it by hand: copy `qmldir` and `plugin.qmltypes` from `target/cxxqt/qml_modules/Kotodama/`, and `target/release/libkotodama_qml.so` renamed to `libKotodama.so`, into `<prefix>/lib/qt-6/qml/Kotodama/`. The `qmldir` names the plugin `Kotodama`, so the file name matters.

The calculator and the AI Extension run `curl` for HTTPS. `KOTODAMA_CURL` at build time sets the path they run it by.

## Installing

- **Binaries**: `kotodama` and `kotodamad` in `bin/`.
- **Extensions**: each Bundled extension's `manifest.toml` and binary go side by side in `share/kotodama/extensions/<name>/`; the manifest runs the binary beside it. The Daemon finds Extensions under `kotodama/extensions/` in every directory of `XDG_DATA_DIRS`, so `/usr/share` works with no setup.
- **systemd user units**: `crates/kotodamad/systemd/kotodamad.socket` as it is, and `kotodamad.service` with `@bindir@` replaced by the directory `kotodamad` is installed in (`/usr/bin`). Both go in the user unit directory (`/usr/lib/systemd/user/`).
- **QML module**: `lib/qt-6/qml/Kotodama/` in the Nix package. A distro puts it in its own Qt 6 QML directory instead (often `/usr/lib/qt6/qml/Kotodama/`), where Quickshell finds it with no `QML_IMPORT_PATH`.
- **Ready Front end**: `crates/kotodama-qml/shell/shell.qml` in `share/kotodama/quickshell/`.

## Socket activation

`kotodamad.socket` listens on `$XDG_RUNTIME_DIR/kotodama.sock`. The Daemon starts on the first connection (the first `kotodama toggle`, or a Front end coming up) and keeps running. Enable the socket for every user, as a package would:

```sh
systemctl --global enable kotodamad.socket
```

The Daemon runs under the user manager, so it sees the user manager's environment. The compositor must hand over `WAYLAND_DISPLAY` and `DBUS_SESSION_BUS_ADDRESS` (`dbus-update-activation-environment --systemd --all`, or uwsm). The Daemon finds the compositor without `WAYLAND_DISPLAY` too, but apps it launches need it.

## Run time

- **Core and Extensions**: libc and libgcc_s. No Qt, no shell. The calculator and the AI Extension need `curl`.
- **Front end**: Quickshell, and the same Qt 6 that Quickshell links against. The module is a Qt plugin, so it must be built against that Qt; one built against another Qt version may not load.
- **Running the ready menu**: `quickshell --path <prefix>/share/kotodama/quickshell`, with `QML_IMPORT_PATH=<prefix>/lib/qt-6/qml` if the module isn't in Qt's own QML directory. The home-manager module runs it as the `kotodama-menu` user service (`nix/home-manager.nix`), started with `graphical-session.target` and only when `WAYLAND_DISPLAY` is set. A distro can ship a similar unit, or leave it to the user's own shell: a Quickshell config that does `import Kotodama` and places `KotodamaMenu {}` is a Front end too. Run one or the other, not both ([Several menus at once](try-it.md#several-menus-at-once)).
