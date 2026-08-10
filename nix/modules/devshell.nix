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
          sha256 = "sha256-A1abGIbOtcBSdrUMhDGrER3pRM1hQP4fp9gh3Y4PKc8=";
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

      hawkRelease =
        {
          x86_64-linux = {
            target = "x86_64-unknown-linux-gnu";
            hash = "sha256-+cr7vFd3srAMNiYZuLYq+ULSkAoSGsri5zIQqOv/2Fc=";
          };
          aarch64-linux = {
            target = "aarch64-unknown-linux-gnu";
            hash = "sha256-iWo5o5junMD8tRNUx48rLEe06/Eo5w9RmeKQVwff0f8=";
          };
        }.${system};

      hawkToolchain =
        (inputs.fenix.packages.${system}.toolchainOf {
          channel = "1.97.1";
          sha256 = "sha256-A1abGIbOtcBSdrUMhDGrER3pRM1hQP4fp9gh3Y4PKc8=";
        }).minimalToolchain;

      cargo-hawk = pkgs.stdenv.mkDerivation {
        pname = "cargo-hawk";
        version = "0.1.12";
        src = pkgs.fetchurl {
          url = "https://github.com/astral-sh/hawk/releases/download/0.1.12/cargo-hawk-${hawkRelease.target}.tar.gz";
          inherit (hawkRelease) hash;
        };

        nativeBuildInputs = [ pkgs.autoPatchelfHook pkgs.makeWrapper ];
        buildInputs = [ pkgs.stdenv.cc.cc.lib ];
        autoPatchelfIgnoreMissingDeps = [ "librustc_driver-*.so" ];

        installPhase = ''
          runHook preInstall
          install -Dm755 cargo-hawk cargo-hawk-driver -t $out/bin
          wrapProgram $out/bin/cargo-hawk --prefix PATH : ${lib.makeBinPath [ hawkToolchain ]}
          runHook postInstall
        '';
      };
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
            hyperfine
            just
            nixd
            bacon
            cargo-edit
            cargo-nextest
            cargo-machete
            clippy
            cargo-autoinherit
            gnupg
            cargo-flamegraph
            samply
            typos
            omnix
          ]
          ++ lib.optionals pkgs.stdenv.isLinux [
            cargo-hawk
            perf
          ];
      };
    };
}
