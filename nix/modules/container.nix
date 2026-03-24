{ inputs, ... }:
{
  perSystem = { self', pkgs, system, lib, ... }:
    let
      nix2container = inputs.nix2container.packages.${system}.nix2container;

      crossPkgs = pkgs.pkgsCross.aarch64-multiplatform;

      containerTargets = lib.optionalAttrs (system == "x86_64-linux") {
        amd64 = {
          arch = "amd64";
          package = self'.packages.pagers;
          rootPkgs = pkgs;
        };
        arm64 = {
          arch = "arm64";
          package = self'.packages.pagers-aarch64-unknown-linux-gnu;
          rootPkgs = crossPkgs;
        };
      };

      makeContainerImage = name: { arch, package, rootPkgs }:
        let
          rootEnv = rootPkgs.buildEnv {
            name = "root";
            paths = [ package rootPkgs.bashInteractive rootPkgs.coreutils ];
            pathsToLink = [ "/bin" ];
          };
        in
        nix2container.buildImage {
          name = "pagers";
          tag = "latest-${arch}";
          inherit arch;

          copyToRoot = [ rootEnv ];

          layers = [
            (nix2container.buildLayer {
              deps = [ rootPkgs.bashInteractive rootPkgs.coreutils ];
              maxLayers = 80;
            })
          ];

          config = {
            entrypoint = [ "/bin/pagers" ];
            Cmd = [ "--help" ];
          };

          maxLayers = 10;
        };

      containers = lib.mapAttrs'
        (name: target:
          lib.nameValuePair "container-${name}" (makeContainerImage name target)
        )
        containerTargets;
    in
    {
      packages = containers;
    };
}
