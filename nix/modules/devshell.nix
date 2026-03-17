{ inputs, ... }:
{
  perSystem = { config, self', pkgs, lib, ... }:
    {
      devShells.default = pkgs.mkShell {
        name = "pagers-shell";
        inputsFrom = [
          self'.devShells.rust
          config.pre-commit.devShell # See ./nix/modules/pre-commit.nix
        ];
        buildInputs = [ pkgs.openssl ];
        nativeBuildInputs = [ pkgs.pkg-config ];
        packages = with pkgs; [
          just
          nixd # Nix language server
          bacon
          cargo-edit
          cargo-nextest
          cargo-machete
          cargo-autoinherit
        ];
      };
    };
}
