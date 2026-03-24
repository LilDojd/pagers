{ ... }:
{
  perSystem = { ... }: {
    treefmt = {
      programs = {
        nixpkgs-fmt.enable = true;
        rustfmt.enable = true;
        taplo.enable = true;
        mdformat.enable = true;
      };
      settings.global.excludes = [
        "LICENSE-*"
        "CHANGELOG.md"
      ];
    };
  };
}
