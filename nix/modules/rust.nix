{ inputs, self, ... }:
{
  imports = [
    inputs.rust-flake.flakeModules.default
    inputs.rust-flake.flakeModules.nixpkgs
  ];
  perSystem = { self', config, lib, pkgs, ... }: {
    rust-project.src = lib.cleanSourceWith {
      src = self; # The original, unfiltered source
      filter = path: type:
        (config.rust-project.crane-lib.filterCargoSources path type);
    };
    rust-project.crates."pagers".crane.args = {
      buildInputs = [ pkgs.openssl ];
      nativeBuildInputs = [ pkgs.pkg-config ];
    };
    packages.default = self'.packages.pagers;
  };
}
