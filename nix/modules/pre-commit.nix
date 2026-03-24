{ inputs, self, ... }:
{
  imports = [
    (inputs.git-hooks + /flake-module.nix)
  ];
  perSystem = { config, pkgs, ... }: {
    pre-commit.settings = {
      package = pkgs.prek;
      settings.rust.check.cargoDeps = pkgs.rustPlatform.importCargoLock {
        lockFile = self + /Cargo.lock;
      };
      hooks = {
        treefmt = {
          enable = true;
          package = config.treefmt.build.wrapper;
        };
        nixpkgs-fmt.enable = true;
        typos.enable = true;
        cargo-check.enable = true;
        check-toml.enable = true;
      };
    };
  };
}
