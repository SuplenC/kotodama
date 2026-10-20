# Trying Kotodama on Hyprland

How to run Kotodama on your own Hyprland desktop with home-manager, and what to check by hand once it runs. The checks are the ones CI can't cover, from #1.

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
