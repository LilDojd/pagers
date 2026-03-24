{ inputs, self, ... }:
{
  perSystem =
    { config
    , self'
    , pkgs
    , lib
    , system
    , ...
    }:
    let
      craneLib = (inputs.crane.mkLib pkgs).overrideToolchain (
        inputs.fenix.packages.${system}.fromToolchainFile {
          file = self + /rust-toolchain.toml;
          sha256 = "sha256-qqF33vNuAdU5vua96VKVIwuc43j4EFeEXbjQ6+l4mO4=";
        }
      );

      devShell =
        if pkgs.stdenv.isLinux
        then
          craneLib.devShell.override
            {
              mkShell = pkgs.mkShell.override {
                stdenv = pkgs.stdenvAdapters.useMoldLinker pkgs.stdenv;
              };
            }
        else craneLib.devShell;
    in
    {
      devShells.default = devShell {
        meta.description = "Development shell for pagers";
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
            samply
            git-cliff
            typos
            inputs.omnix.packages.${system}.default
          ]
          ++ lib.optionals pkgs.stdenv.isLinux [
            perf
          ];
      };
    };
}
