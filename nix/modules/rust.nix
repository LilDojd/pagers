{ inputs, self, ... }:
{
  perSystem = { pkgs, lib, system, ... }:
    let
      craneLib = (inputs.crane.mkLib pkgs).overrideToolchain (
        inputs.fenix.packages.${system}.fromToolchainFile {
          file = self + /rust-toolchain.toml;
          sha256 = "sha256-qqF33vNuAdU5vua96VKVIwuc43j4EFeEXbjQ6+l4mO4=";
        }
      );

      src = craneLib.cleanCargoSource self;

      commonArgs = {
        inherit src;
        pname = "pagers";
        version = "0.1.0";
        strictDeps = true;

        nativeBuildInputs = lib.optionals pkgs.stdenv.isLinux [
          pkgs.clang
          pkgs.mold
        ];

        buildInputs = lib.optionals pkgs.stdenv.isDarwin [
          pkgs.libiconv
        ];
      };

      cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      pagers = craneLib.buildPackage (commonArgs // {
        inherit cargoArtifacts;
      });
    in
    {
      packages.pagers = pagers;
      packages.default = pagers;

      checks = {
        pagers-clippy = craneLib.cargoClippy (commonArgs // {
          inherit cargoArtifacts;
          cargoClippyExtraArgs = "--all-targets -- --deny warnings";
        });

        pagers-nextest = craneLib.cargoNextest (commonArgs // {
          inherit cargoArtifacts;
        });
      };
    };
}
