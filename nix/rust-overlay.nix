let
  # Avoid pkgs.fetchFromGitHub because with that we'd need to import nixpkgs to construct nixpkgs, and that ends up putting nix into a recursion and aborting. This also means this .nix file won't take an optional `pkgs` argument like most of them do.
  rustOverlay = (import (builtins.fetchTarball {
    url = "https://github.com/oxalica/rust-overlay/archive/abf8758f9fc2130adb7fd4ac1e8d5daf3a28cf13.tar.gz";
    sha256 = "1zqgv6hrhf882ncfrhdbzxq2p3hn371i934zirgk070lm0ngjfaf";
  }));
  rustVersionOverlay = (self: super:
    let
      rustChannel = super.rust-bin.fromRustupToolchainFile ./../rust-toolchain.toml;
    in
    {
      rustc = rustChannel;
      # Naersk doesn't work with stable cargo, workaround by using nightly cargo.
      # https://github.com/nix-community/naersk/issues/100
      # cargo = rustChannel;
      cargo = super.rust-bin.nightly."2021-08-17".cargo;
    }
  );
in
# See nixpkgs.lib.fixedPoints.composeExtensions for inspiration.
  # Not using it because, as above, we'd need to import nixpkgs to define nixpkgs.
self: super:
let
  rustOverlayResult = rustOverlay self super;
  super2 = super // rustOverlayResult;
  versionOverlayResult = rustVersionOverlay self super2;
in
# Now combine the two attribute sets.
rustOverlayResult // versionOverlayResult
