# How the DuckDB dependency becomes a derivation, and the three variables that find it.
#
# Imported by both flake.nix and devenv.nix so there is ONE code path from nixpkgs to a
# libduckdb - the same reason nix/toolchains.nix exists. They each have their own nixpkgs pin
# (flake.lock and devenv.lock), so resolving `pkgs.duckdb` independently in each file is two
# DuckDBs that agree until the day one lock moves. A test that passes in the dev shell and fails
# in CI because the two linked different libraries is a bad afternoon, and it is the kind that
# looks like a flaky test.
#
# WHY NOT THE CRATE'S `bundled` FEATURE, which would need none of this. That feature compiles
# DuckDB's C++ from source inside the build. In the Nix sandbox that is minutes and gigabytes,
# paid again per derivation - the clippy check, the nextest check, the doctest check and the
# release build are four - and none of them would share the result, because a build-script
# artifact is not content-addressed the way a store path is. nixpkgs already built this once.
#
# THREE VARIABLES, NOT ONE, and the third is the one that is easy to miss:
#
#   DUCKDB_LIB_DIR       read by libduckdb-sys at BUILD time, to link against
#   DUCKDB_INCLUDE_DIR   read by libduckdb-sys at BUILD time, for duckdb.h
#   LD_LIBRARY_PATH      read by ld.so at RUN time
#
# Without the third the link succeeds and every binary that touches the adapter dies on startup
# with "libduckdb.so: cannot open shared object file". `.cargo/config.toml` selects clang and lld
# directly, so the rpath a Nix compiler wrapper would otherwise bake in is not there to fall back
# on.
#
# The library and the headers are in DIFFERENT OUTPUTS: `duckdb.lib` holds libduckdb.so and
# `duckdb.dev` holds the header. Pointing both at the default output is the first thing to get
# wrong and it fails as a missing header, which reads like a missing package.
{ pkgs }:
let
  duckdb = pkgs.duckdb;
in
{
  # For `packages` in the dev shell and `buildInputs` in the flake.
  package = duckdb;

  # For `env` in the dev shell and the derivation environment in the flake. One attrset, so a
  # variable added here reaches both without being transcribed.
  env = {
    DUCKDB_LIB_DIR = "${duckdb.lib}/lib";
    DUCKDB_INCLUDE_DIR = "${duckdb.dev}/include";
    LD_LIBRARY_PATH = "${duckdb.lib}/lib";
  };
}
