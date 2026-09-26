# The ADBC PostgreSQL driver, built per release triple (telekom/sutura#913) - the four-triple
# matrix of `nix/bigquery-adbc-drivers.nix`, built by `nix/postgres-adbc.nix`. It only builds:
# nothing here loads or runs a driver, and no Rust code reads these packages yet.
{ pkgs, src }:
let
  driver = { triple, crossPkgs }: {
    "adbc-driver-postgresql-${triple}" = import ./postgres-adbc.nix {
      pkgs = crossPkgs;
      inherit src;
      crossSystemName = triple;
    };
  };
in
driver { triple = "x86_64-unknown-linux-gnu"; crossPkgs = pkgs.pkgsCross.gnu64; }
// driver { triple = "aarch64-unknown-linux-gnu"; crossPkgs = pkgs.pkgsCross.aarch64-multiplatform; }
// driver { triple = "aarch64-unknown-linux-musl"; crossPkgs = pkgs.pkgsCross.aarch64-multiplatform-musl; }
// driver { triple = "x86_64-unknown-linux-musl"; crossPkgs = pkgs.pkgsCross.musl64; }
