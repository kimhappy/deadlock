{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    rust-manifest = {
      url = "https://static.rust-lang.org/dist/channel-rust-stable.toml";
      flake = false;
    };
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    fenix,
    rust-manifest,
    ...
  }: flake-utils.lib.eachDefaultSystem (system: let
    pkgs = nixpkgs.legacyPackages.${system};
    rust = fenix.packages.${system}.fromManifestFile rust-manifest;
  in {
    devShells.default = pkgs.mkShell {
      packages = [
        pkgs.llvmPackages.llvm
        (rust.withComponents [
          "cargo"
          "rustc"
          "rust-src"
          "rustfmt"
          "rust-analyzer"
          "clippy"
        ])
      ];
    };
  });
}
