{ inputs, ... }:
{
  perSystem = { config, self', pkgs, lib, ... }:
    {
      devShells.default = pkgs.mkShell {
        name = "pagers-shell";
        inputsFrom = [
          self'.packages.pagers
          config.pre-commit.devShell
        ];
        packages = with pkgs; [
          vmtouch
          just
          nixd # Nix language server
          bacon
          cargo-edit
          cargo-nextest
          cargo-machete
          clippy
          cargo-autoinherit
          cargo-flamegraph
          mold
          samply
          perf
          clang
          git-cliff
        ];
      };
    };
}
