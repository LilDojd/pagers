{ ... }:
{
  perSystem = { self', pkgs, lib, ... }: {
    apps.check-closure-size = {
      type = "app";
      meta.description = "Check that the closure size stays within limits";
      program = lib.getExe (pkgs.writeShellApplication {
        name = "check-closure-size";
        runtimeInputs = [ pkgs.nix ];
        text = ''
          MAX_SIZE_MB=''${1:-100}
          CLOSURE=$(nix path-info -S --json ${self'.packages.default})
          SIZE=$(echo "$CLOSURE" | ${lib.getExe pkgs.jq} '.[].closureSize')
          SIZE_MB=$((SIZE / 1024 / 1024))
          echo "Closure size: ''${SIZE_MB}MB (max: ''${MAX_SIZE_MB}MB)"
          if [ "$SIZE_MB" -gt "$MAX_SIZE_MB" ]; then
            echo "ERROR: Closure size exceeds ''${MAX_SIZE_MB}MB"
            exit 1
          fi
        '';
      });
    };
  };
}
