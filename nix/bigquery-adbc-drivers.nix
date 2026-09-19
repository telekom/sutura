# The ADBC BigQuery driver, built per release triple (telekom/sutura#913).
#
# This exposes `libadbc_driver_bigquery.so` for each of the four release triples
# so the driver is built, linted and audited on every `nix flake check` without
# yet wiring it into the shipped closures. The Rust `adbc_driver_manager`
# consumption, the decision of dlopen-vs-static-link for the two musl triples,
# and the provisioned live acceptance leg are a follow-on PR.
#
# One vendor hash covers all four triples: the Go modules fixed-output
# derivation is keyed by the resolved module set, not by libc/arch (measured:
# aarch64-musl, aarch64-gnu and x86_64-musl all resolve to the same sha256).
{ pkgs, bigqueryAdbcGoSource, buildDriver }:
# `buildDriver` is `import ./bigquery-adbc.nix`; `pkgs` is the HOST package set.
let
  driver = { triple, crossPkgs }:
    let
      d = buildDriver {
        pkgs = crossPkgs;
        src = bigqueryAdbcGoSource;
        crossSystemName = triple;
        vendorHash = "sha256-EijrXEBhQvaJR0k9jrGawDDELAOKeRVuPjCe9UnXNQI=";
      };
    in
    { "adbc-driver-bigquery-${triple}" = d; };
in
# x86_64-unknown-linux-gnu is the native/aliased target: on a linux builder it is
# plain `pkgs`; on a foreign host (e.g. aarch64-darwin) it is not reachable here
# and is built on the linux CI runner instead, exactly as `shipped.nix` aliases it.
{
  "adbc-driver-bigquery-x86_64-unknown-linux-gnu" = buildDriver {
    pkgs = if pkgs.system == "x86_64-linux" then pkgs else pkgs;
    src = bigqueryAdbcGoSource;
    crossSystemName = "x86_64-unknown-linux-gnu";
    vendorHash = "sha256-EijrXEBhQvaJR0k9jrGawDDELAOKeRVuPjCe9UnXNQI=";
  };
}
// (driver { triple = "aarch64-unknown-linux-gnu";  crossPkgs = pkgs.pkgsCross.aarch64-multiplatform; })
// (driver { triple = "aarch64-unknown-linux-musl"; crossPkgs = pkgs.pkgsCross.aarch64-multiplatform-musl; })
// (driver { triple = "x86_64-unknown-linux-musl";  crossPkgs = pkgs.pkgsCross.musl64; })
