# The Keycloak tier as ONE provisioner in two places: the nixpkgs `keycloak` package, started from
# the same script by a `nix run .#keycloak-acceptance` in CI and by `just keycloak-acceptance` in the
# dev shell.
#
# # Why a real issuer at all, when the sandbox already runs a mock
#
# `sutura_dev::issuer` (issue #193) publishes a freshly minted JWK and signs real JWTs, so leg 1 and
# the credential path are provable every run without a container. But the mock answers every question
# it was built to answer, by construction - and the two things it cannot answer are exactly the ones
# on the identity critical path that a real provider exists for:
#
#   * whether a real identity provider will mint an ID token whose `aud` is a THIRD party's client id
#     (`docs/adr/0014` Decision 3, issue #105's blocking unknown); and
#   * `docs/adr/0008`'s two-subject property - two subjects, differently granted at the source,
#     reading different rows - which needs tokens the validator accepts and the source maps to two
#     grants.
#
# This tier is a REAL Keycloak that the developer or CI provisions through the same discovery
# contract every other service uses. It stays off every default path, exactly like the compose
# `identity` profile it mirrors: nothing reads it yet, so booting it per pull request is cost with no
# coverage - `docs/adr/0016`'s "a service with no adapter to test is cost with no coverage", applied
# to an issuer. It is wired where a consumer asks for it, not where the gate happens to walk past.
#
# # The mechanism, and why the distribution has to be copied
#
# nixpkgs' `keycloak` package is pre-augmented by its own `kc.sh build` at package-build time, so its
# `lib` is read-only in the store. Keycloak's Quarkus launcher still rewrites
# `lib/quarkus/transformed-bytecode.jar` on every non-optimised start, and refuses to run `--optimized`
# on a first start with settings that differ from the bake-time ones. The `config_vars.patch` nixpkgs
# carries makes the home configurable through `KC_HOME_DIR`, so the tier copies the whole distribution
# into a writable home and points `KC_HOME_DIR` there - that is exactly what `services.keycloak`'s
# `preStart` does (`ln -s ${keycloakBuild}/lib /run/keycloak/` plus a writable data dir). Measured on
# 2026-09-03, aarch64-darwin: boot to a `200` in ~6s against the h2 dev-file store, no network.
#
# # Seeding, and the "no human" property
#
# The same writable home provides `kcadm.sh` (Keycloak's admin CLI). `start` signs into the `master`
# realm with the bootstrap admin user and, idempotently, creates one confidential client and two
# test users with fixed passwords - `docs/adr/0008`'s two-subject property needs two principals, not
# one. Nothing needs a person: a CI run provisions principals it can mint tokens for, and a laptop
# run does the same. The identifiers are fixtures, like the compose `x-fixture-credentials` block,
# not secrets - the tier binds loopback only.
#
# # The discovery file
#
# `start` writes `<cwd>/.sutura-dev/endpoints.json` naming the `keycloak` service, so
# `sutura_dev::provisioned::here` reads it unchanged - the HTTP loopback shape (`127.0.0.1:<port>`)
# that the discovery contract already admits alongside the postgres socket directory. `stop` removes
# the file, because a stale endpoint is a claim that a server is there when it is not (`nix/postgres-
# tier.nix` carries the measured version of that defect).
#
# The tier is two venues sharing one script: CI provisions it through `nix run .#keycloak-acceptance`
# (fail-closed via `SUTURA_DEV_REQUIRE_TIER`), the dev shell through the same script on the PATH.
{ pkgs }:
{
  package = pkgs.keycloak;

  tier = pkgs.writeShellApplication {
    name = "sutura-keycloak-tier";
    runtimeInputs = [ pkgs.keycloak pkgs.curl pkgs.python3 pkgs.lsof ];
    text = ''
      set -o errexit -o nounset

      root="$(pwd -P)"
      if [ -n "''${NIX_BUILD_TOP:-}" ]; then
        # In the sandbox the output dir is short by fiat; the private network namespace is ours, so
        # a fixed port cannot collide with anything.
        home="$NIX_BUILD_TOP/.sutura-dev/keycloak"
        port=18080
      else
        # A worktree can be deep and a JVM home is not sizeable, so the writable distribution lives
        # in a short per-worktree directory under TMPDIR, keyed by a hash of the worktree's physical
        # path - same shape as the postgres tier. The port is picked free: Keycloak is TCP-only, and
        # on an unsandboxed Mac a fixed port is a collision waiting to happen.
        key="$(printf '%s' "$root" | cksum | cut -d' ' -f1)"
        home="''${TMPDIR:-/tmp}/sutura-keycloak-$key"
        port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')"
      fi

      HOME_KC="$home"

      admin_user=sutura
      admin_password=sutura
      # **# Why `master`, and not a dedicated `test` realm - a measured decision, not a default.** A
      # realm created with `kcadm create realms -s realm=test` (or a minimal `--import-realm`) fails
      # the RO-PC grant: the direct-grant flow never resolves the user, the login record is
      # `userId="null"` with `error="resolve_required_actions", reason="Account is not fully set up"`,
      # while the SAME client and user created in the `master` realm mint a token. Measured 2026-09-03
      # on aarch64-darwin against the pinned package - `master` is the realm Keycloak certifies, so
      # seeding it is what makes the per-subject token reachable. A consumer reads the issuer at
      # `<endpoint>/realms/master`.
      realm=master
      client_id=test-client
      client_secret=sutura-client-secret
      # Two subjects, so `docs/adr/0008`'s two-subject property has principals to hang off.
      user_a=alice
      user_b=bob

      wait_ready() {
        local n=0
        while [ "$n" -lt 60 ]; do
          n=$((n + 1))
          if curl -fsS "http://127.0.0.1:$port/realms/master" >/dev/null 2>&1; then
            return 0
          fi
          sleep 2
        done
        echo "keycloak did not answer /realms/master on 127.0.0.1:$port" >&2
        return 1
      }

      kc() { "$HOME_KC/bin/kc.sh" "$@"; }
      kcadm() { "$HOME_KC/bin/kcadm.sh" "$@"; }

      provision() {
        export KC_HOME_DIR="$HOME_KC" KC_CONF_DIR="$HOME_KC/conf" \
          KC_BOOTSTRAP_ADMIN_USERNAME="$admin_user" KC_BOOTSTRAP_ADMIN_PASSWORD="$admin_password"
        # Idempotent against a persistent dev home; in the sandbox the home is fresh so every run is
        # a first boot. All sealed in `master` realm - see the comment on `realm`.
        kcadm create clients -r "$realm" -s clientId="$client_id" -s enabled=true \
          -s protocol=openid-connect -s 'redirectUris=["http://127.0.0.1/*"]' \
          -s 'directAccessGrantsEnabled=true' -s 'serviceAccountsEnabled=true' \
          -s "secret=$client_secret" \
          --server "http://127.0.0.1:$port" --realm master --user "$admin_user" --password "$admin_password" >/dev/null 2>&1 || true
        for user in "$user_a" "$user_b"; do
          if ! kcadm get "users?username=$user" -r "$realm" --server "http://127.0.0.1:$port" \
              --realm master --user "$admin_user" --password "$admin_password" 2>/dev/null | grep -q '"id"'; then
            kcadm create users -r "$realm" -s "username=$user" -s enabled=true \
              -s "email=$user@example.com" \
              --server "http://127.0.0.1:$port" --realm master --user "$admin_user" --password "$admin_password" >/dev/null
            kcadm set-password -r "$realm" --username "$user" --new-password "$user-pass" \
              --server "http://127.0.0.1:$port" --realm master --user "$admin_user" --password "$admin_password" >/dev/null
          fi
        done
      }

      start() {
        mkdir -p "$root/.sutura-dev"
        if [ ! -f "$HOME_KC/bin/kc.sh" ]; then
          mkdir -p "$HOME_KC"
          cp -a "${pkgs.keycloak}/." "$HOME_KC/"
          chmod -R u+w "$HOME_KC"
        fi
        export KC_HOME_DIR="$HOME_KC" KC_CONF_DIR="$HOME_KC/conf" \
          KC_BOOTSTRAP_ADMIN_USERNAME="$admin_user" KC_BOOTSTRAP_ADMIN_PASSWORD="$admin_password"
        if [ ! -f "$HOME_KC/keycloak.pid" ] || ! kill -0 "$(cat "$HOME_KC/keycloak.pid")" 2>/dev/null; then
          kc start --http-enabled=true --http-port "$port" --hostname-strict=false \
            >"$HOME_KC/keycloak.log" 2>&1 &
          echo $! > "$HOME_KC/keycloak.pid"
        fi
        wait_ready
        provision
        printf '{"project":"sutura","provisioner":"nix","services":{"keycloak":{"host":"127.0.0.1","port":%s}}}\n' \
          "$port" > "$root/.sutura-dev/endpoints.json"
      }

      stop() {
        if [ -f "$HOME_KC/keycloak.pid" ]; then
          kill "$(cat "$HOME_KC/keycloak.pid")" 2>/dev/null || true
          rm -f "$HOME_KC/keycloak.pid"
        fi
        # `kc.sh` is a wrapper that forks the JVM, so killing the recorded PID can orphan the java
        # child - which then holds the h2 lock and blocks the next `start` ("Database may be already
        # in use"). Kill by the data file: that is whatever actually owns our store, wrapper or java,
        # and a stop must leave a start able to boot. Measured 2026-09-03: an orphaned java under
        # init kept the store locked after `stop`.
        lsof -t "$HOME_KC/data/h2/keycloakdb.mv.db" 2>/dev/null | xargs -r kill 2>/dev/null || true
        rm -f "$root/.sutura-dev/endpoints.json" || true
      }

      status() {
        [ -f "$HOME_KC/keycloak.pid" ] && kill -0 "$(cat "$HOME_KC/keycloak.pid")" 2>/dev/null
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
