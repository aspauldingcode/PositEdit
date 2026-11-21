{
  description = "Universal Project/Runtime Workbench DLC (additive, standalone)";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      eachSystem = f: builtins.listToAttrs (map (system: {
        name = system;
        value = f system;
      }) systems);
    in {
      packages = eachSystem (system:
        let pkgs = import nixpkgs { inherit system; }; in {
          default = pkgs.python3Packages.buildPythonApplication {
            pname = "universal-ide-dlc";
            version = "0.1.0";
            pyproject = true;
            src = ./.;
            build-system = [ pkgs.python3Packages.setuptools ];
          };
        });
      apps = eachSystem (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/universal-ide-dlc";
        };
      });
      devShells = eachSystem (system:
        let pkgs = import nixpkgs { inherit system; }; in {
          default = pkgs.mkShell { packages = [ pkgs.python3 pkgs.cmake pkgs.git ]; };
        });
    };
}
