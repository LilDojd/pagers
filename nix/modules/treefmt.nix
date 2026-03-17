{ ... }:
{
  perSystem = { ... }: {
    treefmt.programs = {
      nixpkgs-fmt.enable = true;
      rustfmt.enable = true;
      taplo.enable = true;
    };
  };
}
