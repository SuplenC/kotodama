# The Bundled extensions, each built from crates/kotodama-<name> into a package
# of its own (nix/package.nix), and whether the home-manager module installs it
# unasked (`programs.kotodama.extensions.<name>.enable`). Those on by default
# are of use on any desktop with nothing to set up. Of the rest, folders and ai
# do nothing until configured, hyprland needs Hyprland, and system powers off
# and logs out, which a user should choose to have one keystroke away.
{
  apps = true;
  calculator = true;
  clipboard = true;
  emoji = true;
  web-search = true;
  ai = false;
  folders = false;
  hyprland = false;
  system = false;
}
