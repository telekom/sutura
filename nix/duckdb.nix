# The DuckDB a host mounts as its ADBC driver, and the one variable that names it.
#
# Imported by both flake.nix and devenv.nix so there is ONE code path from nixpkgs to a
# libduckdb - the same reason nix/toolchains.nix exists. They each have their own nixpkgs pin
# (flake.lock and devenv.lock), so resolving `pkgs.duckdb` independently in each file is two
# DuckDBs that agree until the day one lock moves.
#
# NOTHING LINKS IT. `sutura-exec-duckdb` opens the engine's own `duckdb_adbc_init` from this
# library through the ADBC driver manager (`sutura_adbc::mounted_duckdb_driver`), so it is a run-time
# path that no build step reads - no `DUCKDB_LIB_DIR`, no header, no `LD_LIBRARY_PATH`. A musl release
# links the same DuckDB statically instead (`nix/duckdb-adbc.nix`) and never reads the variable.
#
# The library is in the `lib` output, not the default one, which holds the CLI.
{ pkgs }:
let
  duckdb = pkgs.duckdb;
in
{
  # For `packages` in the dev shell: the CLI, for looking at a fixture by hand.
  package = duckdb;

  # For `env` in the dev shell and the derivation environment in the flake. One attrset, so a
  # variable added here reaches both without being transcribed.
  env = {
    SUTURA_DUCKDB_ADBC_DRIVER = "${duckdb.lib}/lib/libduckdb${pkgs.stdenv.hostPlatform.extensions.sharedLibrary}";
  };
}
