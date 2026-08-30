# The Postgres tier as a test-input, so the corpus and differential cells that need a real server
# run inside the `checks.nextest` sandbox instead of a second, separately-compiling CI job.
#
# The nix build sandbox has no network and no docker socket (`dev/src/requirement.rs`), so the
# server has to be bundled and reached over a unix socket. nixpkgs' `postgresqlTestHook` already
# does the lifecycle: `initdb -U postgres` + `pg_ctl` under `$NIX_BUILD_TOP`, `listen_addresses=''`
# (no TCP at all), exporting `PGHOST` as the socket directory. It is the standard "database up
# during checkPhase" mechanism and carries the Linux-sandbox evidence in its ~40 nixpkgs adopters,
# which is where this claim gets its binding rather than from us.
#
# The hook's `meta.badPlatforms` names darwin via NixOS/nix#12548 (SysV IPC not cleaned up in the
# darwin sandbox), which is closed by NixOS/nix#14459; without the override `just validate` stops
# *evaluating* on a Mac. Measured green sandboxed on aarch64-darwin, 2026-08-30; re-check when
# either issue moves.
#
# This is the second provisioner for `.sutura-dev/endpoints.json`, the first being `xtask dev-up`
# over docker. The file is the contract: `sutura_dev::provisioned::here` reads it and nothing
# distinguishes who wrote it, and `SUTURA_DEV_REQUIRE_TIER` makes an absent tier in this derivation
# a red check rather than a loud skip.
{ pkgs }:
{
  package = pkgs.postgresql_18;
  hook = pkgs.postgresqlTestHook.overrideAttrs (old: {
    meta = old.meta // { badPlatforms = [ ]; };
  });

  # Hook inputs on the derivation, so the hook's own `checkPhase` wrapper reads them. `LOCALE 'C'`
  # pins the collation to bytewise, matching DuckDB/DataFusion so the differential does not depend
  # on a libc, and matching what the compose tier must declare for the same reason.
  env = {
    PGUSER = "sutura";
    PGDATABASE = "sutura";
    postgresqlTestSetupSQL = ''
      CREATE ROLE "sutura" LOGIN PASSWORD 'sutura';
      CREATE DATABASE "sutura" OWNER 'sutura' TEMPLATE template0 LOCALE 'C' ENCODING 'UTF8';
    '';
    postgresqlTestSetupPost = ''
      mkdir -p .sutura-dev
      printf '{"project":"nix-sandbox","services":{"postgres":{"host":"%s","port":5432}}}\n' "$PGHOST" \
        > .sutura-dev/endpoints.json
    '';
    postgresqlExtraSettings = "fsync = off\nsynchronous_commit = off";
    SUTURA_DEV_REQUIRE_TIER = "1";
  };
}
