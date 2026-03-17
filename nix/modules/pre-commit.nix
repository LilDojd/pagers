{ inputs, ... }:
{
  imports = [
    (inputs.git-hooks + /flake-module.nix)
  ];
  perSystem = { config, pkgs, ... }: {
    pre-commit.settings = {
      package = pkgs.prek;
      hooks = {
        treefmt = {
          enable = true;
          package = config.treefmt.build.wrapper;
        };
        typos.enable = true;
        cargo-check.enable = true;
        check-toml.enable = true;
      };
    };
  };
}
