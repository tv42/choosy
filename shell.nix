let
  # avoid pkgs.fetchFromGitHub because with that we'd need to import nixpkgs to construct nixpkgs, and that ends up putting nix into a recursion and aborting. this also means this .nix file won't take an optional `pkgs` argument like most of them do.
  rustOverlay = (import (builtins.fetchTarball {
    url = "https://github.com/oxalica/rust-overlay/archive/4fb3803f4c4a846c4e1266e4a792e919e17e4d46.tar.gz";
    sha256 = "11bhgkivvsj1b0hx5bwcliwxpipvvdl85k4mkybvx4yfpsfy9fhj";
  }));
  pkgs = import <nixpkgs> {
    overlays = [ rustOverlay ];
  };
  rustc = pkgs.rust-bin.stable."1.53.0".minimal.override {
    extensions = [ "rust-src" "rust-analysis" ];
    targets = [ "wasm32-unknown-unknown" "x86_64-unknown-linux-musl" ];
  };
  rustPlatform = pkgs.makeRustPlatform {
    cargo = rustc;
    rustc = rustc;
  };
in
with pkgs;
stdenv.mkDerivation {
  name = "nix-shell-rust";
  buildInputs = [
    rustc
    cargo
    rustfmt
    rustPackages.clippy
    cargo-edit
    cargo-watch
    ws

    # mpv v0.33 needed for --input-ipc-client
    mpv
  ];
}
