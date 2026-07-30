# Example NixOS configuration fragment for Metis.
#
# In your system flake:
#
#   inputs.metis.url = "github:digitalexpl0it/Metis";
#   …
#   modules = [
#     metis.nixosModules.metis
#     {
#       programs.metis.enable = true;
#       programs.metis.package = metis.packages.${system}.metis-desktop;
#     }
#   ];
#
# Then rebuild, log out, and pick **Metis** at the greeter.
#
# First-time package build needs a valid smithay cargoLock.outputHashes entry in
# nix/metis-desktop.nix (run `nix build .#metis-desktop` and paste the `got:` hash).
