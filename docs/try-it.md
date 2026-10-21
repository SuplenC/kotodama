# Trying Kotodama on Hyprland

How to run Kotodama on your own Hyprland desktop with home-manager, and what to check by hand once it runs. To look at it first without a Wayland desktop, on a Mac say, see [Try it on macOS](#try-it-on-macos). The checks are the ones CI can't cover, from #1.

## Try it on macOS

`just demo` runs Kotodama on a Linux desktop of its own, in the dev container, and shows it to you over VNC. It works the same on Linux, with Nix and without a container.

```sh
just demo           # state starts fresh every run
just demo --keep    # History, clipboard history and the made-up folders kept between runs
```

The first run fetches the toolchain and builds a release, which takes a while. Then Screen Sharing opens `vnc://localhost:5900`; log in as `kotodama`, password `kotodama`. The server listens on localhost only. Ctrl+C in the terminal stops everything.

What's on the desktop: headless sway with the Daemon, the Bundled extensions, the [shell example Extension](../examples/shell-extension/README.md) and the shipped menu. The menu is open at the start, over a background that explains how Kotodama works and lists queries to try for each feature, each one written to work in the demo.

- **Alt+Space** or **Ctrl+Space** opens and closes the menu. Super doesn't make it through Screen Sharing, and macOS may keep Ctrl+Space for switching input sources.
- **Alt+V** opens the clipboard history; **Alt+B** the shell example's bookmarks View.
- **Alt+Return** opens a terminal (foot); **Alt+Shift+Q** closes a window.

Type `guide` in the menu for the whole list: the "Kotodama demo guide" Quicklink opens [demo/GUIDE.md](../demo/GUIDE.md) in FeatherPad, with every feature and something to try for each.

The demo makes up things for every Extension to find, in a home directory of its own:

- apps: foot, FeatherPad and htop, which opens in foot;
- `~/Projects`: a few git repositories, one with a linked worktree and two both named `api`, opened in tmux, lazygit or foot from the Action panel;
- `~/Scripts`: scripts that show each header option (a name, a background job, arguments asked for in a form);
- a few clipboard entries;
- Aliases (`ed`, `aur`) and Quicklinks (`gh`, `yt`, `say`, `guide`), in [demo/config.toml](../demo/config.toml);
- the shell example's bookmarks and timer.

For the AI Extension, pass your Claude API key; it reaches the container's environment and nothing else, and the demo's config reads it from there:

```sh
ANTHROPIC_API_KEY=sk-ant-... just demo
```

Then type `?` and a request in plain words, such as `? open a terminal`.

What doesn't work in the demo:

- The Hyprland Extension: the desktop is sway, so there are no windows or Hyprland shortcuts to find.
- Anything that leaves the desktop: there's no browser, so a web search, a bookmark or a URL Quicklink opens a terminal that names the page it would have opened, and there's no notification daemon.
- Your own desktop: apps, files and clipboard are the demo's, not your Mac's.
- The feel: the menu is drawn in software and sent over VNC, so it is slower than on real hardware.

Logs are in the dev container's cargo volume; `just shell` and look in `$CARGO_TARGET_DIR/demo-logs`.

## Install

Add the flake and its home-manager module:

```nix
{
  inputs.kotodama.url = "github:SuplenC/kotodama";
}
```

```nix
# in your home-manager configuration
{ inputs, ... }:
{
  imports = [ inputs.kotodama.homeModules.default ];

  programs.kotodama = {
    enable = true;
    # The menu, in a Quickshell of its own, started with the graphical session.
    frontEnd.enable = true;
    # Optional: the look, by KotodamaMenu's property names. Unset, its defaults.
    # frontEnd.look = { background = "#1d2021"; accent = "#d79921"; fontFamily = "Inter"; };

    # Optional: something to find besides apps.
    extensions.folders.folders = [ { path = "~/Projects"; kind = "projects"; } ];
    settings.quicklinks.gh = { title = "GitHub"; url = "https://github.com"; };
  };
}
```

This builds Kotodama, and runs the menu's Quickshell, from Kotodama's own pinned nixpkgs, the one CI tests. If your desktop's Qt comes from another nixpkgs, that is a second Qt on disk (about 760 MiB) and nothing worse, since the menu runs in a Quickshell of its own. To share your Qt instead, add `inputs.kotodama.inputs.nixpkgs.follows = "nixpkgs";` ([Installing](../README.md#installing) has the details); that nixpkgs' Quickshell then runs the menu, so try the pinned one first. The package's dependencies are in the `kotodama` Cachix cache for x86_64 (`cachix use kotodama`).

Then `home-manager switch` (or `nixos-rebuild switch`, with home-manager as a NixOS module).

## Hyprland

The Daemon and the menu are systemd user services, so they see the user manager's environment, not Hyprland's. Hand it over first thing:

```
exec-once = dbus-update-activation-environment --systemd --all
```

uwsm does this for you, and so does home-manager's `wayland.windowManager.hyprland` with its systemd integration (on by default).

The menu's service starts with `graphical-session.target`. uwsm and home-manager's Hyprland module start that target. If you start Hyprland some other way, nothing does, so start the menu yourself, after the import:

```
exec-once = dbus-update-activation-environment --systemd --all && systemctl --user start kotodama-menu
```

The binds:

```
bind = SUPER, Space, exec, kotodama toggle
bind = SUPER, V, exec, kotodama open --view clipboard/history
bind = SUPER, F1, exec, kotodama open --view hyprland/shortcuts
```

`SUPER+Space` is often taken already, by a launcher or a keyboard layout switch, and Hyprland runs both binds. To try Kotodama next to what you have, bind a free key first, such as `SUPER CTRL, Space`, and move it to `SUPER+Space` once you're ready to switch. The same goes for `SUPER+V` and `SUPER+F1`.

`hyprctl reload` applies the binds. The environment import runs only when Hyprland starts, so log out and back in once.

## Is it running?

```sh
systemctl --user status kotodamad.socket kotodamad kotodama-menu
journalctl --user -u kotodama-menu -u kotodamad -f
```

`kotodamad.socket` is always listening. `kotodamad` starts the first time something connects, which is the menu as soon as it starts. `kotodama-menu` is running once the session is up. If it is "inactive (dead)" with a failed condition, the user manager had no `WAYLAND_DISPLAY` when it tried; check the import above, then `systemctl --user start kotodama-menu`.

`systemctl --user show-environment | grep -E 'WAYLAND_DISPLAY|HYPRLAND_INSTANCE_SIGNATURE|DBUS_SESSION_BUS_ADDRESS'` should list all three. If the Daemon started before they arrived, `systemctl --user restart kotodamad`.

For more detail in the logs, including how long each query took from the keystroke to the Results on screen:

```sh
systemctl --user set-environment KOTODAMA_LOG=debug
systemctl --user restart kotodamad kotodama-menu
# and afterwards
systemctl --user unset-environment KOTODAMA_LOG
systemctl --user restart kotodamad kotodama-menu
```

## Updating

```sh
nix flake update kotodama
home-manager switch
systemctl --user restart kotodamad kotodama-menu
```

The restart makes sure both run the new build.

## Manual checks

What CI can't cover, from #1's "Manual checks on real hardware". Tick them off in #1, and note in a comment what failed and how.

Some need extras: the shell example Extension for the Views checks (link `examples/shell-extension` from the flake into `~/.config/kotodama/extensions/`, as its [README](../examples/shell-extension/README.md) says, with `jq` installed), and the `exec-once` and binds above.

### Setup and session

- [ ] Quickshell loads the QML module from the Nix output (`kotodama-menu` is running and `kotodama toggle` shows the menu).
- [ ] `WAYLAND_DISPLAY` and `DBUS_SESSION_BUS_ADDRESS` reach the systemd user manager.
- [ ] #59: under socket activation, start the Daemon before Hyprland; the clipboard records copies and paste works.
- [ ] #59: apps launched from the menu start while the Daemon has no `WAYLAND_DISPLAY`.
- [ ] Apps survive a kotodamad restart.
- [ ] Notifications reach a real notification daemon.

### Typing and pasting

- [ ] #59: typing into Qt and GTK apps loses no first keys (50 ms settle after the menu closes).
- [ ] Emoji: multi-part (🧑‍🚒, 🍋‍🟩, flags, toned) and ⏏️ typed into GTK, Qt, Electron and terminal apps.

### Clipboard

- [ ] `bind = SUPER, V, exec, kotodama open --view clipboard/history`: opens on the list at the first press after login, Esc goes to the search.
- [ ] The password manager's `x-kde-passwordManagerHint` keeps its copies out of clipboard history.

### Hyprland

- [ ] #59: restart Hyprland; windows and binds come back without restarting the Daemon.
- [ ] Window icons resolve from the class.
- [ ] Rapid title changes send one window at a time.
- [ ] `SUPER+F1` → `kotodama open --view hyprland/shortcuts` opens the View in the first frame.
- [ ] A bind in a submap.
- [ ] "Move to Workspace" with a named workspace.
- [ ] The feel of Views and the Action panel.

### Views

- [ ] `kotodama open --view shell-example/bookmarks` opens the shell example's bookmarks; typing filters; Ctrl+K shows contributed Actions; Esc clears, then pops.
- [ ] A wizard's pop-then-push never shows the search.
- [ ] Restart kotodamad with a View open and it comes back.

### History

- [ ] Forget, and undo by typing `undo`.

### Web search

- [ ] `weather` shows search rows at the bottom and Enter opens the browser.
- [ ] `example.com` and `localhost:8080` put "Open …" first.
- [ ] An app name that fills the screen shows no search rows.
- [ ] `main.rs` shows no "Open" row.

### Calculator

- [ ] Copy.
- [ ] The frankfurter currency fetch.
- [ ] Time zones.

### Folders

- [ ] Ghostty, tmux and lazygit openers.
