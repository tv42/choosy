{ pkgs ? import <nixpkgs> { }, pkgsPath ? pkgs.path }:

let
  rustOverlay = import ./nix/rust-overlay.nix;
  pkgs = import pkgsPath {
    overlays = [
      rustOverlay
    ];
  };

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
  naersk = pkgs.callPackage
    (pkgs.fetchFromGitHub {
      owner = "nmattia";
      repo = "naersk";
      rev = "e09c320446c5c2516d430803f7b19f5833781337";
      sha256 = "0k1pk2ixnxl6njjrgy750gm6m1nkkdsah383n3wp4ybrzacnav5h";
    })
    { };

  # Avoid ingesting all of `target/` into the Nix store.
  gitignoreSource = (pkgs.callPackage
    (pkgs.fetchFromGitHub {
      owner = "hercules-ci";
      repo = "gitignore.nix";
      rev = "211907489e9f198594c0eb0ca9256a1949c9d412";
      sha256 = "sha256:06j7wpvj54khw0z10fjyi31kpafkr6hi1k0di13k1xp8kywvfyx8";
    })
    { }).gitignoreSource;
in
naersk.buildPackage rec {
  name = "choosy";
  src = gitignoreSource ./.;
  buildInputs = with pkgs; [
    perl
  ];
  meta = {
    description = "Choose a file and play it with mpv";
    homepage = "https://github.com/tv42/choosy";
    license = with pkgs.lib.licenses; [ asl20 mit ]; # either at your option
  };
}
