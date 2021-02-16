let
  moz_overlay = import (builtins.fetchTarball {
    url = https://github.com/mozilla/nixpkgs-mozilla/archive/8c007b60731c07dd7a052cce508de3bb1ae849b4.tar.gz;
    sha256 = "1zybp62zz0h077zm2zmqs2wcg3whg6jqaah9hcl1gv4x8af4zhs6";
  });
  pkgs = import <nixpkgs> {
    overlays = [ moz_overlay ];
  };
  rustChannel = pkgs.rustChannelOf { date = "2021-02-14"; channel = "nightly"; };
  rustcWASM = rustChannel.rust.override {
    targets = [ "wasm32-unknown-unknown" ];
    extensions = [ "rust-src" "rust-analysis" ];
  };

  catflap = with pkgs; rustPlatform.buildRustPackage rec {
    pname = "catflap";
    version = "1.2.0";

    src = fetchFromGitHub {
      owner = "passcod";
      repo = pname;
      rev = "v${version}";
      sha256 = "077fy2disrixi4s1223flkvlh43wjm51mzv38zls1l0y2ykq0z1y";
    };
    cargoSha256 = "1qh74faajjzap2csbn39wq3bpa7yma2073xz19qmlp0l32vvjr1g";
  };

in
with pkgs;
stdenv.mkDerivation {
  name = "rust";
  buildInputs = [
    rustcWASM
    cargo
    rustfmt
    rustPackages.clippy
    cargo-edit
    catflap
    cargo-watch
    ws
  ];
}
