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
let
  # The one writer for `endpoints.json`, shared with every other nix-native tier. It replaced the
  # single `printf` that used to write the whole document here, which was correct only while this
  # was the only nix tier: see `nix/tier-endpoints.nix` for the entry it would have dropped.
  endpoints = import ./tier-endpoints.nix { inherit pkgs; };
in
# `rec` so `check` can drive `tier`: the check exists to run this exact script, and a second
# reference to it through `flake.nix` would be a second thing to keep pointing here.
rec {
  package = pkgs.postgresql_18;

  # The provisioner, usable from any shell that has it and `postgresql`'s binaries on PATH; the
  # dev shell gets this on PATH through `devenv.nix`, the sandbox gets it as a native input.
  #
  # `start` brings up (or is a no-op restart of) a socket-only server and writes
  # `<cwd>/.sutura-dev/endpoints.json` naming its socket directory, so `sutura_dev::provisioned::here`
  # can read it unchanged. `stop` tears it back down AND WITHDRAWS THAT ENTRY, and `status` answers
  # whether a server is up without changing anything.
  #
  # # ONE record, because two readings of it diverged and a suite run paid for it
  #
  # `status` used to answer from `pg_ctl` while the harness answered from `endpoints.json` - two
  # statements about one fact, and `github.com/telekom/sutura#298` is them disagreeing in the
  # direction that blocks work. A postmaster outlived a teardown that had already withdrawn its
  # entry; `nix/with-tier.sh` read `status`, was told *already up*, started nothing, and every
  # fail-closed cell then panicked on a worktree that published nothing. One `just test` discarded,
  # and most of the cost was working out that a GREEN `status` was the reason.
  #
  # Two changes, and neither is a tolerance widened until the symptom went away:
  #
  # * **`status` is DERIVED from the endpoint file.** The harness reads that document, so that
  #   document is the fact and this answer is a function of it - see the three states at `status`
  #   itself. The state above is self-healing now rather than terminal: it answers *unclaimed*,
  #   `start` republishes the entry, and `start` was already idempotent about a live postmaster.
  # * **A `stop` that cannot stop does not withdraw.** The old pairing - `pg_ctl stop -m fast`
  #   under `|| true` beside an unconditional withdrawal - made the claim the WEAKER of the two
  #   records, retracted whatever happened while the postmaster's death was conditional. That is
  #   the pairing that manufactured the divergence, so a failed stop now keeps the claim and fails
  #   loudly. `checks.postgres-tier` drives both, including the failed stop.
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
    runtimeInputs = [ pkgs.postgresql_18 endpoints.script ];
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
        # The harness reads `<root>/.sutura-dev/endpoints.json` and treats the host as the socket
        # dir. MERGED rather than written whole: a second nix tier's entry lives in the same file.
        sutura-tier-endpoint publish "$root" postgres "$pg" "$port"
      }

      stop() {
        # The `|| true` this replaces was right about one thing and wrong about the distinction: a
        # tier that was never started must not fail a teardown, but *never started* and *would not
        # stop* are not the same state and ignoring the exit code answered both. So the question is
        # asked instead - and a server that is running and did not stop keeps its entry, because
        # withdrawing a claim over a live postmaster is exactly how a tier came to be `up` to a
        # wrapper and absent to every test.
        if [ -d "$pg" ] && pg_ctl -D "$pg" status >/dev/null 2>&1; then
          if ! pg_ctl -D "$pg" stop -m fast; then
            echo "postgres tier: the server did not stop, so its endpoint entry STAYS." >&2
            echo "               It is still running and still discoverable, which is the honest" >&2
            echo "               state - a withdrawn claim over a live server is what made a tier" >&2
            echo "               'already up' to the wrapper and absent to the suite. Retry the" >&2
            echo "               teardown (\`just postgres-tier stop\` in a dev shell)." >&2
            exit 1
          fi
        fi
        # The endpoint entry is a claim that a server is there. Withdraw it, or discovery keeps
        # believing it and the cells fail on a dead socket instead of skipping. Withdrawing the
        # LAST service removes the file, which is what a postgres-only worktree saw when this was a
        # bare `rm -f`; a tier that was never started has nothing to withdraw and that is not a
        # failure.
        sutura-tier-endpoint withdraw "$root" postgres
      }

      # Is a server up, and up in the way THE SUITE will see it? Nothing is changed, and the answer
      # is the exit code - so a wrapper can stop only what it started rather than trampling a tier
      # somebody else brought up.
      #
      # THREE answers, because a wrapper needs two bits out of one fact and asking two commands for
      # them is how they came apart:
      #
      #   0  a postmaster is running AND this worktree's `endpoints.json` publishes it at that
      #      socket directory - a fail-closed cell will find it
      #   3  a postmaster is running and nothing publishes it - `start` heals that, and this server
      #      is NOT the caller's to tear down
      #   1  nothing is running here
      #
      # A boolean caller (`if ... status`) reads 3 as down, which is the honest answer to the
      # question it asked: there is nothing the suite can reach. `start` deliberately does not go
      # through this - its own guard is the postmaster alone, which is what keeps it idempotent over
      # a server whose entry has gone.
      status() {
        pg_ctl -D "$pg" status >/dev/null 2>&1 || return 1
        sutura-tier-endpoint published "$root" postgres "$pg" "$port" || return 3
      }

      case "''${1:-}" in
        start) start ;;
        stop) stop ;;
        status) status ;;
        *) echo "usage: $0 start|stop|status" >&2; exit 2 ;;
      esac
    '';
  };

  # The tier's state machine, driven end to end - and no other check can see it. `checks.nextest`
  # starts this tier and stops it, so a green run there says a server came up: it says nothing
  # about whether `status` and `endpoints.json` agree, nothing about what a stop that FAILS does to
  # the claim, and nothing about which of those two records the wrapper acts on. All three are
  # `github.com/telekom/sutura#298`.
  #
  # **It SOURCES `nix/with-tier.sh` rather than reasoning about it**, because the defect was in
  # neither half alone - it was a skip-or-start decision reading a different record from the one
  # the suite reads. Each arm runs in a SUBSHELL, since `sutura_tier_up` arms an EXIT trap in the
  # shell that sources it, and that trap firing (or not) is precisely what is under test: the
  # wrapper must tear down what it started and must not adopt a server it did not.
  #
  # Declared in `flake.nix` as one line pointing here. That `checks = {` block is read TEXTUALLY by
  # two xtask gates so it cannot leave that file, and this body would put it over the 1000-line cap.
  check = pkgs.runCommand "postgres-tier"
    {
      nativeBuildInputs = [ tier endpoints.script pkgs.jq ];
    }
    ''
      tree="$NIX_BUILD_TOP/worktree"
      mkdir -p "$tree"
      cd "$tree"

      endpoints=.sutura-dev/endpoints.json
      # The tier derives this itself; the check needs it to reach the postmaster's own pid file.
      pg="$NIX_BUILD_TOP/.sutura-dev/pg"

      tier_state() {
        state=0
        sutura-postgres-tier status || state=$?
        printf '%s' "$state"
      }

      expect_state() {
        got="$(tier_state)"
        if [ "$got" != "$1" ]; then
          echo "status answered $got, expected $1 - $2" >&2
          exit 1
        fi
      }

      # `absent`, `true` or `false`, and the three are deliberately one scale: the file's EXISTENCE
      # is what discovery reads as "something is provisioned here", so a missing file and a missing
      # entry are different states and an assertion that cannot tell them apart is worth less than
      # it looks. A bare `test` would answer both with an exit code and no sentence.
      expect_entry() {
        got=absent
        if [ -f "$endpoints" ]; then
          got="$(jq -r '.services | has("postgres")' "$endpoints")"
        fi
        if [ "$got" != "$1" ]; then
          echo "the postgres entry is '$got', expected '$1' - $2" >&2
          exit 1
        fi
      }

      # --- the answer is DERIVED from the document the harness reads ---
      sutura-postgres-tier start
      expect_state 0 "a server that is running and published"
      expect_entry true "start publishes the service it brought up"
      if [ "$(jq -r '.services.postgres.host' "$endpoints")" != "$pg" ]; then
        echo "the published host is not the socket directory the server is listening on" >&2
        exit 1
      fi

      # The divergence, made on purpose: withdraw the claim and leave the postmaster running. That
      # is the state #298 was filed in, and `pg_ctl status` on its own called it up.
      sutura-tier-endpoint withdraw "$tree" postgres
      expect_entry absent "the last service out takes the file with it"
      # 3 rather than 0 IS what a boolean caller needs, and no separate assertion says so: `if
      # ... status` over a non-zero answer is true by construction, so a second test here would
      # only restate the line above and read as coverage.
      expect_state 3 "a running server nothing publishes is unclaimed, not up"

      # --- the wrapper heals that state instead of failing the suite closed ---
      ( . ${./with-tier.sh}
        sutura_tier_up
        printf '%s' "$SUTURA_DEV_REQUIRE_TIER" > "$NIX_BUILD_TOP/required"
      )
      if [ "$(cat "$NIX_BUILD_TOP/required")" != 1 ]; then
        echo "the wrapper did not export SUTURA_DEV_REQUIRE_TIER over a tier it made reachable" >&2
        exit 1
      fi
      # Still up after that subshell exited, which a stricter `status` alone would have broken: the
      # wrapper republished a claim for a server it did not start, so it armed no teardown for it.
      expect_state 0 "the wrapper republished the entry and left the server alone"
      expect_entry true "the wrapper republished the entry the suite reads"

      # A tier that is up AND published is left alone too - the same rule, its ordinary arm.
      ( . ${./with-tier.sh}; sutura_tier_up )
      expect_state 0 "an already-published tier survives the wrapper"

      # --- what the wrapper DID start, it tears down ---
      sutura-postgres-tier stop
      expect_state 1 "a stopped tier"
      expect_entry absent "a stop that took withdraws the claim"
      ( . ${./with-tier.sh}; sutura_tier_up )
      expect_state 1 "the wrapper's EXIT trap stopped the server it started"
      expect_entry absent "that teardown withdrew the claim too"

      # --- a stop that does not take keeps the claim ---
      # SIGSTOP on the postmaster is a fast shutdown that cannot complete: the signal reaches a
      # process that cannot act on it, so `pg_ctl` gives up at `PGCTLTIMEOUT` with the server still
      # there. That is the path the old `|| true` swallowed, and it is what made the divergence
      # reachable without anybody having done anything wrong.
      sutura-postgres-tier start
      postmaster="$(head -1 "$pg/postmaster.pid")"
      kill -STOP "$postmaster"
      echo "--- the failed stop below is expected, its message included ---"
      failed=0
      PGCTLTIMEOUT=5 sutura-postgres-tier stop || failed=$?
      if [ "$failed" = 0 ]; then
        echo "stop reported success over a server it had not stopped" >&2
        exit 1
      fi
      expect_entry true "a stop that did not take keeps the claim over the live server"

      # The queued shutdown runs the moment it is resumed, so wait for it rather than racing a
      # second stop against the first one's signal.
      kill -CONT "$postmaster"
      for _ in $(seq 1 60); do
        kill -0 "$postmaster" 2>/dev/null || break
        sleep 1
      done

      # And a stop that DOES take withdraws the entry; the last service out takes the file. This is
      # `github.com/telekom/sutura#231`'s lesson, kept: a claim left over a dead server makes a
      # fail-closed cell panic where the honest outcome is a skip.
      sutura-postgres-tier stop
      expect_entry absent "the withdrawal still happens when the stop succeeds"

      touch $out
    '';
}
