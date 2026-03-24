{ inputs, ... }:
{
  perSystem = { self', pkgs, system, lib, ... }:
    let
      nix2container = inputs.nix2container.packages.${system}.nix2container;

      containerTargets = lib.optionalAttrs (system == "x86_64-linux") {
        amd64 = {
          arch = "amd64";
          package = self'.packages.pagers;
        };
        arm64 = {
          arch = "arm64";
          package = self'.packages.pagers-aarch64-unknown-linux-gnu;
        };
      };

      makeContainerImage = name: { arch, package }:
        nix2container.buildImage {
          name = "pagers";
          tag = "latest-${arch}";
          inherit arch;

          copyToRoot = [ package ];
          config = {
            entrypoint = [ "${package}/bin/pagers" ];
          };

          maxLayers = 50;
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
