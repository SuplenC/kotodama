// The ready Front end: the menu alone, run as a Quickshell instance of its own
// beside whatever shell the desktop already runs, with the module's default
// look. The Nix package installs it in share/kotodama/quickshell; the
// home-manager module's `frontEnd` runs it, or a copy with the look set.
import Quickshell
import Kotodama

ShellRoot {
    KotodamaMenu {}
}
