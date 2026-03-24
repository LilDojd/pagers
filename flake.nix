{
  inputs = {
    nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.1.*";
    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
    systems.url = "github:nix-systems/default";
    crane.url = "https://flakehub.com/f/ipetkov/crane/0.23.*";
    fenix = {
      url = "https://flakehub.com/f/nix-community/fenix/0.1.*";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    git-hooks = {
      url = "https://flakehub.com/f/cachix/git-hooks.nix/0.1.*";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    treefmt-nix = {
      url = "https://flakehub.com/f/numtide/treefmt-nix/0.1.*";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    omnix = {
      url = "github:juspay/omnix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      systems = import inputs.systems;

      # See ./nix/modules/*.nix for the modules that are imported here.
      imports =
        with builtins;
        [ inputs.treefmt-nix.flakeModule ]
        ++ map (fn: ./nix/modules/${fn}) (attrNames (readDir ./nix/modules));
    };
}
