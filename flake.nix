{
  description = "bpm-rs - A binary package manager based on GitHub Releases";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
            "clippy"
          ];
        };
      in
      {
        packages = {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "bpm";
            version = "0.2.0";
            src = ./.;

            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = with pkgs; [
              libclang
              pkg-config
            ];

            buildInputs = with pkgs; [
            ];

            LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";

            # skip tests that require network
            checkPhase = ''
              cargo test -- --skip test_requires_network
            '';

            meta = {
              description = "A binary package manager based on GitHub Releases";
              homepage = "https://github.com/lxl66566/bpm-rs";
              license = pkgs.lib.licenses.mit;
              mainProgram = "bpm";
            };
          };
        };

        devShells = {
          default = pkgs.mkShell {
            nativeBuildInputs = with pkgs; [
              libclang
              pkg-config
            ];

            buildInputs = with pkgs; [
              rustToolchain
            ];

            LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
          };
        };
      }
    );
}