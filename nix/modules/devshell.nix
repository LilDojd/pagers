{
  perSystem =
    { config
    , self'
    , pkgs
    , lib
    , ...
    }:
    {
      devShells.default = pkgs.mkShell {
        name = "pagers-shell";
        inputsFrom = [
          self'.packages.pagers
          config.pre-commit.devShell
        ];

        packages =
          with pkgs;
          [
            vmtouch
            just
            nixd
            bacon
            cargo-edit
            cargo-nextest
            cargo-machete
            clippy
            cargo-autoinherit
            cargo-flamegraph
            mold
            samply
            clang
            git-cliff
          ]
          ++ lib.optionals pkgs.stdenv.isLinux [
            perf
          ];
      };
    };
}
