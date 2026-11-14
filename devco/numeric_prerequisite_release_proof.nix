{ base }:
let
  src = builtins.toPath base;
  flake = builtins.getFlake ("git+file://" + base + "?rev=b8bce349b79fc16891f8bfd466ba7187cafdf019");
  system = "x86_64-linux";
  frozen = import flake.inputs.nixpkgs {
    inherit system;
    overlays = [ (import flake.inputs.rust-overlay) ];
  };
  pkgs = frozen // import (src + "/nix/dev-tools.nix") { pkgs = frozen; };
  toolchain = (import (src + "/nix/toolchains.nix") { rustPkgs = pkgs; }).nightly;
in
pkgs.mkShell {
  packages = [
    toolchain
    pkgs.clang
    pkgs.lld
    pkgs.just
    pkgs.git
    pkgs.python3
    flake.packages.${system}.jscpd
  ];
}
