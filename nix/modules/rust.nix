{ inputs, self, ... }:
{
  perSystem = { pkgs, lib, system, ... }:
    let
      craneLib = inputs.crane.mkLib pkgs;
      src = craneLib.cleanCargoSource self;

      commonArgs = {
        inherit src;
        pname = "pagers";
        version = "0.1.0";
        strictDeps = true;
        nativeBuildInputs = [ pkgs.clang pkgs.mold ];
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
