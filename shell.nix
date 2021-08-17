let
  rustOverlay = import ./nix/rust-overlay.nix;
  pkgs = import <nixpkgs> {
    overlays = [
      rustOverlay
    ];
  };
in
with pkgs;
stdenv.mkDerivation {
  name = "choosy-shell";
  buildInputs = [
    rustc
    cargo
    rustfmt
    cargo-edit
    cargo-watch
    ws

    # mpv v0.33 needed for --input-ipc-client
    mpv
  ];
}
