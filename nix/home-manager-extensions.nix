# Part of the home-manager module (home-manager.nix): which Bundled extensions
# it installs. `programs.kotodama.extensions.<name>.enable`, for each one in
# nix/extensions.nix with its default there, and `finalPackage`, the package
# with those alone, which the module installs.
{
  config,
  lib,
  ...
}:

let
  cfg = config.programs.kotodama;
  bundled = import ./extensions.nix;
  enabled = lib.filter (name: cfg.extensions.${name}.enable) (lib.attrNames bundled);
in
{
  options.programs.kotodama = {
    extensions = lib.mapAttrs (name: default: {
      enable = lib.mkOption {
        type = lib.types.bool;
        inherit default;
        example = !default;
        description = ''
          Whether to install the ${name} Extension, which the Daemon then
          starts. Off, it isn't installed, so it never runs, whatever
          `[extensions.${name}]` holds.
        '';
      };
    }) bundled;

    finalPackage = lib.mkOption {
      type = lib.types.package;
      readOnly = true;
      default =
        if cfg.package ? withExtensions then
          cfg.package.withExtensions (extensions: map (name: extensions.${name}) enabled)
        else
          cfg.package;
      defaultText = lib.literalMD "`package` with the enabled Bundled extensions";
      description = "The package the module installs: `package` with the enabled Bundled extensions alone.";
    };
  };

  config = lib.mkIf cfg.enable {
    # Settings for an Extension left off would otherwise go unnoticed.
    warnings =
      map
        (
          name:
          "programs.kotodama: `[extensions.${name}]` is set, but the ${name} Extension isn't installed; set programs.kotodama.extensions.${name}.enable = true to install it."
        )
        (
          lib.filter (name: !cfg.extensions.${name}.enable && cfg.settings ? extensions.${name}) (
            lib.attrNames bundled
          )
        );
  };
}
