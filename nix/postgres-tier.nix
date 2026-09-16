# The Postgres tier as ONE provisioner in two places: nixpkgs' `postgresql_18`, started from the
# same script by the `checks.nextest` sandbox and by `just test` in the dev shell.
#
# The nix build sandbox has no network and no docker socket, so a docker tier cannot be a check.
# Postgres needs neither: the ordinary fixture runs over a unix socket, and the TLS cells use one
# loopback listener whose candidate port the operating system allocates for each new cluster. The
# candidate is not a claim: the postmaster must bind it before the endpoint is published, and a
# bind race is retried. One package and one start script in both places means the two cannot drift
# (the SQL_ASCII slip in this PR's first go at a second provisioner is what one script prevents),
# and `just update` moves both.
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
  # `start` brings up (or is a no-op restart of) a socket plus loopback-TLS server and writes
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
    runtimeInputs = [ pkgs.postgresql_18 endpoints.script pkgs.coreutils pkgs.openssl pkgs.python3 ];
    text = builtins.readFile ./postgres-tier-provision.sh;
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
  # The subshell is also what makes the LAST arm possible, and that arm was missing while its
  # posture was documented: a stop can fail inside that trap, and under the errexit every venue
  # sources this file with, a failing trap command replaces the status the shell was leaving with.
  # So the subshell's own exit status is asserted, not just the tier's state afterwards.
  #
  # Declared in `flake.nix` as one line pointing here. That `checks = {` block is read TEXTUALLY by
  # two xtask gates so it cannot leave that file, and this body would put it over the 1000-line cap.
  check = pkgs.runCommand "postgres-tier"
    {
      # `psql` for the canary that proves a running server is REUSED rather than re-created. The
      # tier carries postgres as a runtime input of its own; this body needs a client too.
      nativeBuildInputs = [ tier endpoints.script pkgs.jq pkgs.postgresql_18 ];
    }
    ''
      tree="$NIX_BUILD_TOP/worktree"
      mkdir -p "$tree"
      cd "$tree"

      endpoints=.sutura-dev/endpoints.json
      # The tier derives its state path itself; the check needs it to reach the postmaster's own pid
      # file and to assert that a teardown takes the credential with the cluster.
      pg="$NIX_BUILD_TOP/.sutura-dev/pg"
      cred="$pg.cred"
      # One allocated number serves the unix socket and the loopback TLS listener. The check reads
      # it out of the same document the harness does; it is assigned after `start` publishes it.

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

      # SIGCONT plus a bounded wait for the queued shutdown a SIGSTOP left pending.
      resume_and_wait() {
        kill -CONT "$1"
        for _ in $(seq 1 60); do
          kill -0 "$1" 2>/dev/null || break
          sleep 1
        done
      }

      # --- the answer is DERIVED from the document the harness reads ---
      sutura-postgres-tier start
      expect_state 0 "a server that is running and published"
      expect_entry true "start publishes the service it brought up"
      port="$(jq -r '.services.postgres.port' "$endpoints")"
      if [ "$(jq -r '.services.postgres.host' "$endpoints")" != "$pg" ]; then
        echo "the published host is not the socket directory the server is listening on" >&2
        exit 1
      fi

      # --- THE CREDENTIAL IS PUBLISHED, AND IT IS NOT THE OLD CONSTANT ---
      # `github.com/telekom/sutura#455`. `FixtureCredential::from_env` refuses, naming the variable,
      # when any of these three is unset - so the tier publishing them is what keeps the suite able
      # to reach this server at all, and nothing else in the tree drives that subcommand.
      published="$(sutura-postgres-tier credentials)"
      for variable in \
        SUTURA_POSTGRES_TIER_USER \
        SUTURA_POSTGRES_TIER_PASSWORD \
        SUTURA_POSTGRES_TIER_DB \
        SUTURA_POSTGRES_TIER_CA \
        SUTURA_POSTGRES_TIER_CLIENT_CERT \
        SUTURA_POSTGRES_TIER_CLIENT_KEY; do
        if ! printf '%s\n' "$published" | grep -q "^export $variable=."; then
          echo "credentials published no non-empty $variable:" >&2
          printf '%s\n' "$published" >&2
          exit 1
        fi
      done
      # The point of the change, asserted rather than described: the password is generated, so it is
      # not the `sutura` every reader of the old `pub fn` could have typed.
      secret="$(printf '%s\n' "$published" | sed -n 's/^export SUTURA_POSTGRES_TIER_PASSWORD=//p')"
      if [ "$secret" = sutura ]; then
        echo "the published password is still the constant this tier exists to stop sharing" >&2
        exit 1
      fi
      # It has to be the ROLE's password too, or the client and the server agree only by accident.
      # A TCP-less server admits a unix-socket client by `trust`, so `psql` alone cannot show this -
      # `PASSWORD` in `pg_authid` is a scram verifier, and `scram-sha-256$...` over the published
      # value is what says the two halves match. Asked of the server rather than of the file.
      if ! psql -h "$pg" -p "$port" -U postgres -d postgres -tAc \
        "SELECT 1 FROM pg_authid WHERE rolname = 'sutura' AND rolpassword IS NOT NULL" | grep -q 1; then
        echo "the role carries no password, so what credentials publishes is a value nothing set" >&2
        exit 1
      fi
      # ONE value per worktree, not one per invocation: a second `start` must not re-credential a
      # server the suite is already connected to.
      sutura-postgres-tier start
      if [ "$(sutura-postgres-tier credentials)" != "$published" ]; then
        echo "a second start republished a different credential, so a running suite's would go stale" >&2
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

      # --- the YESYES ARM: an entry at a DIFFERENT socket is still state 3, and the wrapper
      # must republish it, never skip it ---
      # The withdraw arm above leaves NO entry; this one leaves a WRONG one, so state 3 comes from
      # an address mismatch while something still publishes. The old `0 | 3) alive=yes` form read
      # "a postmaster is alive" as *already up* without checking the address, so this arm was
      # skipped and the stale address survived; routing 3 to republish repairs it. RED on that
      # form, GREEN on the fix.
      sutura-tier-endpoint publish "$tree" postgres "$pg.other" "$port"
      expect_entry true "a stale entry naming another socket is still published"
      expect_state 3 "a postmaster whose entry names another address is unclaimed, not up"
      ( . ${./with-tier.sh}
        sutura_tier_up
        printf '%s' "$SUTURA_DEV_REQUIRE_TIER" > "$NIX_BUILD_TOP/required-mismatch"
      )
      if [ "$(cat "$NIX_BUILD_TOP/required-mismatch")" != 1 ]; then
        echo "the wrapper did not export SUTURA_DEV_REQUIRE_TIER over a mismatched-address tier" >&2
        exit 1
      fi
      if [ "$(jq -r '.services.postgres.host' "$endpoints")" != "$pg" ]; then
        echo "the wrapper did not republish the entry onto the live socket dir" >&2
        exit 1
      fi
      expect_state 0 "the wrapper republished the entry onto the address the server is on"
      expect_entry true "the stale-address entry was repaired"

      # --- A PRE-#298 TIER ON PATH, WHOSE `status` ANSWERS FROM THE POSTMASTER ALONE ---
      # `github.com/telekom/sutura#335`. Every arm above drives `nix/with-tier.sh` against THIS
      # tier, and which build a dev shell has on PATH is not a property of this repository: a shell
      # entered before `#298` landed carries a `status` that is `pg_ctl` and nothing else. Two
      # answers, so its exit 0 means *a postmaster* rather than *a postmaster the suite can find*,
      # the republish arm is unreachable, and a `git commit` on a green branch failed both
      # fail-closed cells - the same GREEN `status` as `#298`, measured again six days later.
      #
      # The stub IS that build: `pg_ctl` for `status`, this tier for everything else. With the
      # entry withdrawn over the live server it answers 0 where the real one answers 3, so a
      # wrapper that trusts the answer to be endpoint-derived adopts a tier the suite cannot find
      # and publishes nothing. RED on that form, GREEN on one that reads the document itself.
      stub="$NIX_BUILD_TOP/pre-298"
      mkdir -p "$stub"
      cat > "$stub/sutura-postgres-tier" <<EOS
      #!/bin/sh
      if [ "\$1" = status ]; then
        exec pg_ctl -D "$pg" status >/dev/null 2>&1
      fi
      exec ${tier}/bin/sutura-postgres-tier "\$@"
      EOS
      chmod +x "$stub/sutura-postgres-tier"
      sutura-tier-endpoint withdraw "$tree" postgres
      expect_entry absent "the claim is withdrawn while the postmaster keeps running"
      ( PATH="$stub:$PATH"
        . ${./with-tier.sh}
        sutura_tier_up )
      expect_entry true "the wrapper read the endpoint file itself rather than trusting a two-state status"
      expect_state 0 "and the entry it republished names the socket the server is on"

      # A tier that is up AND published is left alone too - the same rule, its ordinary arm.
      ( . ${./with-tier.sh}; sutura_tier_up )
      expect_state 0 "an already-published tier survives the wrapper"

      # --- what the wrapper DID start, it tears down ---
      sutura-postgres-tier stop
      expect_state 1 "a stopped tier"
      expect_entry absent "a stop that took withdraws the claim"
      # AND THE DATA DIRECTORY GOES WITH IT. Nothing here ever removed it, and nothing said so:
      # measured on one machine, nine directories from four separate days at 40 MB each. The
      # sandbox arm never showed it because `$NIX_BUILD_TOP` is fresh every build - so the only
      # venue that could see this is the only one that reuses a path, and it had no assertion.
      if [ -e "$pg" ]; then
        echo "stop left the data directory behind: $pg" >&2
        exit 1
      fi
      # AND THE CREDENTIAL WENT WITH IT, so `credentials` REFUSES rather than answering for a server
      # that is gone. The same argument the withdrawal above makes: a stale claim is worse than none,
      # and here the stale claim would be a password a fresh cluster never set.
      if [ -e "$cred" ]; then
        echo "stop left the credential behind: $cred" >&2
        exit 1
      fi
      # AND THE TLS PRIVATE KEY WENT WITH THE CLUSTER IT SIGNS. `ca.crt` beside it is public, but the
      # key is exactly what per-cluster generation keeps out of anything reused, and a fresh cluster
      # handed an old key would present a certificate matching an anchor an earlier run published.
      if [ -e "$pg.ssl" ]; then
        echo "stop left the TLS material behind: $pg.ssl" >&2
        exit 1
      fi
      refused=0
      sutura-postgres-tier credentials >/dev/null 2>&1 || refused=$?
      if [ "$refused" = 0 ]; then
        echo "credentials answered for a worktree where nothing is provisioned" >&2
        exit 1
      fi
      # A FRESH cluster gets a FRESH password. Held here because the file and the data directory are
      # removed by the same `stop`, and a reader could reasonably expect the credential to be stable
      # across a teardown - it is not, and a test is how that stays true.
      sutura-postgres-tier start
      if [ "$(sutura-postgres-tier credentials)" = "$published" ]; then
        echo "a re-provisioned tier republished the credential of the cluster that was torn down" >&2
        exit 1
      fi
      sutura-postgres-tier stop
      expect_state 1 "the re-provisioned tier tears down like any other"

      # --- a directory left by a killed run heals, at EVERY point initdb can be killed at ---
      # `github.com/telekom/sutura#377`. Two fixtures, because a structural check cannot tell them
      # apart and the first version of this fix only covered the first: measured, killing `initdb`
      # leaves no `PG_VERSION` for a few milliseconds, then `PG_VERSION` without `global/pg_control`,
      # and from ~100ms BOTH - 23 entries where a finished cluster has 22. The rule is not what is in
      # the directory but whether a postmaster is on it, which `stop` removing the directory is what
      # makes decidable.
      for fixture in early late; do
        mkdir -p "$pg/base" "$pg/global"
        : > "$pg/postgresql.auto.conf"
        if [ "$fixture" = late ]; then
          # The DOMINANT outcome, and the one the first version of this fix could not see: every
          # file a structural test would ask for, and still not a cluster any postgres can open.
          echo 18 > "$pg/PG_VERSION"
          : > "$pg/global/pg_control"
        fi
        sutura-postgres-tier start
        expect_state 0 "a $fixture wreck is cleared and the tier comes up on it"
        expect_entry true "and the server it brought up is published"
        sutura-postgres-tier stop
        expect_state 1 "the healed tier tears down like any other"
      done

      # --- a LIVE server is reused, and reuse is the only thing that means now ---
      # This replaces a guarantee that quietly lost its coverage: with `stop` removing the
      # directory, "a complete cluster is reused" can no longer be reached by a second `start`, so
      # asserting it would assert nothing. What survives - and what keeps `start` idempotent over a
      # repeated `just test` - is that a RUNNING server is left alone and its data with it.
      sutura-postgres-tier start
      port="$(jq -r '.services.postgres.port' "$endpoints")"
      psql -h "$pg" -p "$port" -U postgres -d sutura -v ON_ERROR_STOP=1 \
        -c "CREATE TABLE canary(v int)" -c "INSERT INTO canary VALUES (42)"
      sutura-postgres-tier start
      canary="$(psql -h "$pg" -p "$port" -U postgres -d sutura -tAc "SELECT v FROM canary")"
      if [ "$canary" != 42 ]; then
        echo "a second start did not reuse the running server: canary read '$canary'" >&2
        exit 1
      fi
      sutura-postgres-tier stop

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
      # AND ITS DATA. Held by line order alone before this - the `rm -rf` sits after the
      # stop-failure `exit 1`, and a reorder would delete a LIVE server's directory with every
      # explicit assertion still passing. Deleting a running postmaster's data is a worse failure
      # than the wedge this file set out to fix, so it gets a line rather than a position.
      if [ ! -e "$pg" ]; then
        echo "a failed stop deleted the live server's data directory" >&2
        exit 1
      fi
      # And its CREDENTIAL, held by line order for the same reason and given the same line: the
      # `rm -f` sits after that `exit 1`, and a reorder would leave a live server the suite is
      # connected to with no way for a later `credentials` to answer for it.
      if [ ! -e "$cred" ]; then
        echo "a failed stop deleted the live server's credential" >&2
        exit 1
      fi

      # The queued shutdown runs the moment it is resumed, so wait for it rather than racing a
      # second stop against the first one's signal.
      resume_and_wait "$postmaster"

      # And a stop that DOES take withdraws the entry; the last service out takes the file. This is
      # `github.com/telekom/sutura#231`'s lesson, kept: a claim left over a dead server makes a
      # fail-closed cell panic where the honest outcome is a skip.
      sutura-postgres-tier stop
      expect_entry absent "the withdrawal still happens when the stop succeeds"

      # --- and a failed teardown does not answer for the suite it tore down ---
      # The arm above calls `stop` directly, which is not the path a developer reaches it by:
      # `just test` reaches it through `sutura_tier_up`'s EXIT trap, and every venue that sources
      # that file runs bash with errexit, where a FAILING command in an EXIT trap REPLACES the
      # status the shell was leaving with. The wrapper's `|| true` is what keeps a teardown from
      # rewriting a test result, and this arm is the only thing holding that token: without it the
      # subshell below answers 1.
      #
      # 100 on purpose, because the number that has to survive is the DISCRIMINATING one - it is
      # nextest's *some tests failed*, and a teardown that turns it into 1 has not hidden a failure
      # but has stopped saying which failure it was.
      trap_status=0
      ( set -euo pipefail
        . ${./with-tier.sh}
        sutura_tier_up
        kill -STOP "$(head -1 "$pg/postmaster.pid")"
        # Exported after `start`, so the budget applies to the trap's stop and not to the startup
        # this arm depends on.
        export PGCTLTIMEOUT=5
        echo "--- the failed stop below is expected too, this one inside the wrapper's trap ---"
        exit 100
      ) || trap_status=$?
      if [ "$trap_status" != 100 ]; then
        echo "the wrapper's trap answered $trap_status for a body that chose 100: a failed" >&2
        echo "teardown rewrote the run's exit status, which is what \`|| true\` is there for" >&2
        exit 1
      fi
      expect_entry true "the failed teardown in the trap kept the claim over the live server"

      # Resume it so the queued shutdown completes - the pid is read while it is still SIGSTOPped,
      # because the postmaster takes its pid file with it on the way out.
      postmaster="$(head -1 "$pg/postmaster.pid")"
      resume_and_wait "$postmaster"
      # The remedy that message names, run: the retry withdraws what the failed teardown kept.
      sutura-postgres-tier stop
      expect_entry absent "the retried teardown withdraws the claim the failed one kept"

      # --- github.com/telekom/sutura#807: pg_ctl status lies when only the pidfile is gone ---
      # Reproduced directly: a live postmaster, ONLY its pidfile removed. `pg_ctl status` answers
      # "no server running" (rc=3) over exactly this server, and the old `stop` trusted that
      # answer and ran its destructive half - `rm -rf` on the data directory, the TLS material and
      # the credential - unconditionally underneath it.
      sutura-postgres-tier start
      postmaster="$(head -1 "$pg/postmaster.pid")"
      pidfile_backup="$NIX_BUILD_TOP/postmaster.pid.bak"
      cp "$pg/postmaster.pid" "$pidfile_backup"
      rm -f "$pg/postmaster.pid"
      if pg_ctl -D "$pg" status >/dev/null 2>&1; then
        echo "pg_ctl status did not reproduce #807's lie: it still sees the server" >&2
        exit 1
      fi
      echo "--- the refusal below is expected, its message included ---"
      refused=0
      stop_output="$(sutura-postgres-tier stop 2>&1)" || refused=$?
      echo "$stop_output" >&2
      if [ "$refused" = 0 ]; then
        echo "stop reported success over a live cluster whose pidfile alone was removed" >&2
        exit 1
      fi
      for path in "$pg" "$pg.ssl" "$cred"; do
        if [ ! -e "$path" ]; then
          echo "a refused stop still deleted $path" >&2
          exit 1
        fi
      done
      # `pg_ctl stop -m fast` also fails with no pidfile, so a neutralised `exit 1` here (echo
      # kept, predicate still evaluated) still falls through to the generic "did not stop"
      # refusal - pin the accurate message AND that it alone fired, not both.
      case "$stop_output" in
        *"pg_ctl has no pid to signal"*"did not stop"*) echo "stop's refusal fell through to the generic message too" >&2; exit 1 ;;
        *"pg_ctl has no pid to signal"*) ;;
        *) echo "stop fell through to the generic, misleading 'did not stop' message" >&2; exit 1 ;;
      esac
      if ! kill -0 "$postmaster" 2>/dev/null; then
        echo "the postmaster died even though stop refused to touch its state" >&2
        exit 1
      fi
      expect_entry true "the endpoint claim over a live server whose pidfile is gone STAYS"
      cp "$pidfile_backup" "$pg/postmaster.pid"
      sutura-postgres-tier stop
      expect_state 1 "a genuinely stopped tier tears down fully once its pidfile is back"

      # --- the FIFTH state: SIGSTOPped AND its pidfile gone, `alive` cannot prove either way ---
      # #807's fixture plus one signal: frozen, not dead, so `pg_ctl status` fails the same way
      # (no pidfile) and `alive`'s `psql` neither answers nor is refused before its timeout. A
      # larger timeout only widens how long this arm waits, never whether it passes.
      sutura-postgres-tier start
      postmaster="$(head -1 "$pg/postmaster.pid")"
      pidfile_backup="$NIX_BUILD_TOP/postmaster.pid.fifth.bak"
      cp "$pg/postmaster.pid" "$pidfile_backup"
      kill -STOP "$postmaster"
      rm -f "$pg/postmaster.pid"
      echo "--- the refusal below is expected, its message included ---"
      unknown=0
      started="$(date +%s)"
      sutura-postgres-tier stop || unknown=$?
      elapsed=$(( $(date +%s) - started ))
      [ "$unknown" != 0 ] || { echo "stop reported success over a frozen, not dead, postmaster" >&2; exit 1; }
      [ "$elapsed" -ge 4 ] || { echo "stop answered in ''${elapsed}s, skipping the timeout path" >&2; exit 1; }
      for path in "$pg" "$pg.ssl" "$cred"; do
        [ -e "$path" ] || { echo "a refused stop over the frozen postmaster still deleted $path" >&2; exit 1; }
      done
      # `ps` is not on this sandbox's PATH; `kill -0` is this file's existing liveness idiom.
      kill -0 "$postmaster" 2>/dev/null || { echo "the postmaster died during a refused stop" >&2; exit 1; }
      expect_entry true "the endpoint claim over a frozen, pidfile-less server STAYS"
      resume_and_wait "$postmaster"
      cp "$pidfile_backup" "$pg/postmaster.pid"
      sutura-postgres-tier stop
      expect_state 1 "a resumed tier with its pidfile restored tears down fully"

      # --- concurrent writers to the SHARED document do not race ---
      # `github.com/telekom/sutura#528` item 3, and the shared writer's bug, not this tier's: two
      # `sutura-tier-endpoint publish`es for DIFFERENT services used one hardcoded `$file.new`, so
      # the second's `mv` found the first already gone. Backgrounded and waited on, genuinely
      # concurrent rather than sequential - a serialised pair would never have reached the race.
      sutura-tier-endpoint publish "$tree" c4-race-a 127.0.0.1 11111 &
      race_a=$!
      sutura-tier-endpoint publish "$tree" c4-race-b 127.0.0.1 22222 &
      race_b=$!
      race_failed=0
      wait "$race_a" || race_failed=1
      wait "$race_b" || race_failed=1
      if [ "$race_failed" != 0 ]; then
        echo "a concurrent publish exited non-zero: the shared writer is not serialised" >&2
        exit 1
      fi
      if [ "$(jq -r '.services["c4-race-a"].port' "$endpoints")" != 11111 ] || \
         [ "$(jq -r '.services["c4-race-b"].port' "$endpoints")" != 22222 ]; then
        echo "a concurrent pair of publishes lost one entry to the other's read-modify-write" >&2
        exit 1
      fi
      sutura-tier-endpoint withdraw "$tree" c4-race-a
      sutura-tier-endpoint withdraw "$tree" c4-race-b
      expect_entry absent "the race fixture withdraws cleanly behind it"

      touch $out
    '';
}
