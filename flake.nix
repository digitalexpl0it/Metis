{
  description = "Metis Wayland desktop environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          metisDesktop = pkgs.callPackage ./nix/metis-desktop.nix {
            inherit (pkgs) rustPlatform;
            src = self;
          };
        in
        {
          default = metisDesktop;
          metis-desktop = metisDesktop;
        });

      nixosModules.default = import ./nix/module.nix;
      nixosModules.metis = self.nixosModules.default;

      checks = forAllSystems (system: {
        metis-desktop = self.packages.${system}.metis-desktop;
      });

      # Convenience: nix develop
      devShells = forAllSystems (system:
        let pkgs = import nixpkgs { inherit system; }; in {
          default = pkgs.mkShell {
            inputsFrom = [ self.packages.${system}.metis-desktop ];
            packages = with pkgs; [ rustc cargo rustfmt clippy pkg-config ];
          };
        });
    };
}
