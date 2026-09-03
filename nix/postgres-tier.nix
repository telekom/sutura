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
  # can read it unchanged. `stop` tears it back down AND REMOVES THAT FILE, and `status` answers
  # whether a server is up without changing anything.
  #
  # **The endpoint file is a CLAIM that a server is there, and `stop` used to leave it behind.** That
  # is not cosmetic: discovery reads the file's existence as availability, so after any `stop` the
  # next bare `cargo nextest` found the claim, tried to connect to a server that was gone, and the
  # two postgres cells PANICKED - `postgres did not open at <dir>:5432: could not connect` - where
  # the honest outcome is a skip. Measured on 2026-09-02: a `git commit` was blocked by exactly that,
  # on a tree whose `.sutura-dev/endpoints.json` named a socket directory that no longer existed.
  # Withdrawing the claim when the server goes is what makes "available" mean something.
  #
  # `status` exists so a caller can tear down only what it brought up. Every wrapper used to
  # `start` unconditionally and `stop` on EXIT, so a nested run - or a developer who started the
  # tier by hand - had their server stopped by somebody else's trap. See `nix/with-tier.sh`.
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

      # A unix socket path caps around 100 bytes on macOS. Refuse early with a message that names the
      # cause, rather than let pg_ctl fail with a bare "could not create any Unix-domain sockets" in
      # the log. `$TMPDIR` on a dev machine is well short of this; a custom long one is the case
      # this catches at the point it fails.
      if [ ''${#pg} -gt 80 ]; then
        echo "socket path too long (\$pg): a unix socket cannot be created here" >&2
        exit 1
      fi

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
        # Idempotent: if the server is already up - a repeated `just test`, or an interrupted run
        # whose trap did not fire - pg_ctl would abort on the existing postmaster.pid. Start only if
        # it is not already running, and create the role and database only if they are missing. The
        # sandbox never saw this because `$NIX_BUILD_TOP` is fresh every build.
        if ! pg_ctl -D "$pg" status >/dev/null 2>&1; then
          pg_ctl -D "$pg" -o "-p $port" -l "$pg/server.log" start
        fi
        if ! psql -h "$pg" -p "$port" -U postgres -d postgres -tAc \
          "SELECT 1 FROM pg_roles WHERE rolname='sutura'" | grep -q 1; then
          psql -h "$pg" -p "$port" -U postgres -d postgres \
            -v ON_ERROR_STOP=1 -c "CREATE ROLE sutura LOGIN PASSWORD 'sutura'"
        fi
        if ! psql -h "$pg" -p "$port" -U postgres -d postgres -tAc \
          "SELECT 1 FROM pg_database WHERE datname='sutura'" | grep -q 1; then
          psql -h "$pg" -p "$port" -U postgres -d postgres \
            -v ON_ERROR_STOP=1 \
            -c "CREATE DATABASE sutura OWNER sutura TEMPLATE template0 LOCALE 'C' ENCODING 'UTF8'"
        fi
        # The harness reads `<root>/.sutura-dev/endpoints.json` and treats the host as the socket dir.
        printf '{"project":"sutura","provisioner":"nix","services":{"postgres":{"host":"%s","port":%s}}}\n' \
          "$pg" "$port" > "$root/.sutura-dev/endpoints.json"
      }

      stop() {
        if [ -d "$pg" ]; then
          pg_ctl -D "$pg" stop -m fast || true
        fi
        # The endpoint file is a claim that a server is there. Withdraw it, or discovery keeps
        # believing it and the cells fail on a dead socket instead of skipping. `|| true` because a
        # tier that was never started has no file to remove and that is not a failure.
        rm -f "$root/.sutura-dev/endpoints.json" || true
      }

      # Is a server up? Nothing is changed, and the answer is the exit code - so a wrapper can stop
      # only what it started rather than trampling a tier somebody else brought up.
      status() {
        pg_ctl -D "$pg" status >/dev/null 2>&1
      }

      case "''${1:-}" in
        start) start ;;
        stop) stop ;;
        status) status ;;
        *) echo "usage: $0 start|stop|status" >&2; exit 2 ;;
      esac
    '';
  };
}
