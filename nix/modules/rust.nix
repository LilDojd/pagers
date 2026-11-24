{ inputs, self, ... }:
{
  perSystem = { pkgs, lib, system, ... }:
    let
      craneLib = (inputs.crane.mkLib pkgs).overrideToolchain (
        inputs.fenix.packages.${system}.fromToolchainFile {
          file = self + /rust-toolchain.toml;
          sha256 = "sha256-MFHoWVcPZKjFrLnQnodCXbUsfZcofGfo9tV9fTTvklg=";
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

        # The build script writes man pages and completions here instead of into
        # Cargo's `OUT_DIR`, whose location under `target` is not stable.
        preBuild = ''
          export SHELL_COMPLETIONS_DIR="$NIX_BUILD_TOP/shell-completions"
        '';

        postInstall = ''
          installManPage "$SHELL_COMPLETIONS_DIR"/*.1
          installShellCompletion \
            --bash "$SHELL_COMPLETIONS_DIR"/pagers.bash \
            --zsh "$SHELL_COMPLETIONS_DIR"/_pagers \
            --fish "$SHELL_COMPLETIONS_DIR"/pagers.fish
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
