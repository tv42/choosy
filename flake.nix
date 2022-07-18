{
  inputs = {
    nixpkgs.url = github:nixos/nixpkgs/nixos-22.05;

    flake-utils.url = github:numtide/flake-utils;
    rust-overlay = {
      url = github:oxalica/rust-overlay;
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.flake-utils.follows = "flake-utils";
    };
    naersk = {
      url = github:nix-community/naersk;
      inputs.nixpkgs.follows = "nixpkgs";
    };
    gitignore = {
      url = github:hercules-ci/gitignore;
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, naersk, ... }@inputs:
    flake-utils.lib.eachDefaultSystem (system:
      let
        rustVersionOverlay = (self: prev:
          let
            rustChannel = prev.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          in
          {
            rustc = rustChannel;
            cargo = rustChannel;
          }
        );
        pkgs = import nixpkgs {
          inherit system;
          overlays = [
            inputs.rust-overlay.overlays.default
            rustVersionOverlay
          ];
        };
        naersk-lib = naersk.lib.${system}.override {
          rustc = pkgs.rustc;
          cargo = pkgs.cargo;
        };
        gitignoreSource = inputs.gitignore.outputs.lib.gitignoreSource;
      in
      rec {
        # Use naersk as a rust builder.
        # This avoids
        #
        # 1) nixpkgs.rustPlatform.buildRustPackage demanding a cargoSha256 that is updated every time someone dares to change anything.
        # 2) nixpkgs.rustPlatform.buildRustPackage with `cargoVendorDir` demanding a vendored copy of all dependencies to be stored in the repo.
        # 3) crate2nix (https://crates.io/crates/crate2nix) demanding a Cargo.nix file that is updated every time someone dares to change anything.
        #
        # Cargo.lock is just as trustworthy as a source as cargoSha256.
        #
        # https://github.com/NixOS/nixpkgs/issues/63653
        packages.choosy = naersk-lib.buildPackage {
          pname = "choosy";
          # Avoid ingesting all of `target/` into the Nix store.
          src = gitignoreSource ./.;
          buildInputs = with pkgs; [
            perl
          ];
          meta = {
            description = "Choose a file and play it with mpv";
            homepage = "https://github.com/tv42/choosy";
            license = with pkgs.lib.licenses; [ asl20 mit ]; # either at your option
          };
        };
        defaultPackage = packages.choosy;

        # `nix run`
        apps.choosy = flake-utils.lib.mkApp {
          drv = packages.choosy;
        };
        defaultApp = apps.choosy;

        # `nix develop`
        devShell = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            rustc
            cargo
            cargo-edit
            cargo-audit
          ];
        };
      });
}
