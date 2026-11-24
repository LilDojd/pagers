{ inputs, self, ... }:
{
  perSystem = { pkgs, system, lib, ... }:
    let
      crossTargets = lib.optionalAttrs (system == "x86_64-linux") {
        "x86_64-unknown-linux-musl" = pkgs.pkgsCross.musl64;
        "aarch64-unknown-linux-musl" = pkgs.pkgsCross.aarch64-multiplatform-musl;
        "aarch64-unknown-linux-gnu" = pkgs.pkgsCross.aarch64-multiplatform;
        "armv7-unknown-linux-gnueabihf" = pkgs.pkgsCross.armv7l-hf-multiplatform;
      };

      makeCrossPackage = target: crossPkgs:
        let
          toolchain = inputs.fenix.packages.${system}.fromToolchainFile {
            file = self + /rust-toolchain.toml;
            sha256 = "sha256-MFHoWVcPZKjFrLnQnodCXbUsfZcofGfo9tV9fTTvklg=";
          };
          craneLib = (inputs.crane.mkLib crossPkgs).overrideToolchain (_: toolchain);
          src = craneLib.cleanCargoSource self;

          envTarget = builtins.replaceStrings [ "-" ] [ "_" ] (lib.toUpper target);
        in
        craneLib.buildPackage {
          inherit src;
          inherit (craneLib.crateNameFromCargoToml { cargoToml = self + /Cargo.toml; }) version;
          pname = "pagers";
          strictDeps = true;

          CARGO_BUILD_TARGET = target;
          "CARGO_TARGET_${envTarget}_LINKER" =
            "${crossPkgs.stdenv.cc.targetPrefix}cc";
          HOST_CC = "${pkgs.stdenv.cc}/bin/cc";

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

          nativeBuildInputs = [
            crossPkgs.stdenv.cc
            pkgs.installShellFiles
          ];
        };
    in
    let
      crossPackages = lib.mapAttrs'
        (target: crossPkgs:
          lib.nameValuePair "pagers-${target}" (makeCrossPackage target crossPkgs))
        crossTargets;
    in
    {
      packages = crossPackages;
      checks = crossPackages;
    };
}
