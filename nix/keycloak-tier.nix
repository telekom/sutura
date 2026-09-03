# The Keycloak tier as ONE provisioner in two places, on `nix/postgres-tier.nix`'s pattern:
# nixpkgs' `keycloak`, started from the same script by the `checks.nextest` sandbox and by the dev
# shell, and provisioned with NO HUMAN AT A KEYBOARD - a realm, two clients, an audience mapper and
# two subjects, all through `kcadm.sh`.
#
# # What this venue is for, and the one row it is the only venue for
#
# `docs/where-identity-is-proven.md` carries a row nothing could answer: **whether a real identity
# provider will mint an ID token whose `aud` is a third party's client id.** The mock issuer in
# `sutura_dev::issuer` answers it *yes* by construction - the audience is a parameter there - which
# that page calls worse than no test. The question is not academic: `docs/adr/0008` records, verified
# against the vendor's documentation, that a workforce-identity token exchange requires the caller's
# ID token to carry the PROVIDER's configured client id in `aud`, and that this is not the `audience`
# value sent to the exchange endpoint. Whether a provider will issue such a token is a property of
# the provider, so only a provider can answer it.
#
# So this tier's job is that one question, and `dev/tests/keycloak.rs` asks it. **What it may not be
# cited for** is written beside the claim, because a venue that cannot state its limit is how
# *verified* drifts:
#
#   * It is a real OIDC provider, not the enterprise deployment somebody will federate with. What it
#     shows is that the mapper exists, is configurable with no human in the loop, and produces the
#     token - never that a particular organisation's policy permits it.
#   * It says nothing about leg 2. `AGENTS.md` keeps the shipped position that no source a deployment
#     serves executes as the asking subject, and two real subjects holding two real tokens do not
#     move it: what is missing there is an adapter that can carry a per-subject credential.
#   * `docs/adr/0008`'s two-subject test asserts that two subjects READ TWO DIFFERENT ROW SETS. This
#     tier supplies two subjects whose tokens a validator accepts, which is the prerequisite and not
#     the test. The compose block for the same service already says why: the thing under test there
#     is the DATA SYSTEM validating a token it trusts, and this tier has no data system in it.
#
# # Why it can be a nix check at all, which Postgres made look easy and DataHub does not
#
# The build sandbox has no network and no docker socket. Postgres needs neither - a unix socket has
# no port. Keycloak DOES need a TCP port and it does need a database, and the two reasons it still
# fits are that it binds loopback only, and that its file-backed development store needs no second
# server. That is the whole difference from DataHub, which `compose.services.yaml` declares
# compose-only for reasons that are properties of the software rather than of this repository.
#
# # The port is DERIVED, and that is only safe because of the readiness check
#
# `sutura_dev::scope` refuses to derive a port and says why: a hash into a port range cannot promise
# disjoint blocks, and check-then-bind is a race that reads as a guarantee and delivers a
# probability. The asymmetry it draws is the one that matters - *a hash collision in a NAME is a
# startup error somebody reads; a hash collision in a PORT is a test that passes against the wrong
# fixture.*
#
# This tier derives a port and turns the second case back into the first, because it has something a
# docker publish does not: **a realm only this tier creates.** `is_up` does not ask whether the port
# is bound. It asks whether the server on it serves this realm AND reports that realm as its own
# issuer. A stranger holding the port fails that, so `start` goes on to bind, fails with the server's
# own message, and NOTHING is written to the discovery file: the claim *keycloak is at this address*
# is only ever made after a document from this realm came back.
#
# The residual gap, stated rather than smoothed over: two worktrees whose keys collided AND that both
# serve this realm would be indistinguishable by that document. `is_up` therefore also requires the
# pid this tier recorded to still be alive - one pid file per worktree, which is why the home lives
# under the worktree - and that is what makes "somebody else's server" answer `false` for a worktree
# that never started one. It is not a process identity, because a pid is reused, so the compound case
# (a reused pid here, plus a colliding key, plus the same realm there) is not reached by anything in
# this file. `stop` covers the other half of the same gap by asking the SERVER before withdrawing a
# claim it could not honour.
#
# # Why the server starts `--optimized` against a store-provided `lib`
#
# The nixpkgs package already ran `kc.sh build`, so the augmentation output is in the store. Starting
# in a mode that re-runs it would write into `$KC_HOME_DIR/lib/quarkus`, and that directory is a
# symlink into `/nix/store`. So the runtime layout is the one nixpkgs' own NixOS module uses: a
# writable home whose `lib`, `providers` and `themes` are symlinks into the package, with a writable
# `conf` and `data` beside them.
#
# **Measured, because the failure is silent otherwise:** `db` and `health-enabled` are BUILD-time
# options. Setting either in `conf/keycloak.conf` under `--optimized` prints *"the following build
# time options have values that differ from what is persisted"* and the server never comes up - no
# stack trace, no bind, nothing else in the log. Neither is set here, so what runs is what the
# package was built with.
{ pkgs }:
let
  endpoint = import ./tier-endpoint.nix { inherit pkgs; };

  # The realm, the clients and the subjects in ONE place. The script provisions them and writes them
  # into `.sutura-dev/keycloak.json` for a harness to read, so no test ever spells a client id.
  realm = "sutura-tier";
  gateway = "tier-gateway";
  thirdParty = "tier-third-party";

  # FIXTURE credentials, and the spelling is the point: the server binds loopback so nothing off this
  # host can reach it, its store is thrown away on the next `start`, and a value that reads as
  # generated is one nobody mistakes for a secret worth protecting. `just secrets` sweeps the tree.
  adminUser = "tier-admin";
  adminPassword = "not-a-secret-tier-admin";
  subjectPassword = "not-a-secret-tier-subject";
in
{
  package = pkgs.keycloak;

  # The provisioner, usable from any shell that has it. The dev shell gets it on PATH through
  # `devenv.nix`, the sandbox as a native input of `checks.nextest`.
  #
  # `start` brings the server up (or is a no-op where one is already serving this realm), provisions
  # it, and only then writes `<cwd>/.sutura-dev/endpoints.json` through `nix/tier-endpoint.nix` -
  # which MERGES, so the Postgres tier's entry survives. `stop` tears it down and WITHDRAWS the
  # claim, for the reason `nix/postgres-tier.nix` measured: discovery reads the file's existence as
  # availability, so a claim left behind turns an honest skip into a connection failure blamed on the
  # code under test. `status` answers without changing anything, so a wrapper tears down only what it
  # brought up - see `nix/with-tier.sh`.
  tier = pkgs.writeShellApplication {
    name = "sutura-keycloak-tier";
    runtimeInputs = [
      pkgs.keycloak
      pkgs.curl
      pkgs.jq
      endpoint
    ];
    text = ''
      set -o errexit -o nounset

      realm=${realm}
      gateway=${gateway}
      third_party=${thirdParty}
      admin_user=${adminUser}
      admin_password=${adminPassword}
      subject_password=${subjectPassword}
      # Two subjects, because one proves nothing about a subject reaching a token of its own.
      # `example.com` is the reserved documentation domain, so neither address can belong to anybody.
      subjects=(alice@example.com bob@example.com)

      root="$(pwd -P)"
      # The server's writable home lives UNDER THE WORKTREE, unlike `nix/postgres-tier.nix`'s, and
      # the difference is that a unix socket path caps around 100 bytes and an HTTP port does not.
      # That module has to put its data directory somewhere short and therefore outside the tree;
      # this one does not, and putting it in `$TMPDIR` instead cost a real hour.
      #
      # **Measured on 2026-09-03.** `$TMPDIR` is not the same value in every shell on macOS - a
      # devenv shell had `/tmp` while an interactive one had the per-user `/var/folders/...` path -
      # so the home, and with it the PID FILE, differed between two shells looking at ONE worktree.
      # `status` in the second shell then answered *not up* about a server the first had started,
      # and the `stop` that followed withdrew the endpoint claim from under a running suite, which
      # failed four minutes later as *`keycloak` was not provisioned*. One home per worktree makes
      # `status` and `stop` mean the same thing to every shell in it. `.sutura-dev/` is gitignored
      # and nothing removes it wholesale, which is what makes it a safe place to keep a server.
      home="$root/.sutura-dev/keycloak"
      # The PORT is keyed differently in the two venues: on `$NIX_BUILD_TOP`, which is fresh per
      # build, inside the sandbox, and on the worktree's physical path outside it.
      if [ -n "''${NIX_BUILD_TOP:-}" ]; then
        key="$(printf '%s' "$NIX_BUILD_TOP" | cksum | cut -d' ' -f1)"
      else
        key="$(printf '%s' "$root" | cksum | cut -d' ' -f1)"
      fi
      # The header says why deriving this is safe here and is not safe for a docker service.
      port=$(( 20000 + key % 20000 ))
      base="http://127.0.0.1:$port"
      issuer="$base/realms/$realm"
      pidfile="$home/keycloak.pid"

      export KC_HOME_DIR="$home"
      export KC_CONF_DIR="$home/conf"

      # The admin CLI, always against this tier's own config file. Without `--config` it writes
      # `$HOME/.keycloak`, which the sandbox may not have and which two worktrees would share.
      admin_cli() {
        kcadm.sh "$@" --config "$home/kcadm.json"
      }

      # Is the process this tier recorded still running?
      #
      # NOT a process identity - a pid is reused, and the header says what that leaves unreached.
      # What it buys is the case that matters: a worktree which never started a server has no pid
      # file, so it cannot mistake somebody else's Keycloak for its own.
      pid_alive() {
        [ -f "$pidfile" ] || return 1
        pid="$(cat "$pidfile")"
        [ -n "$pid" ] || return 1
        kill -0 "$pid" 2>/dev/null
      }

      # Does something on our port serve OUR realm, and name that realm as its own issuer?
      #
      # THE check that makes a derived port safe: a stranger on the port answers with a 404 or with
      # somebody else's issuer, so a collision becomes a loud failure to bind rather than a test
      # against the wrong fixture.
      serves_our_realm() {
        curl -fsS --max-time 10 "$issuer/.well-known/openid-configuration" 2>/dev/null \
          | jq -e --arg issuer "$issuer" '.issuer == $issuer' >/dev/null 2>&1
      }

      is_up() {
        pid_alive && serves_our_realm
      }

      # Stop what the pid file names, and wait for it to go.
      #
      # Shared by `stop` and by `start`, and `start` is the reason it is a function. A run whose trap
      # never fired - a killed `just test`, a closed terminal - leaves a JVM holding both the port and
      # an exclusive lock on the file-backed store, and the next `start` then dies with *The file is
      # locked* rather than coming up. **Measured on 2026-09-03**, from a `pkill` of a gate: the
      # orphan was still LISTENING on the derived port eleven minutes later. Reaping it is safe
      # because the pid file lives under the worktree and is written by this tier alone, so a live
      # process it names is ours.
      kill_owned() {
        pid_alive || return 0
        # TERM, not KILL: `kc.sh` traps it and forwards it to the JVM it is waiting on, so the server
        # shuts down rather than leaving a half-written store behind.
        kill "$(cat "$pidfile")" 2>/dev/null || true
        waited=0
        while pid_alive && [ "$waited" -lt 30 ]; do
          sleep 1
          waited=$(( waited + 1 ))
        done
      }

      # The names a harness needs, written by the thing that created them.
      #
      # A second file rather than more fields in `endpoints.json`, and the reason is what each one
      # is: an ADDRESS has exactly one door (`sutura_dev::provisioned`) because a memorised constant
      # would connect to a neighbour's fixture. A realm name cannot do that. What this removes is the
      # other drift - a test spelling a client id the script no longer creates.
      write_fixture() {
        jq -n \
          --arg realm "$realm" \
          --arg issuer "$issuer" \
          --arg gateway "$gateway" \
          --arg third_party "$third_party" \
          --arg password "$subject_password" \
          --args \
          '{ realm: $realm, issuer: $issuer, gateway_client: $gateway,
             third_party_client: $third_party, subject_password: $password,
             subjects: $ARGS.positional }' \
          "''${subjects[@]}" > "$root/.sutura-dev/keycloak.json"
      }

      provision() {
        admin_cli config credentials --server "$base" --realm master \
          --user "$admin_user" --password "$admin_password"
        admin_cli create realms -s realm="$realm" -s enabled=true

        # The THIRD PARTY. It exists only to be named as an audience: no flow is enabled on it and
        # nothing ever authenticates to it, which is the shape a workforce-identity provider's client
        # id has in `docs/adr/0008`.
        admin_cli create clients -r "$realm" -s clientId="$third_party" -s enabled=true \
          -s publicClient=false -s standardFlowEnabled=false -s serviceAccountsEnabled=false \
          -s directAccessGrantsEnabled=false

        # The GATEWAY: the client a subject authenticates to. Public, and only the direct access
        # grant is enabled, because a browser redirect needs a human and this tier may not.
        admin_cli create clients -r "$realm" -s clientId="$gateway" -s enabled=true \
          -s publicClient=true -s standardFlowEnabled=false -s serviceAccountsEnabled=false \
          -s directAccessGrantsEnabled=true
        gateway_id="$(admin_cli get clients -r "$realm" -q clientId="$gateway" \
          --fields id --format csv --noquotes)"

        # THE MAPPER THIS TIER EXISTS FOR. It puts the third party's client id into the ID token's
        # `aud`, beside the gateway's own - the token shape `docs/adr/0008` says a token exchange
        # requires, and the one thing no mock issuer may be cited for.
        admin_cli create "clients/$gateway_id/protocol-mappers/models" -r "$realm" \
          -s name=third-party-audience \
          -s protocol=openid-connect \
          -s protocolMapper=oidc-audience-mapper \
          -s "config.\"included.client.audience\"=$third_party" \
          -s 'config."id.token.claim"=true' \
          -s 'config."access.token.claim"=false'

        # `firstName` and `lastName` are NOT decoration: the realm's default user profile declares
        # both required, and a user missing either is created without complaint and then gets
        # `invalid_grant: Account is not fully set up` from the token endpoint. Measured 2026-09-03.
        for subject in "''${subjects[@]}"; do
          admin_cli create users -r "$realm" \
            -s "username=$subject" -s "email=$subject" \
            -s enabled=true -s emailVerified=true \
            -s firstName=Test -s lastName=Subject
          admin_cli set-password -r "$realm" --username "$subject" \
            --new-password "$subject_password"
        done
      }

      start() {
        # Idempotent, and the condition is `is_up` rather than a pid: a repeated `just test`, or an
        # interrupted run whose trap never fired, must not put a second server on the same port.
        if is_up; then
          return 0
        fi
        mkdir -p "$root/.sutura-dev"
        # An orphan from a run whose trap never fired holds the port AND the store's lock, so it has
        # to go before the store is removed - see `kill_owned` for the measurement.
        kill_owned
        # A fresh store every start. Reusing a file-backed database whose server is gone would mean
        # provisioning against unknown state, and 12 seconds of JVM is cheaper than that question.
        rm -rf "$home"
        mkdir -p "$home/conf" "$home/data"
        # The store supplies the jars, the themes and the providers; the home directory supplies
        # everything the server WRITES. The header says why `--optimized` needs exactly this layout.
        ln -s ${pkgs.keycloak}/lib "$home/lib"
        ln -s ${pkgs.keycloak}/providers "$home/providers"
        ln -s ${pkgs.keycloak}/themes "$home/themes"
        # LOOPBACK ONLY. `hostname-strict=false` because the address is derived per worktree and
        # there is no name to declare. No `db` and no `health-enabled`: both are build-time options
        # and setting either here stops the server coming up at all - see the header.
        cat > "$home/conf/keycloak.conf" <<EOC
      http-enabled=true
      http-host=127.0.0.1
      http-port=$port
      hostname-strict=false
      EOC
        KC_BOOTSTRAP_ADMIN_USERNAME="$admin_user" \
        KC_BOOTSTRAP_ADMIN_PASSWORD="$admin_password" \
          kc.sh start --optimized > "$home/server.log" 2>&1 &
        echo "$!" > "$pidfile"

        # A JVM and a schema migration. Measured at 11-12s on an aarch64-darwin dev machine; the
        # bound is generous because a cold runner is not that machine, and because what it has to
        # distinguish is "slower than expected" from "did not start at all" - which is why the loop
        # also gives up the moment the process is gone rather than waiting out the whole budget.
        waited=0
        until curl -fsS --max-time 5 "$base/realms/master/.well-known/openid-configuration" \
          >/dev/null 2>&1; do
          if ! pid_alive; then
            echo "keycloak exited before it served the master realm; its log follows:" >&2
            tail -40 "$home/server.log" >&2
            return 1
          fi
          if [ "$waited" -ge 180 ]; then
            echo "keycloak did not answer on $base within 180s; its log follows:" >&2
            tail -40 "$home/server.log" >&2
            return 1
          fi
          sleep 1
          waited=$(( waited + 1 ))
        done

        provision

        # The endpoint is published only after the provisioned realm answered for itself. Writing it
        # any earlier would be a claim about a server that might have no realm yet, and the whole
        # value of that file is that its existence means something.
        if ! serves_our_realm; then
          echo "keycloak is up and $issuer does not answer as its own issuer" >&2
          tail -40 "$home/server.log" >&2
          return 1
        fi
        write_fixture
        sutura-tier-endpoint set "$root" keycloak 127.0.0.1 "$port"
      }

      stop() {
        kill_owned
        rm -f "$pidfile"
        # THE CLAIM IS WITHDRAWN WHEN IT IS FALSE, and not merely when a stop was asked for.
        #
        # `nix/postgres-tier.nix` withdraws unconditionally, and that is right for it: it can always
        # stop what the endpoint names. This tier cannot, because the pid file is the only thing
        # tying it to a process - so a `stop` that killed nothing would otherwise erase a claim about
        # a server that is still answering, and a suite reading the file next would report
        # *`keycloak` was not provisioned* about a live provider. **Measured on 2026-09-03**, from
        # the `$TMPDIR` split the home comment above records: a `stop` in the shell with the other
        # `$TMPDIR` found no pid, killed nothing, withdrew the claim, and a nextest run four minutes
        # into its compile failed on three cells with the tier up the whole time.
        #
        # So the order is: stop what we own, then ask the SERVER whether the claim is still true.
        if serves_our_realm; then
          echo "keycloak still answers at $issuer - leaving the endpoint claim, which is true." >&2
          echo "  This shell did not start it: the pid file under $home names no live process." >&2
          return 0
        fi
        # `|| true` because a tier that was never started has nothing to withdraw, and that is not a
        # failure.
        sutura-tier-endpoint unset "$root" keycloak || true
        rm -f "$root/.sutura-dev/keycloak.json" || true
      }

      case "''${1:-}" in
        start) start ;;
        stop) stop ;;
        status) is_up ;;
        *) echo "usage: $0 start|stop|status" >&2; exit 2 ;;
      esac
    '';
  };
}
