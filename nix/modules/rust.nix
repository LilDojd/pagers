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

      inherit (craneLib.crateNameFromCargoToml { cargoToml = self + /Cargo.toml; }) version;

      commonArgs = {
        inherit src version;
        pname = "pagers";
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
        meta.description = "A tool for monitoring page cache usage";

        postInstall = ''
          buildDir=$(find target -path '*/build/pagers-*/out' -type d -exec test -f '{}/pagers.bash' \; -print -quit)
          if [ -n "$buildDir" ]; then
            installManPage "$buildDir"/*.1
            installShellCompletion \
              --bash "$buildDir"/pagers.bash \
              --zsh "$buildDir"/_pagers \
              --fish "$buildDir"/pagers.fish
          fi
        '';

        nativeBuildInputs = (commonArgs.nativeBuildInputs or [ ]) ++ [
          pkgs.installShellFiles
        ];
      });
    in
    {
      packages.pagers = pagers;
      packages.default = pagers;

      checks = {
        pagers-clippy = craneLib.cargoClippy (commonArgs // {
          inherit cargoArtifacts;
          cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          meta.description = "Run clippy lints on the workspace";
        });

        pagers-nextest = craneLib.cargoNextest (commonArgs // {
          inherit cargoArtifacts;
          meta.description = "Run tests with cargo-nextest";
        });
      };
    };
}
