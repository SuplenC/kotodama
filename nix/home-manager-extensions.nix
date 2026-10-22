# Part of the home-manager module (home-manager.nix): which Extensions it
# installs. `programs.kotodama.extensions.<name>.enable`, for each Bundled one
# in nix/extensions.nix with its default there, `extraExtensions`, packages of
# others, and `finalPackage`, the package with those alone, which the module
# installs.
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

    extraExtensions = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [ ];
      example = lib.literalExpression "[ pkgs.kotodama-notes ]";
      description = ''
        Extensions that aren't Bundled, each a package holding
        {file}`share/kotodama/extensions/<name>/` with its manifest, installed
        beside the enabled Bundled extensions. `package`'s `withExtensions`
        checks them as it checks those: it refuses one that needs at run time
        what no Extension may, such as a runpath to GCC's lib output, which
        holds libstdc++, or a shell.
      '';
    };

    finalPackage = lib.mkOption {
      type = lib.types.package;
      readOnly = true;
      default =
        if cfg.package ? withExtensions then
          cfg.package.withExtensions (
            extensions: map (name: extensions.${name}) enabled ++ cfg.extraExtensions
          )
        else
          cfg.package;
      defaultText = lib.literalMD "`package` with the enabled Bundled extensions and `extraExtensions`";
      description = "The package the module installs: `package` with the enabled Bundled extensions and `extraExtensions` alone.";
    };
  };

  config = lib.mkIf cfg.enable {
    # A package without `withExtensions` is installed as it is, which would
    # leave them out.
    assertions = [
      {
        assertion = cfg.package ? withExtensions || cfg.extraExtensions == [ ];
        message = "programs.kotodama.extraExtensions: `package` has no `withExtensions` to install them with.";
      }
    ];

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
