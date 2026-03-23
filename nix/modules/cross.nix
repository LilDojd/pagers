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
            sha256 = "sha256-qqF33vNuAdU5vua96VKVIwuc43j4EFeEXbjQ6+l4mO4=";
          };
          craneLib = (inputs.crane.mkLib crossPkgs).overrideToolchain (_: toolchain);
          src = craneLib.cleanCargoSource self;

          envTarget = builtins.replaceStrings [ "-" ] [ "_" ] (lib.toUpper target);
        in
        craneLib.buildPackage {
          inherit src;
          pname = "pagers";
          version = "0.1.0";
          strictDeps = true;

          CARGO_BUILD_TARGET = target;
          "CARGO_TARGET_${envTarget}_LINKER" =
            "${crossPkgs.stdenv.cc.targetPrefix}cc";
          HOST_CC = "${pkgs.stdenv.cc}/bin/cc";

          nativeBuildInputs = [
            crossPkgs.stdenv.cc
          ];
        };
    in
    {
      packages = lib.mapAttrs'
        (target: crossPkgs:
          lib.nameValuePair "pagers-${target}" (makeCrossPackage target crossPkgs))
        crossTargets;
    };
}
