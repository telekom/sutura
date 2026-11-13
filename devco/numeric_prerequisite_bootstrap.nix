{ source }:
let
  src = builtins.toPath source;
  flake = builtins.getFlake ("git+file://" + source);
  system = "x86_64-linux";
  base = import flake.inputs.nixpkgs {
    inherit system;
    overlays = [ (import flake.inputs.rust-overlay) ];
  };
  pkgs = base // import (src + "/nix/dev-tools.nix") { pkgs = base; };
  toolchain = (import (src + "/nix/toolchains.nix") { rustPkgs = pkgs; }).nightly;
in
pkgs.mkShell {
  packages = [
    toolchain
    pkgs.clang
    pkgs.lld
    pkgs.just
    pkgs.python3
    flake.packages.${system}.jscpd
  ];
}
