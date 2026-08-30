# The Postgres tier as ONE provisioner in two places: nixpkgs' `postgresql_18`, started from the
# same script by the `checks.nextest` sandbox and by `xtask dev-up` in the dev shell.
#
# The nix build sandbox has no network and no docker socket, so a docker tier cannot be a check.
# Postgres needs neither: it runs over a unix socket beneath the worktree, which has no port, so no
# allocator, no collision, no race - the port machinery `docs/adr/0009` spends itself on is for
# docker services only. One package and one start script in both places means the two cannot drift
# (the SQL_ASCII slip in this PR's first go at a second provisioner is what one script prevents),
# and `just update` moves both.
#
# `postgresql_18` pins the same minor the docker image once used, and like `nix/duckdb.nix` it is
# the single path from nixpkgs to the server used by flake.nix AND devenv.nix.
{ pkgs }:
{
  package = pkgs.postgresql_18;

  # The provisioner, usable from any shell that has it and `postgresql`'s binaries on PATH; the
  # dev shell gets this on PATH through `devenv.nix`, the sandbox gets it as a native input.
  #
  # `start` brings up (or is a no-op restart of) a socket-only server under
  # `<cwd>/.sutura-dev/pg/` and writes `<cwd>/.sutura-dev/endpoints.json` naming its socket
  # directory, so `sutura_dev::provisioned::here` can read it unchanged. `stop` tears it back down.
  tier = pkgs.writeShellApplication {
    name = "sutura-postgres-tier";
    runtimeInputs = [ pkgs.postgresql_18 ];
    text = ''
      set -o errexit -o nounset

      root="$(pwd -P)"
      # In the sandbox `$NIX_BUILD_TOP` is short; the build-tree source path under it would make a
      # unix-socket path longer than macOS allows (104 chars). In the dev shell it falls back to the
      # worktree, which is both short enough for a socket and where the data may live.
      base="''${NIX_BUILD_TOP:-$root}"
      pg="$base/.sutura-dev/pg"
      port=5432
      # Two single quotes at RUNTIME, so the nix indented string never holds two adjacent apostrophes
      # (nix would strip them); `listen_addresses` empty means no TCP at all.
      empty=

      start() {
        mkdir -p "$base/.sutura-dev" "$pg" "$root/.sutura-dev"
        if [ ! -f "$pg/PG_VERSION" ]; then
          initdb -D "$pg" -U postgres -E UTF8 --locale=C
        fi
        # Socket-only, under the build/worktree. No TCP, so no port allocation or collision.
        cat > "$pg/postgresql.conf" <<EOC
      listen_addresses = '$empty'
      unix_socket_directories = '$pg'
      port = $port
      fsync = off
      synchronous_commit = off
      EOC
        pg_ctl -D "$pg" -o "-p $port" -l "$pg/server.log" start
        # Role and database, idempotently (the superuser here is postgres).
        psql -h "$pg" -p "$port" -U postgres -d postgres \
          -v ON_ERROR_STOP=1 -c "CREATE ROLE sutura LOGIN PASSWORD 'sutura'"
        psql -h "$pg" -p "$port" -U postgres -d postgres \
          -v ON_ERROR_STOP=1 \
          -c "CREATE DATABASE sutura OWNER sutura TEMPLATE template0 LOCALE 'C' ENCODING 'UTF8'"
        # The harness reads `<root>/.sutura-dev/endpoints.json` and treats the host as the socket dir.
        printf '{"project":"sutura","provisioner":"nix","services":{"postgres":{"host":"%s","port":%s}}}\n' \
          "$pg" "$port" > "$root/.sutura-dev/endpoints.json"
      }

      stop() {
        if [ -d "$pg" ]; then
          pg_ctl -D "$pg" stop -m fast || true
        fi
      }

      case "''${1:-}" in
        start) start ;;
        stop) stop ;;
        *) echo "usage: $0 start|stop" >&2; exit 2 ;;
      esac
    '';
  };
}
