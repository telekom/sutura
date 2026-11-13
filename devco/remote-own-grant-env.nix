{ source }:
let
  flake = builtins.getFlake (toString source);
  pkgs = import flake.inputs.nixpkgs {
    system = "x86_64-linux";
    overlays = [ (import flake.inputs.rust-overlay) ];
  };
in
pkgs.mkShell {
  packages = [
    pkgs.python3
    pkgs.git
    pkgs.clang
    pkgs.lld
    pkgs.cmake
    pkgs.pkg-config
    pkgs.coreutils
  ];
  buildInputs = [ pkgs.openssl ];
  REMOTE_GRANT_TOOLCHAIN = pkgs.rust-bin.fromRustupToolchainFile (
    source + "/devco/rust-toolchain-nightly.toml"
  );
}
