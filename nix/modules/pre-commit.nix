{ inputs, self, ... }:
{
  imports = [
    inputs.git-hooks.flakeModule
  ];
  perSystem = { config, pkgs, lib, ... }: {
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
        cargo-check = {
          enable = true;
          extraPackages = lib.optionals pkgs.stdenv.isLinux [
            pkgs.clang
            pkgs.mold
          ];
        };
        check-toml.enable = true;
      };
    };
  };
}
