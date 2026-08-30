# The Postgres tier as ONE provisioner in two places: nixpkgs' `postgresql_18`, started from the
# same script by the `checks.nextest` sandbox and by `just test` in the dev shell.
#
# The nix build sandbox has no network and no docker socket, so a docker tier cannot be a check.
# Postgres needs neither: it runs over a unix socket, which has no port, so no allocator, no
# collision, no race - the port machinery `docs/adr/0009` spends itself on is for docker services
# only. One package and one start script in both places means the two cannot drift (the SQL_ASCII
# slip in this PR's first go at a second provisioner is what one script prevents), and `just update`
# moves both.
#
# `postgresql_18` pins the same minor the docker image once used, and like `nix/duckdb.nix` it is
# the single path from nixpkgs to the server used by flake.nix AND devenv.nix.
{ pkgs }:
{
  package = pkgs.postgresql_18;

  # The provisioner, usable from any shell that has it and `postgresql`'s binaries on PATH; the
  # dev shell gets this on PATH through `devenv.nix`, the sandbox gets it as a native input.
  #
  # `start` brings up (or is a no-op restart of) a socket-only server and writes
  # `<cwd>/.sutura-dev/endpoints.json` naming its socket directory, so `sutura_dev::provisioned::here`
  # can read it unchanged. `stop` tears it back down.
  #
  # WHERE the server lives is the one thing that differs between the two callers, and both choose
  # a SHORT path: a unix socket path is capped around 100 bytes on macOS, so the server can never
  # sit under an arbitrarily deep worktree. The sandbox uses `$NIX_BUILD_TOP` (short by fiat); the
  # dev shell uses a short per-worktree directory under `$TMPDIR`, keyed by a hash of the worktree
  # so two worktrees cannot clobber each other's server. The endpoint FILE still lands in the
  # worktree, which is where the harness looks.
  tier = pkgs.writeShellApplication {
    name = "sutura-postgres-tier";
    runtimeInputs = [ pkgs.postgresql_18 ];
    text = ''
      set -o errexit -o nounset

      root="$(pwd -P)"
      if [ -n "''${NIX_BUILD_TOP:-}" ]; then
        # In the sandbox the build-tree source path is deep, but `$NIX_BUILD_TOP` itself is short.
        pg="$NIX_BUILD_TOP/.sutura-dev/pg"
      else
        # A worktree can be far deeper than a socket allows, so the server lives in a short
        # per-worktree directory under TMPDIR, keyed by a hash of the worktree's physical path.
        key="$(printf '%s' "$root" | cksum | cut -d' ' -f1)"
        pg="''${TMPDIR:-/tmp}/sutura-pg-$key"
      fi
      port=5432
      # Two single quotes at RUNTIME, so the nix indented string never holds two adjacent apostrophes
      # (nix would strip them); `listen_addresses` empty means no TCP at all.
      empty=

      start() {
        mkdir -p "$pg" "$root/.sutura-dev"
        if [ ! -f "$pg/PG_VERSION" ]; then
          initdb -D "$pg" -U postgres -E UTF8 --locale=C
        fi
        # Socket-only, under the short directory. No TCP, so no port allocation or collision.
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
