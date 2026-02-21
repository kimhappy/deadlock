{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    fenix,
    ...
  }: flake-utils.lib.eachDefaultSystem (system: let
    pkgs = import nixpkgs {
      inherit system;

      overlays = [
        fenix.overlays.default
      ];
    };
  in {
    devShells = {
      default = pkgs.mkShell {
        packages = [
          pkgs.llvmPackages.llvm
          pkgs.fenix.stable.toolchain
        ];
      };

      nightly = pkgs.mkShell {
        packages = [
          pkgs.llvmPackages.llvm
          pkgs.fenix.complete.toolchain
        ];
      };
    };
  });
}
