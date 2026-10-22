# Part of the home-manager module (home-manager.nix): which Extensions it
# installs. `programs.kotodama.extensions.<name>.enable`, for each Bundled one
# in nix/extensions.nix with its default there, `extraExtensions`, packages of
# others, `strictExtensions`, how those are checked, and `finalPackage`, the package with those alone, which the module
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
        refuses, by name, one that needs the build-time Qt at run time; with
        `strictExtensions`, also one that needs what Kotodama's own Extensions
        may not.
      '';
    };

    strictExtensions = lib.mkOption {
      type = lib.types.bool;
      default = false;
      example = true;
      description = ''
        Whether `extraExtensions` are checked as the Bundled extensions are:
        refused, by name, when one needs at run time a runpath to GCC's lib
        output, which holds libstdc++, a shell or the full curl, besides the
        build-time Qt, which no Extension may. Off, a script Extension's shell
        or a C++ one's libstdc++ is allowed.
      '';
    };

    finalPackage = lib.mkOption {
      type = lib.types.package;
      readOnly = true;
      default =
        if cfg.package ? withExtensions then
          cfg.package.withExtensions {
            select = extensions: map (name: extensions.${name}) enabled;
            extra = cfg.extraExtensions;
            strict = cfg.strictExtensions;
          }
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
