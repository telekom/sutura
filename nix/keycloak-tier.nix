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
  # The `aud` a token from this client carries, and it is a FIXED, hardcoded audience mapper
  # rather than this realm's own default. Measured on 2026-09-14: with no mapper at all, a
  # password-grant token's `aud` is the literal string `account` - Keycloak's built-in "account"
  # client scope, present on every realm and not shaped like a resource identifier at all.
  # `sutura-config`'s `security.inbound.resource` refuses anything that is not an absolute
  # `https://` URI (the same rule an audience-confusion attack would need broken to matter), so a
  # deployment declaring `account` as its resource never starts. `RFC 2606`'s reserved
  # `.example.com` for the same reason this repository's own fixtures already use it (see
  # `crates/sutura-cli/tests/served/harness.rs`'s `RESOURCE`) - not a real host, and not a
  # secret, so unlike the realm's passwords it is a plain literal rather than generated per start.
  resourceAudience = "https://sutura-dev-cli.example.com";
  # Every scope `sutura_app::Capability::scope` licenses (`crates/sutura-app/src/capability.rs`),
  # DUPLICATED here rather than derived - this is nix, and nothing here reads Rust source. A real
  # token carries whatever ITS issuer granted, byte for byte, in `sutura_http::inbound::caller`'s
  # own `Scopes::parse` (RFC 6749's space-delimited string) - unlike `resourceAudience` this is not
  # a value this tier invents, it is a name this workspace already owns three of, so `provision`
  # creates one client scope PER NAME and attaches each as a DEFAULT client scope: Keycloak lists a
  # granted client scope's own NAME in the `scope` claim it mints, which is what makes this the
  # right mechanism rather than a hardcoded-claim mapper synthesizing a value nothing granted.
  capabilityScopes = [ "sutura:catalog.read" "sutura:metrics.ask" "sutura:sql.run" ];
  # A third-party audience (an OIDC workforce pool identity provider) for the
  # `#105` ID-token probe: this mapper puts it into the `aud` claim when the
  # password grant requests `scope=openid`. Not cited by any harness test -
  # only exercised by the probe script that answers the blocking unknown.
  # RFC 2606 `.example.com`, non-secret, not a real host.
  idTokenAudience = "https://workforce-pool.example.com";
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
      pkgs.jq
      pkgs.flock
      # For the self-signed certificate `start` generates below - see the comment there for why
      # the realm's own issuer has to be `https://`, not merely reachable.
      pkgs.openssl
      endpoints.script
    ];
    text = ''
      set -o errexit -o nounset

      root="$(pwd -P)"
      state="$root/.sutura-dev"
      realm=${realm}
      client=${client}
      resourceAudience=${resourceAudience}
      idTokenAudience=${idTokenAudience}
      capabilityScopes="${builtins.concatStringsSep " " capabilityScopes}"
      subjects="${builtins.concatStringsSep " " subjects}"

      # The server's own directory, UNDER THE WORKTREE - where the tree is the key and there is
      # nothing to derive, which is `github.com/telekom/sutura#405`'s first answer and was this
      # tier's second.
      #
      # **The dev-shell arm it replaces put the CALLER'S ENVIRONMENT in the tier's identity.**
      # `''${TMPDIR:-/tmp}/sutura-keycloak-$key` keys the home, and the pidfile is inside it, so
      # `running` - `start`'s guard - is keyed by `$TMPDIR` too. Measured for
      # `github.com/telekom/sutura#528` against a live JVM: same root, a different `TMPDIR`, and
      # `running` answers false over a server that is up. `start` then takes its cold path, `rm
      # -rf`s a home nothing is using, and leaves TWO JVMs on two OS-chosen ports sharing one realm
      # file - the outcome that guard exists to prevent, reached by a route nobody had stated.
      #
      # Keycloak needs no short path: it listens on TCP, and a unix socket's ~100-byte cap is the
      # whole reason `nix/postgres-tier.nix` still takes a machine-shared root. The sandbox arm goes
      # with it rather than being kept as a second spelling - `$NIX_BUILD_TOP/worktree` IS the root
      # there, so both venues now run one derivation and the venue that can be tested is the one a
      # developer gets. Nothing in this script reads `$TMPDIR` or `$NIX_BUILD_TOP` any more.
      home="$state/keycloak"
      pidfile="$home/tier.pid"
      log="$home/server.log"
      admincfg="$home/kcadm.json"
      realmfile="$state/keycloak-realm.json"
      # SIBLING of `home`, not inside it - `start`'s cold path `rm -rf`s `home`, and a lock path
      # that died with it would let a live holder's lock survive on a now-UNLINKED inode while a
      # fresh `running` check opens a brand-new one at the same path and finds it uncontested. One
      # stable path for this worktree's whole lifetime is what makes the lock mean anything.
      lockfile="$state/keycloak.lock"

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
      #
      # **THE PIDFILE ALONE IS NOT THE PROCESS** - `github.com/telekom/sutura#528` item 1a,
      # measured: the pidfile lost, the JVM still alive, and this function used to answer false
      # over a server that is up, so `start` took the cold path and produced the exact two-JVM
      # outcome the paragraph above says a guard exists to prevent. The pidfile is unrelated
      # state - a file `stop` reads to know WHAT to kill - and losing it says nothing about
      # whether the process is still there. `start`'s launch below now holds `flock` on
      # `$lockfile` for as long as it (or whatever it `exec`s into) lives - a KERNEL-HELD lock,
      # released the instant that process ends for any reason - so this asks the kernel whether
      # anyone holds it rather than trusting a file that can go missing out from under a live
      # server. A SHARED probe (`-s`) is what keeps this a question rather than a second
      # claimant: an exclusive probe would itself contend for the slot the real holder has, so two
      # concurrent probes racing each other - never mind the holder - would answer RUNNING off
      # each other's transient hold rather than off the server. `github.com/telekom/sutura#528`
      # item 1, measured: 40 concurrent probe pairs under `-x` all answered RUNNING; `-s` gives 0.
      # THREE outcomes, not two - `nix/postgres-tier.nix`'s `alive()`, ported: 0 running, 1
      # PROVABLY not (there is nothing to probe: `$state` itself was never created, so no run
      # ever touched this lock), 2 UNKNOWN (the lock's own open failed for any other reason - a
      # permissions problem, a full filesystem, an fd limit - which proves nothing about the
      # JVM). `github.com/telekom/sutura#809`: the shape this replaced collapsed every open
      # failure into "not running", and since #803 that verdict gates `stop`'s `rm -rf`. Callers
      # MUST treat 2 like 0 and refuse, the same rule `alive`'s own callers hold.
      running() {
        # The group redirect is load-bearing, not style: `2>/dev/null` after a bare `exec` only
        # attaches once the FIRST redirect (opening `$lockfile`) has already succeeded.
        [ -d "$state" ] || return 1
        { exec {running_fd}>"$lockfile"; } 2>/dev/null || return 2
        if flock -n -s "$running_fd" 2>/dev/null; then
          flock -u "$running_fd"
          exec {running_fd}>&-
          return 1
        fi
        exec {running_fd}>&-
        return 0
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
        port="$(published_port)" || return 3
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

      # The SAME log line's other half - Quarkus prints both ports together, `Listening on:
      # http://127.0.0.1:<p> and https://127.0.0.1:<p2>` - so this is `published_port`'s own
      # pattern over the second clause. `provision` is the only reader: the realm's issuer and
      # every request `start` and `provision` make of the admin API go over THIS port, never the
      # http one - see the `resourceAudience` comment above for why a plaintext issuer is refused
      # outright rather than merely inconvenient. `endpoints.json` still publishes the http port,
      # unchanged: that file is a generic reachability address for `sutura_dev::discovery` and
      # nothing here reads an OIDC document off it.
      published_https_port() {
        [ -f "$log" ] || return 0
        sed -n 's|.*Listening on: .*https://127\.0\.0\.1:\([0-9]*\).*|\1|p' "$log" | tail -1
      }

      provision() {
        port="$1"
        https_port="$(published_https_port)"
        if [ -z "$https_port" ]; then
          echo "keycloak tier: the server's log names no https port - see start's own https-*" >&2
          echo "               options and the certificate this function generates for them" >&2
          tail -20 "$log" >&2
          exit 1
        fi
        base="https://127.0.0.1:$https_port"
        client_secret="$(generated)"

        # The admin console is answerable a moment after the port is, so the login is retried
        # rather than assumed. Everything after it is a hard failure: a realm that did not get
        # created is not something to continue past.
        for _ in $(seq 1 30); do
          if kcadm.sh config credentials --config "$admincfg" --server "$base" \
            --realm master --user "$admin_user" --password "$admin_password" \
            "''${kcadm_trust[@]}" >/dev/null 2>&1; then
            break
          fi
          sleep 2
        done

        kcadm.sh create realms --config "$admincfg" -s realm="$realm" -s enabled=true \
          "''${kcadm_trust[@]}" >/dev/null
        # Confidential, with the direct access grant: that is the flow a test uses to obtain a
        # token for a named subject without a browser. `standardFlowEnabled=false` because nothing
        # here redirects, and a client offering a flow nobody uses is surface for free.
        # The `protocolMappers` entry is the fix for the gap `resourceAudience`'s own comment
        # states: with no mapper, a token's `aud` is the realm-wide built-in `account` client
        # scope, not a resource identifier `security.inbound.resource` will accept. A hardcoded
        # `oidc-audience-mapper`, added at creation rather than as a second `kcadm.sh update`
        # call, puts the fixed value on every access token this client's grants mint.
        client_uuid="$(kcadm.sh create clients --config "$admincfg" -r "$realm" \
          -s clientId="$client" -s enabled=true -s publicClient=false \
          -s directAccessGrantsEnabled=true -s standardFlowEnabled=false \
          -s secret="$client_secret" \
          -s 'protocolMappers=[{"name":"resource-audience","protocol":"openid-connect","protocolMapper":"oidc-audience-mapper","consentRequired":false,"config":{"included.custom.audience":"'"$resourceAudience"'","id.token.claim":"false","access.token.claim":"true","introspection.token.claim":"true"}},{"name":"id-token-audience","protocol":"openid-connect","protocolMapper":"oidc-audience-mapper","consentRequired":false,"config":{"included.custom.audience":"'"$idTokenAudience"'","id.token.claim":"true","access.token.claim":"false","introspection.token.claim":"false"}}]' \
          "''${kcadm_trust[@]}" -i)"

        # One client scope PER CAPABILITY, DEFAULT (auto-granted, no consent screen) - see
        # `capabilityScopes`'s own comment above for why this is a client scope per name rather
        # than a mapper synthesizing the `scope` claim's value.
        for capability_scope in $capabilityScopes; do
          scope_uuid="$(kcadm.sh create client-scopes --config "$admincfg" -r "$realm" \
            -s name="$capability_scope" -s protocol=openid-connect \
            -s 'attributes={"include.in.token.scope":"true","display.on.consent.screen":"false"}' \
            "''${kcadm_trust[@]}" -i)"
          kcadm.sh update "clients/$client_uuid/default-client-scopes/$scope_uuid" \
            --config "$admincfg" -r "$realm" "''${kcadm_trust[@]}"
        done

        # `requiredActions=[]` and a complete profile are load-bearing, not decoration. Measured on
        # 2026-09-03: a user created with a username alone is refused at the token endpoint with
        # `invalid_grant: Account is not fully set up`, because the realm's default profile action
        # is still pending - a human at a browser is exactly what would have cleared it, which is
        # the one thing this tier may not need.
        for subject in $subjects; do
          kcadm.sh create users --config "$admincfg" -r "$realm" \
            -s username="$subject" -s enabled=true -s emailVerified=true \
            -s email="$subject@example.com" -s firstName="$subject" -s lastName=fixture \
            -s 'requiredActions=[]' "''${kcadm_trust[@]}" >/dev/null
          kcadm.sh set-password --config "$admincfg" -r "$realm" \
            --username "$subject" --new-password "$(subject_password "$subject")" \
            "''${kcadm_trust[@]}" >/dev/null
        done

        # PROVE IT, before reporting success. A tier whose realm came up half-provisioned must fail
        # here, where the message is about provisioning, rather than in whatever reads it next.
        for subject in $subjects; do
          token="$(curl -sS --max-time 20 --cacert "$cacertfile" -X POST \
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
            # The CA `start` generated for THIS `https://` issuer above - NOT the leaf `kc.sh`
            # serves, `$certfile`, which a client cannot trust directly (see the `CaUsedAsEndEntity`
            # comment above `cacertfile`'s own declaration). Not a secret (a certificate is the
            # public half), published anyway because it is the one fact a reader needs to trust
            # this loopback server at all: no public CA signed the leaf, and a client that does not
            # load this CA explicitly gets `certificate_unknown` against a real provider's real
            # TLS, the same failure a network attacker's own certificate would produce. An absolute
            # path, like every other file this script writes beside it.
            printf '"tls_certificate_file":"%s",' "$cacertfile"
            printf '"realm":"%s","client":{"id":"%s","secret":"%s"},' \
              "$realm" "$client" "$client_secret"
            # The third-party audience the `id-token-audience` mapper puts in the ID token's `aud`
            # when the password grant requests `scope=openid` (`nix/keycloak-tier.nix`'s own
            # `idTokenAudience`). Published here rather than restated in the test, so the value the
            # tier provisions and the value the cell asserts against are the same document.
            printf '"id_token_audience":"%s",' "$idTokenAudience"
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
        endpointsfile="$state/endpoints.json"
        if [ -f "$endpointsfile" ] && ! jq --exit-status \
          '.services.keycloak == null or .services.keycloak.provisioner == "nix"' \
          "$endpointsfile" >/dev/null 2>&1; then
          echo "keycloak tier: refusing to replace a keycloak entry that this nix tier does" >&2
          echo "               not own. Stop its provisioner or remove its stale claim first." >&2
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
        # `elif [ $? -eq 2 ]`, not `running; rc=$?` - a bare `running` outside a conditional
        # would trip `errexit` on its own `return 1`. `nix/postgres-tier.nix`'s `stop` holds
        # the same shape for `alive`'s three outcomes.
        if running; then
          if status; then
            echo "keycloak tier: already up - leaving it to whoever started it."
            return 0
          fi
          republish
          return 0
        elif [ $? -eq 2 ]; then
          # #809: `running` could not open `$lockfile` to ask the kernel, so alive or dead
          # cannot be proven - refusing beats clearing `$home` out from under a server this
          # cannot rule out.
          echo "keycloak tier: $lockfile could not be opened to check for a live JVM, so" >&2
          echo "               alive or dead cannot be proven. $home stays - fix whatever" >&2
          echo "               blocked the open (permissions, disk space, an fd limit) and retry." >&2
          exit 1
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

        # A throwaway CA and a leaf it signs, fresh per start like every other credential here.
        # NOT for confidentiality - loopback traffic in this sandbox is not the threat this
        # defends - but because `security.inbound.authorization_server`
        # (`crates/sutura-config/src/inbound/primitive.rs`) refuses anything that is not
        # `https://`, unconditionally, and a served deployment declaring this realm's OWN issuer
        # has to name it byte-for-byte. Measured on 2026-09-14: the same realm behind
        # `--http-enabled=true` alone mints tokens whose `iss` is `http://...`, which is not a
        # spelling that refusal accepts - so the tier has to actually SERVE https, not merely
        # claim to, for a real deployment to ever declare it.
        #
        # **TWO certificates, not one, and that is measured rather than simpler-looking.** A
        # single self-signed cert handed to `kc.sh` AND trusted directly as a root fails every
        # TLS client here with `CaUsedAsEndEntity`: X.509 path validation refuses a certificate
        # that is simultaneously the trusted root and the leaf a server presents, because a root
        # (`CA:true`) is not a valid end-entity certificate. `cacertfile`/`cakeyfile` are a
        # `CA:true` root that never leaves this script; `certfile`/`keyfile` are a `CA:false` leaf
        # it signs, and `kc.sh` gets only the leaf. A client trusting `cacertfile` alone still
        # verifies `certfile` correctly - that one edge, root-signs-leaf, is a valid chain.
        cacertfile="$home/keycloak-ca-cert.pem"
        cakeyfile="$home/keycloak-ca-key.pem"
        certfile="$home/keycloak-cert.pem"
        keyfile="$home/keycloak-key.pem"
        csrfile="$home/keycloak-cert.csr"
        leafextfile="$home/keycloak-cert.ext"
        truststorefile="$home/keycloak-truststore.p12"
        trustpass="$(generated)"
        openssl req -x509 -newkey rsa:2048 -nodes -keyout "$cakeyfile" -out "$cacertfile" -days 1 \
          -subj "/CN=sutura-keycloak-tier-ca" \
          -addext "basicConstraints=critical,CA:true" -addext "keyUsage=critical,keyCertSign,cRLSign" \
          >/dev/null 2>&1
        openssl req -newkey rsa:2048 -nodes -keyout "$keyfile" -out "$csrfile" \
          -subj "/CN=127.0.0.1" >/dev/null 2>&1
        printf 'subjectAltName=IP:127.0.0.1\nbasicConstraints=CA:false\nkeyUsage=digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n' \
          > "$leafextfile"
        openssl x509 -req -in "$csrfile" -CA "$cacertfile" -CAkey "$cakeyfile" -CAcreateserial \
          -out "$certfile" -days 1 -extfile "$leafextfile" >/dev/null 2>&1
        # This script's own trust of the CA above, built with `openssl pkcs12` rather than
        # `keytool` so this stays one dependency rather than two - `kcadm.sh --truststore` reads
        # PKCS12, not a bare PEM.
        openssl pkcs12 -export -nokeys -in "$cacertfile" -out "$truststorefile" \
          -passout "pass:$trustpass" >/dev/null 2>&1
        # One array, not a flag pair repeated at every `kcadm.sh` call site below: `provision`'s
        # admin session is over `$base`, which is `https://...` now, and `kcadm.sh` trusts nothing
        # by default - the CA this SAME script minted a moment ago is exactly what an unconfigured
        # JVM refuses.
        kcadm_trust=(--truststore "$truststorefile" --trustpass "$trustpass")

        # `set -m` puts the launch in its own process group; `stop` kills the group, because
        # `kc.sh` spawns the JVM rather than replacing itself with it.
        #
        # The subshell takes `$lockfile` FIRST and only then `exec`s into `kc.sh` - replacing the
        # subshell's own process image, which is why `$!` below still names the right pid, but
        # KEEPING the open, locked file descriptor: `exec` does not close a descriptor that was
        # not marked close-on-exec. So the process `running` above checks against is holding the
        # lock for as long as it lives, pidfile or no pidfile.
        set -m
        ( exec {start_fd}>"$lockfile"
          flock -x "$start_fd"
          exec kc.sh start --optimized --cache=local --http-enabled=true --hostname-strict=false \
            --http-host=127.0.0.1 --http-port=0 --http-management-port=0 \
            --https-certificate-file="$certfile" --https-certificate-key-file="$keyfile" \
            --https-port=0
        ) >"$log" 2>&1 &
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
        signalled=0
        if [ -f "$pidfile" ]; then
          signalled=1
          pid="$(cat "$pidfile")"
          kill -TERM -- "-$pid" 2>/dev/null || true
          for _ in $(seq 1 20); do
            kill -0 -- "-$pid" 2>/dev/null || break
            sleep 1
          done
          kill -KILL -- "-$pid" 2>/dev/null || true
          rm -f "$pidfile"
        fi
        # A STOP THAT CANNOT STOP DOES NOT WITHDRAW OR DELETE - `nix/postgres-tier.nix`'s `stop`
        # holds the same rule over `pg_ctl status`. `running` is `start`'s OWN pidfile-independent
        # guard (`github.com/telekom/sutura#528` item 1a: the pidfile lost, the JVM still alive,
        # answered by a `flock -s` probe on `$lockfile` rather than by this file). Reusing it here
        # closes the asymmetry `github.com/telekom/sutura#724` names: without this check, a heal
        # that lost only the pidfile skips the kill above entirely and falls straight through to
        # withdraw the claim and delete the realm file and the home out from under a JVM this
        # function never signalled.
        # `if running; then ... elif [ $? -eq 2 ]; then ...` - not `running; rc=$?`, which
        # would trip `errexit` on `running`'s own bare `return 1`. `start`'s guard holds the
        # same shape, ported from `nix/postgres-tier.nix`'s `stop`.
        refuse=alive
        if running; then
          :
        elif [ $? -eq 2 ]; then
          refuse=unknown
        else
          refuse=
        fi
        if [ -n "$refuse" ]; then
          if [ "$refuse" = unknown ]; then
            # #809: `running`'s own lock probe could not open $lockfile, an outcome the two-
            # valued shape used to read as "not running" and this refuses instead - alive
            # or dead is unproven, and treating unknown like alive is the rule to hold, not
            # the rare case to special-case away.
            echo "keycloak tier: $lockfile could not be opened to check for a live JVM, so" >&2
            echo "               alive or dead cannot be proven. Its endpoint entry, realm" >&2
            echo "               file and home STAY - fix whatever blocked the open" >&2
            echo "               (permissions, disk space, an fd limit) and retry \`stop\`." >&2
          elif [ "$signalled" -eq 1 ]; then
            echo "keycloak tier: sent TERM and KILL to pid $pid but a server still holds" >&2
            echo "               $lockfile. Its endpoint entry, realm file and home STAY -" >&2
            echo "               deleting them under a live JVM is worse than the stale" >&2
            echo "               pidfile this guard exists to catch. Find and kill the" >&2
            echo "               process by hand, then retry \`stop\`." >&2
          else
            echo "keycloak tier: $pidfile is gone but a server still holds $lockfile, so a" >&2
            echo "               JVM is running here with no pid this script can signal." >&2
            echo "               Its endpoint entry, realm file and home STAY - withdrawing" >&2
            echo "               or deleting them out from under a live server is the" >&2
            echo "               defect \`github.com/telekom/sutura#724\` names. Find the" >&2
            echo "               process holding $lockfile by hand, kill it, then retry" >&2
            echo "               \`stop\`." >&2
          fi
          exit 1
        fi
        # Withdraw the claim - both halves of it. A stale endpoint makes a fail-closed cell panic
        # on a dead server where the honest outcome is a skip, and a stale realm file hands out
        # credentials for a realm that is gone.
        sutura-tier-endpoint withdraw "$root" keycloak
        rm -f "$realmfile"
        # AND THE SERVER'S OWN DIRECTORY, which lives under the worktree now: an embedded store is
        # provisioned on demand, nothing in it is meant to outlive a teardown, and `start`'s cold
        # path would `rm -rf` it on the way up anyway. `nix/postgres-tier.nix`'s `stop` takes its
        # data directory for the same reason, measured there at 40 MB a run left behind.
        rm -rf "''${home:?the tier home is unset}"
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
      # the only thing that can tell a heal from a second server. Under the worktree, and the
      # sandbox reads the SAME derivation a dev shell does now - `github.com/telekom/sutura#528`
      # records that the `$TMPDIR` arm this replaces went unexercised here for exactly that reason.
      kc_home="$tree/.sutura-dev/keycloak"

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

      usage_state=0
      sutura-keycloak-tier >/dev/null 2>&1 || usage_state=$?
      if [ "$usage_state" != 2 ]; then
        echo "usage answered $usage_state, expected 2" >&2
        exit 1
      fi

      sutura-keycloak-tier start
      expect_state 0 "a server that is running, published and provisioned"

      chmod u-r "$kc_home/server.log"
      unreadable_state="$(tier_state)"
      chmod u+r "$kc_home/server.log"
      if [ "$unreadable_state" != 3 ]; then
        echo "status answered $unreadable_state, expected 3 - an unreadable live server log is unclaimed, not usage" >&2
        exit 1
      fi
      expect_state 0 "the readable live server returns to the published state"

      # --- THE TIER'S IDENTITY IS THE WORKTREE, AND NOT THE CALLER'S ENVIRONMENT ---
      # `github.com/telekom/sutura#528`. The home was `''${TMPDIR:-/tmp}/sutura-keycloak-$key` in a
      # dev shell, so a caller whose `TMPDIR` differed looked for the pidfile elsewhere, `running`
      # answered false over this live JVM, and `start`'s cold path left two servers sharing one
      # realm file. `NIX_BUILD_TOP` is unset in here as well as `TMPDIR` being moved, and both are
      # load-bearing: with it set, the arm this replaced took the sandbox branch and the assertion
      # could not fail for the reason it is written.
      ( elsewhere="$NIX_BUILD_TOP/another-tmpdir"
        mkdir -p "$elsewhere"
        unset NIX_BUILD_TOP
        export TMPDIR="$elsewhere"
        expect_state 0 "a live tier is found by a caller whose TMPDIR and NIX_BUILD_TOP are not the ones that started it"
      )

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
      # The issuer is the realm's OWN `https://` loopback URL off the port the OS chose for the
      # https listener - the same line `provision` reads (`published_https_port`), because the
      # http port `endpoints.json` publishes and this https port are different OS choices and the
      # realm file says nothing about the http one. `endpoints.json`'s own port is asserted above
      # it is positive and loopback; it is the reachability address, not the issuer spelling.
      https_port="$(sed -n 's|.*Listening on: .*https://127\.0\.0\.1:\([0-9]*\).*|\1|p' "$kc_home/server.log" | tail -1)"
      test -n "$https_port"
      test "$(jq -r '.issuer' "$realm")" = "https://127.0.0.1:$https_port/realms/${realm}"

      # --- A LOST PIDFILE ALONE DOES NOT PRODUCE A SECOND JVM ---
      # `github.com/telekom/sutura#528` item 1a: the old guard was `[ -f "$pidfile" ]` first, so
      # losing JUST the pidfile - independent of `TMPDIR`, independent of `endpoints.json` -
      # answered false over a live JVM, and `start` took its cold path: `rm -rf`s the home a live
      # server is using and launches a second one on a second OS-chosen port, sharing one realm
      # file. Reproduced directly: the pidfile is the ONLY thing removed here.
      lost_pid="$(cat "$kc_home/tier.pid")"
      rm -f "$kc_home/tier.pid"
      expect_state 0 "a live tier is found by status even with its pidfile gone"
      sutura-keycloak-tier start
      if [ -f "$kc_home/tier.pid" ] && [ "$(cat "$kc_home/tier.pid")" != "$lost_pid" ]; then
        echo "start launched a SECOND JVM: the pid changed after losing only the pidfile" >&2
        exit 1
      fi
      expect_state 0 "still one server, one claim, after the lost pidfile is healed"
      # NEITHER of `start`'s healthy-path branches rewrites the pidfile - it is `stop`'s
      # bookkeeping, not `running`'s any more - so it stays gone after a heal like this one.
      # Restored here for the assertions below, which read it directly; that gap is real and
      # stated rather than fixed, and it is WORSE than "cannot signal the JVM": `stop`'s kill is
      # guarded by `[ -f "$pidfile" ]`, but its withdraw / `rm "$realmfile"` / `rm -rf "$home"`
      # below are NOT - so a `stop` reaching this worktree after such a heal exits 0 having
      # withdrawn the claim and deleted the realm file and home out from under a JVM it never
      # touched, and every later `start` that then fails names this same `stop` as its remedy,
      # which cannot perform it.
      if [ ! -f "$kc_home/tier.pid" ]; then
        echo "$lost_pid" > "$kc_home/tier.pid"
      fi

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

      # A mismatched entry from ANOTHER provisioner is not this tier's stale claim to heal. The
      # service key is shared with the docker identity profile, so replacing that entry crosses
      # the provisioner boundary and makes a later docker teardown leave this JVM published.
      foreign_port=$((port + 1))
      jq --argjson port "$foreign_port" \
        '.services.keycloak = { host: "127.0.0.1", port: $port, provisioner: "docker" }' \
        "$endpoints" > "$endpoints.new"
      mv "$endpoints.new" "$endpoints"
      expect_state 3 "another provisioner's entry does not describe this tier"

      echo "--- the foreign-claim refusal below is expected, its message included ---"
      refused=0
      sutura-keycloak-tier start || refused=$?
      if [ "$refused" = 0 ]; then
        echo "start replaced another provisioner's keycloak entry" >&2
        exit 1
      fi
      test "$(jq -r '.services.keycloak.provisioner' "$endpoints")" = docker
      test "$(jq -r '.services.keycloak.port' "$endpoints")" = "$foreign_port"

      # Restore this tier's own claim so the remaining state transitions still start from the
      # live JVM and the records it created.
      sutura-tier-endpoint publish "$tree" keycloak 127.0.0.1 "$port"
      expect_state 0 "the tier reads as reachable after its own claim is restored"

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

      # --- STOP REFUSES RATHER THAN DELETING A LIVE JVM'S STATE WITH NO PIDFILE ---
      # `github.com/telekom/sutura#724`, and NOT restored afterward this time - the earlier heal
      # above restores the pidfile because the assertions after it read the file directly, which
      # is exactly why this gap went uncaught: the one place that reproduces the precondition also
      # erased it before `stop` ever ran. Here it does not: `stop`'s destructive half is guarded by
      # `running`, `start`'s own pidfile-independent check, so losing only the pidfile must make
      # `stop` refuse rather than withdraw the claim and delete the realm file and home out from
      # under a JVM it never signalled.
      rm -f "$kc_home/tier.pid"
      echo "--- the refusal below is expected, its message included ---"
      refused=0
      sutura-keycloak-tier stop || refused=$?
      if [ "$refused" = 0 ]; then
        echo "stop reported success without ever signalling the live JVM" >&2
        exit 1
      fi
      kill -0 "$pid_before" 2>/dev/null || {
        echo "the JVM stop could not signal is gone anyway - refusal proved nothing" >&2
        exit 1
      }
      test -f "$realm"
      test -e "$kc_home"
      test "$(jq -r '.services.keycloak.port' "$endpoints")" = "$port"
      echo "$pid_before" > "$kc_home/tier.pid"
      expect_state 0 "the live server, its realm file and its home all survived the refused stop"

      # --- STOP REFUSES WHEN THE LOCK PROBE ITSELF CANNOT BE OPENED (#809) ---
      # `running`'s three outcomes collapse the same way `alive`'s do in `nix/postgres-tier.nix`:
      # an open failure that is NOT "nothing to probe" (`$state` missing) is UNKNOWN, and unknown
      # must refuse like alive rather than read as dead. The pidfile is removed FIRST - `stop`
      # signals whatever it names before it ever asks `running`, so a present pidfile would kill
      # the JVM through that path regardless of the lock probe below and prove nothing about it.
      # Denying write on the lock file itself - `$state` still exists, `chmod u-r` on
      # `server.log` above already proved this build is not running as a user permission bits do
      # not bind - leaves the JVM genuinely alive and unreachable by `running` for a reason that
      # says nothing about it.
      rm -f "$kc_home/tier.pid"
      lockfile="$tree/.sutura-dev/keycloak.lock"
      chmod u-w "$lockfile"
      echo "--- the refusal below is expected, its message included ---"
      refused=0
      sutura-keycloak-tier stop || refused=$?
      chmod u+w "$lockfile"
      if [ "$refused" = 0 ]; then
        echo "stop reported success while its own lock probe could not be opened" >&2
        exit 1
      fi
      kill -0 "$pid_before" 2>/dev/null || {
        echo "the JVM stop could not signal is gone anyway - refusal proved nothing" >&2
        exit 1
      }
      test -f "$realm"
      test -e "$kc_home"
      echo "$pid_before" > "$kc_home/tier.pid"
      expect_state 0 "the live server survives a stop whose own lock probe could not be opened"

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
      # AND THE HOME GOES WITH THE SERVER. It sits under the worktree now, so a home left behind is
      # an embedded store accumulating in a developer's checkout rather than in a directory the
      # operating system eventually reclaims - `nix/postgres-tier.nix` measured that at nine
      # directories of 40 MB from four separate days.
      test ! -e "$kc_home"
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
