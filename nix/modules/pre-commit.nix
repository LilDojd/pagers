{ inputs, ... }:
{
  imports = [
    (inputs.git-hooks + /flake-module.nix)
  ];
  perSystem = { config, ... }: {
    pre-commit.settings = {
      hooks = {
        treefmt = {
          enable = true;
          package = config.treefmt.build.wrapper;
        };
      };
    };
  };
}
