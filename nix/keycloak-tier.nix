# The Keycloak tier, nix-native: the CI venue for the identity provider, on
# `nix/postgres-tier.nix`'s pattern.
#
# `compose.services.yaml` is the DEMO venue - a person's machine, a docker socket, the `identity`
# profile. This is the other one: one start/stop/status script over the `nixpkgs` package, run by
# `checks.keycloak-tier` inside the sandbox and by `just keycloak-tier` from the same script, so the
# two cannot drift. It needs no docker socket and no network beyond loopback.
#
# # What it provisions, and why NO HUMAN is in the loop
#
# A realm, one confidential client and TWO users, through `kcadm.sh`. Two rather than one is the
# whole point: `docs/adr/0008` draws the two-subject property, and a tier that can only produce one
# subject's token cannot be cited for it. Every credential is GENERATED at `start` - there is no
# password anywhere in this file, in the repository, or in a fixture, which is what makes a public
# repository and a working identity fixture compatible.
#
# `start` then **proves its own provisioning before it reports success**: it asks the token endpoint
# for an access token as each subject and exits non-zero if either does not come back. So "no human
# is needed" is a fail-closed check rather than a claim - a realm that came up half-provisioned is a
# red run at the point of provisioning, not a puzzling refusal in a test twenty minutes later.
#
# # The two things a reader should know before citing this tier
#
# **What it can be cited for that `sutura_dev::issuer` cannot:** a real provider's own documents and
# signatures - an `RS256` token, the JWKS it publishes for it, the discovery document, and the
# question that module names as having exactly one venue (what a real provider will actually mint).
# The mock issuer cannot generate an RSA key at all, by deliberate design, so that path has no other
# venue.
#
# **What it still cannot be cited for:** two subjects reading two row sets. Nothing in this
# repository carries a per-subject credential into a data system yet - `compose.services.yaml` says
# so at length and this tier does not change it. It makes leg 1 provable against a real provider; it
# does not make leg 2 exist.
#
# # Three mechanics that are not obvious
#
# **The package is already augmented, so nothing is copied.** `nixpkgs` runs `kc.sh build` in its
# own build phase, so the store tree carries `lib/quarkus`. What Keycloak needs on top of that is a
# WRITABLE `kc.home.dir` for its embedded store - and it looks for the build marker under that same
# directory. So the home is a symlink farm onto the store package with one real directory, `data`,
# and the server starts `--optimized`: no augmentation at run time, no write next to the jar, no
# 186 MB copy. Measured on 2026-09-03: `start-dev` against the store package fails with
# `AccessDeniedException: .../lib/quarkus/transformed-bytecode.jar`, and `--optimized` against an
# empty home fails with *"the '--optimized' flag was used for first ever server start"* - the farm is
# what satisfies both.
#
# **THE PORT IS CHOSEN BY THE OPERATING SYSTEM, not by us.** `--http-port=0` binds an ephemeral port
# and the server prints it; the tier reads it back out of the log and publishes it into
# `.sutura-dev/endpoints.json`. That is `docs/adr/0009`'s rule applied to a service that, unlike
# Postgres, cannot be reached over a unix socket: there is no fixed port to collide with a neighbour
# worktree, and no "check whether the port is free, then bind" window. `--http-management-port=0`
# is there for the same reason and is easy to miss - Keycloak's management interface defaults to a
# FIXED 9000, which two worktrees would fight over even though nothing here reads it.
#
# **`--cache=local`, and this is the one a reader is most likely to leave out.** `--http-port=0`
# removes the port that is visible; production mode also opens an Infinispan/JGroups channel on a
# FIXED range, 7800 to 7810, before it serves anything. Measured on 2026-09-03: the check passed on
# a machine where that range happened to be free and the SAME derivation, rebuilt minutes later,
# failed with *"Unable to start JGroups Channel: No available port to bind to in range
# [7800 .. 7810]"*. A local cache has nobody to replicate to and no channel to open, so the second
# fixed port goes away rather than being allocated around. It is a RUN-time option, verified rather
# than assumed: passing it through the package's `confFile` instead makes `kc.sh build` print *"The
# following run time options were found, but will be ignored during build time: kc.cache"*.
#
# **`stop` kills a process GROUP.** `kc.sh` is a shell script that spawns the JVM rather than
# `exec`ing it, so killing the script's pid leaves Keycloak running. `set -m` makes the launch its
# own process group and `kill -- -$pid` takes the whole group, which needs no `pgrep` - `procps` is
# not in a nix build sandbox's PATH and is not portable to darwin.
{ pkgs }:
let
  endpoints = import ./tier-endpoints.nix { inherit pkgs; };

  # The realm this tier provisions. Named once, here, because the tier script, the check that runs
  # it and anything that later reads the realm file all have to agree on it.
  realm = "sutura-dev";
  client = "sutura-dev-cli";
  # The two subjects `docs/adr/0008`'s property needs. Deliberately not people: a fixture that
  # looks like somebody's account in a public repository is a disclosure with extra steps.
  subjects = [ "subject-a" "subject-b" ];
in
# `rec` so `check` can drive `tier`: the check exists to run this exact script, and a second
# reference to it through `flake.nix` would be a second thing to keep pointing here.
rec {
  package = pkgs.keycloak;

  # Where the realm, the client and the two subjects are written for a reader.
  #
  # NOT in `endpoints.json`: that file is the discovery contract and holds addresses only, and
  # `dev/src/discovery.rs` is deliberately narrow about what an endpoint is. Credentials go beside
  # it in their own file, generated per `start`, `0600`, and removed by `stop`.
  realmFile = ".sutura-dev/keycloak-realm.json";

  inherit realm client subjects;

  tier = pkgs.writeShellApplication {
    name = "sutura-keycloak-tier";
    runtimeInputs = [
      pkgs.keycloak
      pkgs.curl
      pkgs.coreutils
      endpoints.script
    ];
    text = ''
      set -o errexit -o nounset

      root="$(pwd -P)"
      state="$root/.sutura-dev"
      realm=${realm}
      client=${client}
      subjects="${builtins.concatStringsSep " " subjects}"

      # The server's own directory. In the sandbox `$NIX_BUILD_TOP` is per-build and goes away with
      # it; in a dev shell it is keyed by a hash of the worktree so two worktrees cannot share an
      # embedded store - the fixture nobody can debug.
      if [ -n "''${NIX_BUILD_TOP:-}" ]; then
        home="$NIX_BUILD_TOP/sutura-keycloak"
      else
        key="$(printf '%s' "$root" | cksum | cut -d' ' -f1)"
        home="''${TMPDIR:-/tmp}/sutura-keycloak-$key"
      fi
      pidfile="$home/tier.pid"
      log="$home/server.log"
      admincfg="$home/kcadm.json"
      realmfile="$state/keycloak-realm.json"

      # Keycloak reads both from the environment - `nixpkgs` patches its launcher for exactly this,
      # which is what lets the store package stay read-only while the state does not.
      export KC_HOME_DIR="$home"
      export KC_CONF_DIR="${pkgs.keycloak}/conf"
      # The JVM and `kcadm.sh` both write under `$HOME`; in the sandbox there is not one.
      export HOME="$home"

      # A throwaway credential, generated per start. Nothing here is ever written down.
      generated() {
        head -c 24 /dev/urandom | base64 | tr -dc 'A-Za-z0-9'
      }

      # Is a JVM of OURS running? THE PROCESS AND NOTHING DERIVED, and that is the whole point of
      # this function existing separately: it is `start`'s guard, and `start`'s cold path `rm -rf`s
      # the home. A guard that read `endpoints.json` would go false over a live server whose entry
      # something dropped, the home would be deleted under it, and the outcome is two JVMs on two
      # OS-chosen ports sharing one realm file. `nix/postgres-tier.nix` keeps `pg_ctl` for the same
      # reason and says so at its own `status`.
      running() {
        [ -f "$pidfile" ] || return 1
        kill -0 -- "-$(cat "$pidfile")" 2>/dev/null
      }

      # Is the tier up, and up in the way A READER will see it? Nothing is changed and the answer
      # is the exit code, so a caller can tear down only what it brought up.
      #
      # THREE answers, `nix/postgres-tier.nix`'s three. Splitting them off `start`'s guard is what
      # made deriving this possible at all - `github.com/telekom/sutura#324` - because until then
      # this function WAS the guard and could not answer about a document without the guard
      # answering about it too:
      #
      #   0  a JVM is running AND both records `start` writes still stand - a reader will find it
      #   3  a JVM is running and those records do not stand: unclaimed. `start` republishes what
      #      it can, and this server is NOT the caller's to tear down
      #   1  nothing is running here
      #
      # A boolean caller (`if ... status`) reads 3 as down, which is the honest answer to the
      # question it asked: there is nothing usable published here.
      #
      # **BOTH records, which is where this differs from postgres, and the difference is that this
      # tier writes two.** `endpoints.json` carries the address, and the realm file carries the
      # client secret, the admin password and the subject passwords - generated per `start` and
      # written nowhere else. `stop` withdraws both, so *does the claim I made still stand* is both
      # of them. Answering 0 on the address alone would put `start` straight back on its no-op
      # branch over a tier nothing can authenticate against, which is the defect #324 was filed for
      # wearing a different hat.
      #
      # What it does NOT answer: whether the realm file DESCRIBES this server. The run that owns
      # this home is what wrote it and `stop` is what removes it, so the two travel together
      # through every state this script can reach; a file put there by hand is not one of them.
      status() {
        running || return 1
        port="$(published_port)"
        [ -n "$port" ] || return 3
        [ -f "$realmfile" ] || return 3
        sutura-tier-endpoint published "$root" keycloak 127.0.0.1 "$port" || return 3
      }

      # The port the operating system chose, out of the server's own log. Silent rather than noisy
      # when there is no log yet: `status` asks this of a tier that may never have started, and
      # under `pipefail` a `sed` on a missing file is a failed pipeline rather than an answer.
      published_port() {
        [ -f "$log" ] || return 0
        sed -n 's|.*Listening on: http://127\.0\.0\.1:\([0-9]*\).*|\1|p' "$log" | tail -1
      }

      provision() {
        port="$1"
        base="http://127.0.0.1:$port"
        client_secret="$(generated)"

        # The admin console is answerable a moment after the port is, so the login is retried
        # rather than assumed. Everything after it is a hard failure: a realm that did not get
        # created is not something to continue past.
        for _ in $(seq 1 30); do
          if kcadm.sh config credentials --config "$admincfg" --server "$base" \
            --realm master --user "$admin_user" --password "$admin_password" >/dev/null 2>&1; then
            break
          fi
          sleep 2
        done

        kcadm.sh create realms --config "$admincfg" -s realm="$realm" -s enabled=true >/dev/null
        # Confidential, with the direct access grant: that is the flow a test uses to obtain a
        # token for a named subject without a browser. `standardFlowEnabled=false` because nothing
        # here redirects, and a client offering a flow nobody uses is surface for free.
        kcadm.sh create clients --config "$admincfg" -r "$realm" \
          -s clientId="$client" -s enabled=true -s publicClient=false \
          -s directAccessGrantsEnabled=true -s standardFlowEnabled=false \
          -s secret="$client_secret" >/dev/null

        # `requiredActions=[]` and a complete profile are load-bearing, not decoration. Measured on
        # 2026-09-03: a user created with a username alone is refused at the token endpoint with
        # `invalid_grant: Account is not fully set up`, because the realm's default profile action
        # is still pending - a human at a browser is exactly what would have cleared it, which is
        # the one thing this tier may not need.
        for subject in $subjects; do
          kcadm.sh create users --config "$admincfg" -r "$realm" \
            -s username="$subject" -s enabled=true -s emailVerified=true \
            -s email="$subject@example.com" -s firstName="$subject" -s lastName=fixture \
            -s 'requiredActions=[]' >/dev/null
          kcadm.sh set-password --config "$admincfg" -r "$realm" \
            --username "$subject" --new-password "$(subject_password "$subject")" >/dev/null
        done

        # PROVE IT, before reporting success. A tier whose realm came up half-provisioned must fail
        # here, where the message is about provisioning, rather than in whatever reads it next.
        for subject in $subjects; do
          token="$(curl -sS --max-time 20 -X POST \
            "$base/realms/$realm/protocol/openid-connect/token" \
            -d grant_type=password -d client_id="$client" -d client_secret="$client_secret" \
            -d username="$subject" -d "password=$(subject_password "$subject")")"
          case "$token" in
            *access_token*) ;;
            *)
              echo "keycloak tier: $subject could not obtain a token from $realm" >&2
              echo "$token" >&2
              exit 1
              ;;
          esac
        done

        # The realm's own credentials, for a reader that needs a token. `umask` first: the file
        # holds generated secrets and the state directory is inside the worktree.
        (
          umask 077
          {
            printf '{"issuer":"%s/realms/%s",' "$base" "$realm"
            printf '"discovery":"%s/realms/%s/.well-known/openid-configuration",' "$base" "$realm"
            printf '"realm":"%s","client":{"id":"%s","secret":"%s"},' \
              "$realm" "$client" "$client_secret"
            printf '"admin":{"username":"%s","password":"%s"},' "$admin_user" "$admin_password"
            printf '"subjects":['
            separator=
            for subject in $subjects; do
              printf '%s{"username":"%s","password":"%s"}' \
                "$separator" "$subject" "$(subject_password "$subject")"
              separator=,
            done
            printf ']}\n'
          } > "$realmfile"
        )
      }

      # One password per subject, derived from this start's own seed so it never has to be stored
      # between the two places that need it.
      subject_password() {
        printf '%s-%s' "$1" "$password_seed"
      }

      # The heal for a live server this worktree has stopped claiming. Both writers of
      # `endpoints.json` merge per ENTRY now (`github.com/telekom/sutura#317`), so a `dev-up` no
      # longer takes this tier's entry with it - but the state is still reachable without anybody
      # having done anything wrong: a `start` that dies between the bind and its publish, a
      # hand-removed file, a `stop` that failed. The JVM survives, its entry does not.
      #
      # The entry that goes back is THE SAME ENTRY - the port is the one the server itself printed
      # into its log, and nothing here re-provisions or re-generates. That is the point of healing
      # rather than restarting: a cold start over this server would first `rm -rf` the home under a
      # live JVM, and even done politely it mints a new realm, a new client secret and a new
      # OS-chosen port, so everything holding the old realm file has to re-read it.
      republish() {
        port="$(published_port)"
        if [ -z "$port" ]; then
          echo "keycloak tier: a server is running here and its own log names no port, so there" >&2
          echo "               is nothing to republish. Tear it down with" >&2
          echo "               \`just keycloak-tier stop\` and start again." >&2
          exit 1
        fi
        # The credentials are NOT RECOVERABLE and this is the one arm that cannot heal. Every
        # secret this tier has was generated at its `start` and written to the realm file alone -
        # not to the server, which only ever saw the hashes, and not to `endpoints.json`, which
        # holds addresses by design. So a republished entry would name a live server nothing can
        # authenticate against. Refuse, name the remedy, and leave the killing to a person: a token
        # something else obtained from this realm is still valid until the JVM goes.
        if [ ! -f "$realmfile" ]; then
          echo "keycloak tier: a server is running here and $realmfile is gone with the client" >&2
          echo "               secret, the admin password and the subject passwords in it. They" >&2
          echo "               are generated per start and written nowhere else, so no entry" >&2
          echo "               republished over this server would be usable. Tear it down with" >&2
          echo "               \`just keycloak-tier stop\` and start again for a fresh realm." >&2
          exit 1
        fi
        # PROVE THE REALM BEFORE CLAIMING IT AGAIN. The entry is a claim that a PROVISIONED realm
        # is at this address - the cold path states that by publishing last - and a republish is
        # the same claim made from a log line and a file rather than from a provisioning that just
        # ran. Keycloak answers 404 at this path for a realm it does not have, so this separates
        # *the port answers* from *the realm these records name is what is behind it*.
        if ! curl -sSf --max-time 20 \
          "http://127.0.0.1:$port/realms/$realm/.well-known/openid-configuration" >/dev/null; then
          echo "keycloak tier: $realm did not answer at 127.0.0.1:$port, so nothing is claimed" >&2
          echo "               for it. Tear the server down with \`just keycloak-tier stop\`" >&2
          echo "               and start again." >&2
          exit 1
        fi
        echo "keycloak tier: republishing the entry for the server already running here."
        sutura-tier-endpoint publish "$root" keycloak 127.0.0.1 "$port"
      }

      start() {
        # THE GUARD IS THE PROCESS, and what the records say about it is a second question asked
        # after it. Both were one question until `github.com/telekom/sutura#324`, which is why a
        # dropped entry used to print *already up* and return 0 having published nothing.
        if running; then
          if status; then
            echo "keycloak tier: already up - leaving it to whoever started it."
            return 0
          fi
          republish
          return 0
        fi
        # Not running, so whatever is in the home is from a previous run: an embedded store that no
        # longer matches the realm file is worse than a cold start.
        rm -rf "$home"
        mkdir -p "$home/data" "$state"
        # The read-only halves come from the store package, which `nixpkgs` has already run
        # `kc.sh build` over; `data` is the one directory the server writes.
        for part in bin lib conf providers themes; do
          ln -s "${pkgs.keycloak}/$part" "$home/$part"
        done

        admin_user=tier-admin
        admin_password="$(generated)"
        password_seed="$(generated)"
        export KC_BOOTSTRAP_ADMIN_USERNAME="$admin_user"
        export KC_BOOTSTRAP_ADMIN_PASSWORD="$admin_password"

        # `set -m` puts the launch in its own process group; `stop` kills the group, because
        # `kc.sh` spawns the JVM rather than replacing itself with it.
        set -m
        kc.sh start --optimized --cache=local --http-enabled=true --hostname-strict=false \
          --http-host=127.0.0.1 --http-port=0 --http-management-port=0 \
          >"$log" 2>&1 &
        echo "$!" > "$pidfile"
        set +m

        port=
        for _ in $(seq 1 90); do
          port="$(published_port)"
          [ -n "$port" ] && break
          # `running`, not `status`: nothing is published yet at this point in a cold start, so
          # the derived answer is 3 here by construction and the question being asked is whether
          # the JVM is still alive.
          if ! running; then
            echo "keycloak tier: the server exited before it published a port" >&2
            tail -20 "$log" >&2
            exit 1
          fi
          sleep 2
        done
        if [ -z "$port" ]; then
          echo "keycloak tier: no port published within the timeout" >&2
          tail -20 "$log" >&2
          stop
          exit 1
        fi

        provision "$port"
        # Published LAST, and that ordering is the contract: the entry in `endpoints.json` is a
        # claim that a provisioned realm is there, so it may not appear before the realm does.
        sutura-tier-endpoint publish "$root" keycloak 127.0.0.1 "$port"
      }

      stop() {
        if [ -f "$pidfile" ]; then
          pid="$(cat "$pidfile")"
          kill -TERM -- "-$pid" 2>/dev/null || true
          for _ in $(seq 1 20); do
            kill -0 -- "-$pid" 2>/dev/null || break
            sleep 1
          done
          kill -KILL -- "-$pid" 2>/dev/null || true
          rm -f "$pidfile"
        fi
        # Withdraw the claim - both halves of it. A stale endpoint makes a fail-closed cell panic
        # on a dead server where the honest outcome is a skip, and a stale realm file hands out
        # credentials for a realm that is gone.
        sutura-tier-endpoint withdraw "$root" keycloak
        rm -f "$realmfile"
      }

      case "''${1:-}" in
        start) start ;;
        stop) stop ;;
        status) status ;;
        *) echo "usage: $0 start|stop|status" >&2; exit 2 ;;
      esac
    '';
  };

  # The identity tier, brought up and provisioned INSIDE the sandbox - the nix-native venue
  # for `compose.services.yaml`'s `keycloak`, whose demo venue is a docker profile.
  #
  # **What it holds, and it is not "a server started".** `sutura-keycloak-tier start`
  # provisions a realm, a confidential client and two subjects through `kcadm.sh` and then
  # asks the token endpoint for a token AS each subject, failing if either does not come
  # back. So this check is the mechanical form of the claim that the tier needs NO HUMAN: a
  # realm that came up half-provisioned, a flow a Keycloak upgrade turns off, or a required
  # action that reappears is a red check here rather than a puzzling refusal in whatever
  # reads it next. It asserts the harness contract on top of that - `endpoints.json` names
  # the port the operating system chose, the realm file names both subjects, and `stop`
  # withdraws both claims.
  #
  # **And it drives the three states `status` answers, which no other venue can see.** A
  # green run elsewhere says a server came up; it says nothing about a server whose entry
  # something else dropped, and that state is reachable without anybody having done anything
  # wrong - a `start` that dies between the bind and its publish, a hand-removed file, a
  # `stop` that failed. So the unclaimed arm
  # is made here on purpose: withdraw the entry over the live JVM, assert 3 rather than 0,
  # and assert that `start` heals it with THE SAME PID and THE SAME CLIENT SECRET. The pid is
  # the load-bearing half - a heal and a second JVM on a second OS-chosen port are both
  # "published again" to every other assertion in this file.
  #
  # **Its own check rather than `nextest`'s `preCheck`, and the reason is what reads it.**
  # Postgres is provisioned there because Rust cells connect to it in that pass. Nothing in
  # this repository can carry a per-subject credential yet, so no cell reads this tier -
  # paying a JVM's start-up on every test pass for a server nothing connects to is the cost
  # `compose.services.yaml` declines for the same service on the same grounds. The
  # convergence is one line: when a cell needs a real issuer, this tier moves into
  # `nextest`'s `preCheck` beside Postgres and this check goes away.
  #
  # No network beyond loopback, no docker socket, no state outside the build directory.
  check = pkgs.runCommand "keycloak-tier"
    {
      nativeBuildInputs = [ tier endpoints.script pkgs.jq ];
    }
    ''
      tree="$NIX_BUILD_TOP/worktree"
      mkdir -p "$tree"
      cd "$tree"

      # The tier derives this itself; the check needs it to reach the JVM's own pid file, which is
      # the only thing that can tell a heal from a second server.
      kc_home="$NIX_BUILD_TOP/sutura-keycloak"

      # `status` answers by EXIT CODE and its middle answer is 3, so a bare `if` cannot see it: an
      # `if ... status` over 3 is false, exactly as a boolean caller should read it, which makes an
      # assertion written that way silently unable to fail for the reason it is here.
      tier_state() {
        state=0
        sutura-keycloak-tier status || state=$?
        printf '%s' "$state"
      }

      expect_state() {
        got="$(tier_state)"
        if [ "$got" != "$1" ]; then
          echo "status answered $got, expected $1 - $2" >&2
          exit 1
        fi
      }

      sutura-keycloak-tier start
      expect_state 0 "a server that is running, published and provisioned"

      # The discovery contract: a harness learns the port from this file and nowhere else,
      # so a tier that started and published nothing is a tier no test can reach.
      endpoints=.sutura-dev/endpoints.json
      test -f "$endpoints"
      # THE MARKER IS ON THE ENTRY - `github.com/telekom/sutura#317`. This asserted a
      # document-level `.provisioner`, which is the last writer's opinion about every other
      # writer's service and answers wrong for one of them the moment `xtask dev-up` merges
      # its own entries beside these. `dev/src/discovery.rs` reads it per service now.
      test "$(jq -r '.services.keycloak.provisioner' "$endpoints")" = nix
      port="$(jq -r '.services.keycloak.port' "$endpoints")"
      test "$port" -gt 0
      test "$(jq -r '.services.keycloak.host' "$endpoints")" = 127.0.0.1

      # Two subjects, because one is not the property `docs/adr/0008` draws.
      realm=.sutura-dev/keycloak-realm.json
      test "$(jq -r '.subjects | length' "$realm")" = 2
      test "$(jq -r '.issuer' "$realm")" = "http://127.0.0.1:$port/realms/${realm}"

      # --- a live server whose entry was dropped HEALS IN PLACE ---
      # `github.com/telekom/sutura#324`. The state is any publish this tier's entry did not
      # survive - a `start` that died between the bind and its publish, a hand-removed file, a
      # `stop` that failed: the JVM survives and its entry does not.
      # Two wrong answers are possible here and this tier gave the first one for as long as
      # `start`'s guard WAS `status` - print *already up* and return 0 having published nothing.
      # The second is what an endpoint-derived guard alone would have given: a cold start that
      # `rm -rf`s the home under a live JVM and leaves two servers on two OS-chosen ports sharing
      # one realm file. So the arm asserts the heal AND that nothing was restarted to get it.
      pid_before="$(cat "$kc_home/tier.pid")"
      secret_before="$(jq -r '.client.secret' "$realm")"
      sutura-tier-endpoint withdraw "$tree" keycloak
      expect_state 3 "a running server nothing publishes is unclaimed, not up"

      sutura-keycloak-tier start
      expect_state 0 "start republished the entry for the server that was already running"
      test "$(jq -r '.services.keycloak.port' "$endpoints")" = "$port"
      # THE SAME JVM. A cold start writes a new pid here, so this is the assertion that separates
      # a heal from the two-server outcome; the port above would be new with it.
      test "$(cat "$kc_home/tier.pid")" = "$pid_before"
      # AND NOTHING RE-PROVISIONED. The client secret is generated per `start`, so an unchanged one
      # is what says anything holding the old realm file does not have to re-read it.
      test "$(jq -r '.client.secret' "$realm")" = "$secret_before"

      # --- and the arm that CANNOT heal refuses, rather than reporting success ---
      # Every secret this tier has is generated at `start` and written to the realm file alone -
      # the server holds hashes and `endpoints.json` holds addresses by design. So with that file
      # gone there is no way back to a usable tier over THIS server, and an entry republished
      # anyway would name a live server nothing can authenticate against: the same shape of lie as
      # the no-op success above. Moved aside rather than deleted, because what follows still needs
      # a provisioned tier and a second cold start costs a JVM boot to assert nothing new.
      aside="$NIX_BUILD_TOP/realm-file-aside"
      mv "$realm" "$aside"
      expect_state 3 "a running server whose credentials are gone is not reachable either"
      echo "--- the refusal below is expected, its message included ---"
      refused=0
      sutura-keycloak-tier start || refused=$?
      if [ "$refused" = 0 ]; then
        echo "start reported success over a server whose realm file it cannot recover" >&2
        exit 1
      fi
      test ! -f "$realm"
      # It refused rather than taking the decision: killing the JVM invalidates every token
      # anything else obtained from this realm, and that is a person's call and not this script's.
      test "$(cat "$kc_home/tier.pid")" = "$pid_before"
      mv "$aside" "$realm"
      expect_state 0 "the tier reads as reachable again once its own credentials are back"

      # A SECOND TIER IN THE SAME FILE, which is the property `nix/tier-endpoints.nix`
      # exists for and which no other check can see: `checks.nextest` provisions Postgres
      # alone and this one provisions Keycloak alone, so the two-tier case only happens on
      # a developer's machine - where the old single-`printf` writer silently dropped the
      # first service's entry and discovery answered a truthful file about half a tier.
      # A neighbour is published by hand here rather than by starting a real server,
      # because what is under test is the writer and not the second service.
      sutura-tier-endpoint publish "$tree" postgres "$tree/.sutura-dev/pg" 5432
      test "$(jq -r '.services | length' "$endpoints")" = 2
      test "$(jq -r '.services.keycloak.port' "$endpoints")" = "$port"
      # And each entry still says who published it, which is what makes `stop` able to
      # withdraw its own and leave the neighbour's.
      test "$(jq -r '.services.postgres.provisioner' "$endpoints")" = nix

      # `stop` withdraws BOTH of ITS OWN claims and NEITHER of the neighbour's. A stale
      # endpoint is read as availability, which is how a fail-closed cell panics on a dead
      # server instead of skipping; a withdrawal that took the whole file with it is the
      # clobbering above, in the other direction.
      sutura-keycloak-tier stop
      test ! -f "$realm"
      test -f "$endpoints"
      test "$(jq -r '.services | has("keycloak")' "$endpoints")" = false
      test "$(jq -r '.services.postgres.port' "$endpoints")" = 5432
      expect_state 1 "a stopped tier: nothing is running here"

      # The last service out takes the file with it, because its EXISTENCE is what
      # discovery reads as "something is provisioned here".
      sutura-tier-endpoint withdraw "$tree" postgres
      test ! -f "$endpoints"

      touch $out
    '';
}
