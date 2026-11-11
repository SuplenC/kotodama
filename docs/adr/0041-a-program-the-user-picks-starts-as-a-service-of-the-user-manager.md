# 41. A program the user picks starts as a service of the user manager

Status: accepted (2026-11-11)

Kotodama starts two kinds of process. Extensions belong to the Daemon: they run in its service's cgroup, start and stop with it, and what systemd tells the service is true of them too. Apps, Quicklinks' commands and scripts are different. The user picked them, and they belong to the user's session, not to the Daemon.

Until now the Extension that ran an Action forked the program itself and asked the user manager to move it into a scope (`app-kotodama-<app>-<pid>.scope`). A scope moves a process to another cgroup, but the process is still the Extension's child. It inherits everything the Extension has: the Daemon's service environment (`INVOCATION_ID`, `JOURNAL_STREAM`, `MEMORY_PRESSURE_WATCH` pointing at `kotodamad.service`, and under socket activation `LISTEN_*`), the Extension's working directory (its install directory), its rlimits, ignored signals and OOM score, and any descriptor left without `FD_CLOEXEC`. It also spends the moment between the fork and the move in the Daemon's cgroup. #269 is the case that broke: `cloudflared`, started by T3 Code launched from Kotodama, took the inherited `LISTEN_FDS` for its own and died.

## Decision

**When there is a systemd user manager, the program the user picks is started by the manager, as a transient service, and is never Kotodama's child.** `kotodama-extension`'s launch asks over the manager's private socket for `StartTransientUnit` of `app-kotodama-<app>@<random>.service`, the name systemd's desktop-environment conventions give an app's service (systemd.io/DESKTOP_ENVIRONMENTS, which encourages a service over a scope: "allowing systemd to start the process on behalf of the caller"). `<app>` is the desktop id, else the program's file name, escaped as before.

The unit's properties:

- **`ExecStartEx`**, with the flag `no-env-expand`: the command, its program resolved against the `PATH` of the manager's environment (`Manager.Environment`), the session's, not the Extension's. A program that isn't found there is an error before anything starts. Plain `ExecStart` would replace every `$NAME` in the arguments with the service's variable, so a Quicklink's `$` would not reach its program; specifiers such as `%h` aren't expanded over D-Bus either way.
- **`Type=exec`**: the start job finishes once the program has been executed, so a program that can't be executed fails the launch, in words for the user.
- **`ExitType=cgroup`**: the service lasts while anything in it runs, so a launcher that starts the app and exits leaves the app running. With the default, the service would stop when its main process exits, and kill the app.
- **`Slice=app.slice`**, **`CollectMode=inactive-or-failed`**: as the scopes had.
- **`WorkingDirectory`**: the directory the Action names. Without one, the manager's default, the user's home directory.
- **`Environment`**: nothing, except `WAYLAND_DISPLAY` when the manager's environment has none, found as before in the runtime directory. The program gets the manager's environment, which is the session's: what the compositor or the login put there. It gets its own `INVOCATION_ID` and `MEMORY_PRESSURE_WATCH`, and its output goes to the journal under its own unit.
- **`StandardOutputFileDescriptor`**, **`StandardErrorFileDescriptor`**: for a program in the background only, the pipe whose last line Kotodama reports. The D-Bus client passes descriptors for this.

How the program ends comes from the manager's signals, which it sends to every peer on its private socket: `PropertiesChanged` on the unit's `Service` interface carries `ExecMainCode` and `ExecMainStatus` when the main process exits, and on its `Unit` interface `ActiveState` turns `inactive` or `failed` once nothing in the unit runs. A background program reports when its main process exits, as before. A watched app counts as closed when its unit ends, which replaces polling the scope's `cgroup.procs`.

**Without a user manager**, or with a launch prefix set, nothing changes in kind: the program is a child, started after the prefix if there is one. When the manager refuses the unit, for one older than systemd 250, which has no `ExitType`, the program starts as a child too, and the refusal is logged. `SERVICE_PREFIX` needs systemd 254 for `--expand-environment`. A child starts in the home directory, not the Extension's, unless the Action names a directory.

**Extensions keep inheriting from the Daemon**, which is right for them, except the four `LISTEN_*` variables, which the Daemon takes out of its environment once read, as `sd_listen_fds(unset_environment=1)` does: they hand one socket to one process.

**Extensions in other languages** that start programs through the launch prefix get `systemd-run --user --slice=app.slice --collect --quiet --same-dir --property=Type=exec --property=ExitType=cgroup --expand-environment=no` from the Daemon, so they start services too: `systemd-run` without `--scope` has the manager start the program, in the manager's environment. In `kotodama-protocol`, `SCOPE_PREFIX` becomes `SERVICE_PREFIX`, and `LAUNCH_SCOPED` (`KOTODAMA_LAUNCH_SCOPED`), which marks a prefix that only stands in for this, becomes `LAUNCH_AS_SERVICE` (`KOTODAMA_LAUNCH_AS_SERVICE`). Nothing is released yet, so the names change rather than keep a lie for compatibility.

## Cost

One more round trip on the manager's private socket per launch, for its environment, and the manager forks the program instead of the Extension. None of it is on the keystroke path: it happens after Enter. A program in the background or a watched app keeps one connection to the manager, where it kept a thread waiting on its child, until it ends or the watch's deadline passes.

## Considered options

- **Removing the service's variables before each launch.** Rejected: a list of what systemd sets today. `MANAGERPIDFDID` and `MEMORY_PRESSURE_*` came in recent versions, and the next will add more. It leaves the working directory, descriptors and limits inherited too.
- **Clearing the environment and copying the manager's into the child.** Rejected: right about the environment only, and it does by hand what the manager does when it starts the program.
- **Keeping scopes.** Rejected: a scope can only hold a process its caller started, so the program stays the caller's child, with all it inherits.
- **Resolving the program against the Extension's `PATH`.** Rejected: an Extension may run with a `PATH` of its own, as a Nix wrapper gives it, and the program should be found as the session would find it.
