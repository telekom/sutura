# The ADBC DuckDB driver archive, per release triple - the four-triple matrix of
# `nix/postgres-adbc-drivers.nix`, built by `nix/duckdb-adbc.nix`. `nix/shipped.nix` links the two
# musl archives into every musl build of `sutura-cli`, and its `linkedDriversTests` runs the x86_64
# one.
{ pkgs }:
let
  driver = { triple, crossPkgs }: {
    "adbc-driver-duckdb-${triple}" = import ./duckdb-adbc.nix {
      pkgs = crossPkgs;
      crossSystemName = triple;
    };
  };
in
driver { triple = "x86_64-unknown-linux-gnu"; crossPkgs = pkgs.pkgsCross.gnu64; }
// driver { triple = "aarch64-unknown-linux-gnu"; crossPkgs = pkgs.pkgsCross.aarch64-multiplatform; }
// driver { triple = "aarch64-unknown-linux-musl"; crossPkgs = pkgs.pkgsCross.aarch64-multiplatform-musl; }
// driver { triple = "x86_64-unknown-linux-musl"; crossPkgs = pkgs.pkgsCross.musl64; }
