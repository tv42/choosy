let
  # avoid pkgs.fetchFromGitHub because with that we'd need to import nixpkgs to construct nixpkgs, and that ends up putting nix into a recursion and aborting. this also means this .nix file won't take an optional `pkgs` argument like most of them do.
  rustOverlay = (import (builtins.fetchTarball {
    url = "https://github.com/oxalica/rust-overlay/archive/e88036b9fc7b6ad4e2db86944204877b9090d8b9.tar.gz";
    sha256 = "1m29m49f7q0r6qvzjxkyq3yqiqff6b4cwl385cbpz551421bmr63";
  }));
  pkgs = import <nixpkgs> {
    overlays = [ rustOverlay ];
  };
  rustc = pkgs.rust-bin.stable."1.52.1".minimal.override {
    extensions = [ "rust-src" "rust-analysis" ];
    targets = [ "wasm32-unknown-unknown" "x86_64-unknown-linux-musl" ];
  };
  rustPlatform = pkgs.makeRustPlatform {
    cargo = rustc;
    rustc = rustc;
  };

  # mpv v0.33 needed for --input-ipc-client, which is not in
  # nixos-20.09. revisit to use nixos-21.05 when possible.
  unstable = import (builtins.fetchTarball {
      url = "https://github.com/nixos/nixpkgs/archive/b3ea6461fa5e265f6ccc11561fa26a6af3c9d5a7.tar.gz";
      sha256 = "14zxxwpflyy86y24li6m9s74as3xc58l94jh6hfj41kl4smb5g2b";
    })
    {
      # reuse the current configuration
      config = pkgs.config;
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
    systemfd
    cargo-watch
    ws

    unstable.mpv
  ];
}
