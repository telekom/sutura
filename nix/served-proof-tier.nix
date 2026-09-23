# A SECOND, single-purpose Keycloak tier - not `nix/keycloak-tier.nix` with a flag, because that
# tier's realm signs with whatever RSA key Keycloak generates for itself at `start`, and this one
# has to sign with a FIXED key an operator uploaded to a Google workload-identity pool provider as
# `jwks_json` (`test-infra/pulumi/google/__main__.py`'s "served-caller proof" block). Two different
# invariants over the same server, so this is its own realm and its own script rather than a branch
# in the one that already exists.
#
# # What this proves, and the one thing it cannot
#
# `docs/where-identity-is-proven.md`'s "a served binary under a verified human caller" row: a
# caller's own token has to satisfy BOTH this deployment's capability gate (needs a `scope` claim)
# and the identity pool's provider (needs a signature Google can verify) at once. A Google ID
# token has no scope; the DEV Keycloak tier's tokens carry scope but sign with a key on loopback
# Google cannot fetch. This tier's realm signs with a key the pool was given directly, so the same
# token satisfies both - see `test-infra/pulumi/google/__main__.py`'s own header for the upload
# mechanism and its citation.
#
# What it cannot prove by itself: that Google's STS actually accepts a token this realm mints. That
# needs the pool provisioned with this realm's own public key, which is the hosted half - this tier
# only has to mint a token that WOULD satisfy it, and the CI job that runs against the real pool is
# what turns "wired" into "yes".
#
# # Why this is not `nix/keycloak-tier.nix` plus a parameter, in the ways that matter
#
# **No TLS.** The dev tier's issuer IS the address a caller reaches it at, so that address has to
# be `https://` for `security.inbound.authorization_server`'s own refusal
# (`crates/sutura-config/src/inbound/primitive.rs`) to accept it - hence the self-signed CA dance.
# This tier's issuer is a FIXED, DELIBERATELY UNREACHABLE literal (below), decoupled from wherever
# it actually listens by Keycloak's own `--hostname` override - confirmed live: a realm started
# with `--hostname=https://served-proof.sutura-ci.example.com` over a PLAIN `--http-port=0`
# listener mints a token whose `iss` is that literal, byte for byte, no TLS anywhere. So the
# harness that mints a token talks plain loopback HTTP, and `security.inbound.authorization_server`
# is still satisfied because that setting is a string compared against the token's `iss`, never a
# URL this process dials.
#
# **No interactive-safety mechanism.** `keycloak-tier.nix`'s `flock`-based `running()`, its
# heal-in-place `republish`, and its foreign-provisioner refusal all exist because a developer's
# shell can lose track of a server across `TMPDIR`s or leave one running past a crashed `stop` -
# `github.com/telekom/sutura#528`, `#324`, `#724`. This tier has exactly one caller across its
# whole lifetime: the CI recipe that starts it, runs one cell, and stops it in the same script. A
# plain pidfile plus `kill -0` is the ceiling that matches that use, stated so a future interactive
# use of this tier ports the guard rather than reinventing it under load.
#
# # The fixed key
#
# `SUTURA_SERVED_PROOF_SIGNING_KEY_FILE` names a PEM RSA private key `start` reads and hands to a
# realm-level `rsa` key provider component with an EXPLICIT `kid` config value
# (`org.keycloak.keys.AbstractRsaKeyProvider` reads `Attributes.KID_KEY` before falling back to a
# hash of the public key) - so the kid this realm publishes is the literal below, not something
# computed from the key and guessed at from outside. Verified live against Keycloak 26: a component
# created with `priority: 200` (above the realm's own default-generated keys, all `100`) becomes
# the signing key for every subsequent mint, and its `kid` in `/protocol/openid-connect/certs` is
# exactly the configured one. The private key is never written to this tier's own state files or
# realm document - only handed to `kcadm.sh` over its own admin session.
{ pkgs }:
let
  endpoints = import ./tier-endpoints.nix { inherit pkgs; };

  realm = "served-proof";
  client = "served-proof-cli";
  # The two fixed callers `test-infra/pulumi/google/__main__.py` binds
  # `roles/iam.workloadIdentityUser` to, by USERNAME - not by Keycloak's own internal user id, which
  # a torn-down-and-reprovisioned realm cannot keep stable across CI runs. The provider's
  # `attribute_mapping` reads `assertion.preferred_username`, which Keycloak's built-in "profile"
  # client scope maps from the username - confirmed live: a password-grant token for a user created
  # with this exact username carries `"preferred_username":"served-proof-caller-a"`.
  callers = [ "served-proof-caller-a" "served-proof-caller-b" ];
  # Both files that hardcode these two literals a second time: `test-infra/pulumi/google/
  # __main__.py`'s `SERVED_PROOF_ISSUER`/`SERVED_PROOF_KID`. Neither file can read the other's
  # source, so the three copies are kept byte-identical by hand - see either file's own header.
  issuer = "https://served-proof.sutura-ci.example.com/realms/served-proof";
  hostname = "https://served-proof.sutura-ci.example.com";
  kid = "served-proof-signing-key-1";
  # The one capability this venue's cell asks for - `Capability::AskMetric`'s scope name
  # (`crates/sutura-http/src/capability.rs`), duplicated here for `nix/keycloak-tier.nix`'s own
  # reason: this is nix, and nothing here reads Rust source.
  capabilityScope = "sutura:metrics.ask";
  # `security.inbound.resource` refuses anything that is not an absolute `https://` URI - the same
  # rule `nix/keycloak-tier.nix`'s own `resourceAudience` comment explains - and the WIF pool's
  # audience above is a `//iam.googleapis.com/...` resource path, not an `https://` one. So this
  # realm's client needs TWO custom audiences in one token: this one for SUTURA's own gate, the
  # pool's for GOOGLE's. Purely local to this tier and the test that reads it - no other file has
  # to agree on this literal, unlike `issuer`/`kid` above.
  resource = "https://served-proof-resource.sutura-ci.example.com";
in
rec {
  package = pkgs.keycloak;
  realmFile = ".sutura-dev/served-proof-realm.json";
  inherit realm client callers issuer resource;

  tier = pkgs.writeShellApplication {
    name = "sutura-served-proof-tier";
    runtimeInputs = [ pkgs.keycloak pkgs.curl pkgs.coreutils pkgs.jq pkgs.flock endpoints.script ];
    text = ''
      set -o errexit -o nounset

      root="$(pwd -P)"
      state="$root/.sutura-dev"
      home="$state/served-proof"
      pidfile="$home/tier.pid"
      log="$home/server.log"
      admincfg="$home/kcadm.json"
      realmfile="$state/served-proof-realm.json"
      realm=${realm}
      client=${client}
      issuer=${issuer}
      hostname=${hostname}
      kid=${kid}
      resource=${resource}
      capabilityScope=${capabilityScope}
      callers="${builtins.concatStringsSep " " callers}"

      export KC_HOME_DIR="$home"
      export KC_CONF_DIR="${pkgs.keycloak}/conf"
      export HOME="$home"

      generated() {
        head -c 24 /dev/urandom | base64 | tr -dc 'A-Za-z0-9'
      }

      # A plain pidfile probe - this module's own header says why that ceiling matches this tier's
      # one caller, and names the guard to port if that ever stops being true.
      running() {
        [ -f "$pidfile" ] && kill -0 "$(cat "$pidfile")" 2>/dev/null
      }

      published_port() {
        [ -f "$log" ] || return 0
        sed -n 's|.*Listening on: http://[^:]*:\([0-9]*\).*|\1|p' "$log" | tail -1
      }

      status() {
        running || return 1
        port="$(published_port)" || return 1
        [ -n "$port" ] || return 1
        [ -f "$realmfile" ] || return 1
        sutura-tier-endpoint published "$root" served-proof 127.0.0.1 "$port"
      }

      provision() {
        port="$1"
        base="http://127.0.0.1:$port"
        client_secret="$(generated)"

        for _ in $(seq 1 30); do
          if kcadm.sh config credentials --config "$admincfg" --server "$base" \
            --realm master --user "$admin_user" --password "$admin_password" >/dev/null 2>&1; then
            break
          fi
          sleep 2
        done

        kcadm.sh create realms --config "$admincfg" -s realm="$realm" -s enabled=true >/dev/null

        # The fixed signing key, given an EXPLICIT `kid` rather than one Keycloak would derive from
        # the public key - see this module's own header for the source that reads that attribute
        # before falling back. Priority above the realm's own default-generated keys (all `100`)
        # is what makes THIS key the one new tokens are signed with.
        #
        # ONE JSON value for the whole `config` field, `nix/keycloak-tier.nix`'s own pattern for
        # `attributes`/`protocolMappers` rather than a dotted `-s config.foo=` per key: `config` is
        # a `Map<String, List<String>>` on the wire, and `jq -n` builds it with the PEM safely
        # escaped rather than hand-quoted into a shell-built string, which is where a real key's
        # newlines would break a `-s key=value` split.
        component_config="$(jq -n --arg kid "$kid" --rawfile key "$signing_key_file" \
          '{active: ["true"], enabled: ["true"], priority: ["200"], algorithm: ["RS256"], kid: [$kid], privateKey: [$key]}')"
        kcadm.sh create components --config "$admincfg" -r "$realm" \
          -s name=served-proof-key -s providerId=rsa -s providerType=org.keycloak.keys.KeyProvider \
          -s "config=$component_config" >/dev/null

        client_uuid="$(kcadm.sh create clients --config "$admincfg" -r "$realm" \
          -s clientId="$client" -s enabled=true -s publicClient=false \
          -s directAccessGrantsEnabled=true -s standardFlowEnabled=false \
          -s secret="$client_secret" \
          -s 'protocolMappers=[{"name":"pool-aud","protocol":"openid-connect","protocolMapper":"oidc-audience-mapper","consentRequired":false,"config":{"included.custom.audience":"'"$audience"'","id.token.claim":"false","access.token.claim":"true","introspection.token.claim":"true"}},{"name":"resource-aud","protocol":"openid-connect","protocolMapper":"oidc-audience-mapper","consentRequired":false,"config":{"included.custom.audience":"'"$resource"'","id.token.claim":"false","access.token.claim":"true","introspection.token.claim":"true"}}]' \
          -i)"

        scope_uuid="$(kcadm.sh create client-scopes --config "$admincfg" -r "$realm" \
          -s name="$capabilityScope" -s protocol=openid-connect \
          -s 'attributes={"include.in.token.scope":"true","display.on.consent.screen":"false"}' -i)"
        kcadm.sh update "clients/$client_uuid/default-client-scopes/$scope_uuid" \
          --config "$admincfg" -r "$realm"

        for caller in $callers; do
          kcadm.sh create users --config "$admincfg" -r "$realm" \
            -s username="$caller" -s enabled=true -s emailVerified=true \
            -s email="$caller@example.com" -s firstName="$caller" -s lastName=fixture \
            -s 'requiredActions=[]' >/dev/null
          kcadm.sh set-password --config "$admincfg" -r "$realm" \
            --username "$caller" --new-password "$(caller_password "$caller")" >/dev/null
        done

        for caller in $callers; do
          token="$(curl -sS --max-time 20 -X POST \
            "$base/realms/$realm/protocol/openid-connect/token" \
            -d grant_type=password -d client_id="$client" -d client_secret="$client_secret" \
            -d username="$caller" -d "password=$(caller_password "$caller")")"
          case "$token" in
            *access_token*) ;;
            *)
              echo "served-proof tier: $caller could not obtain a token from $realm" >&2
              echo "$token" >&2
              exit 1
              ;;
          esac
        done

        (
          umask 077
          {
            printf '{"issuer":"%s","client":{"id":"%s","secret":"%s"},"callers":[' \
              "$issuer" "$client" "$client_secret"
            separator=
            for caller in $callers; do
              printf '%s{"username":"%s","password":"%s"}' \
                "$separator" "$caller" "$(caller_password "$caller")"
              separator=,
            done
            printf '],"capability_scope":"%s","kid":"%s","resource":"%s"}\n' "$capabilityScope" "$kid" "$resource"
          } > "$realmfile"
        )
      }

      caller_password() {
        printf '%s-%s' "$1" "$password_seed"
      }

      start() {
        if running; then
          if status; then
            echo "served-proof tier: already up - leaving it to whoever started it."
            return 0
          fi
          echo "served-proof tier: a JVM is running here with no usable claim on it - this" >&2
          echo "                   tier's one caller never leaves that state; stop it by hand." >&2
          exit 1
        fi
        signing_key_file="''${SUTURA_SERVED_PROOF_SIGNING_KEY_FILE:?set SUTURA_SERVED_PROOF_SIGNING_KEY_FILE to a PEM RSA private key}"
        [ -s "$signing_key_file" ] || { echo "served-proof tier: $signing_key_file is empty or missing" >&2; exit 1; }
        audience="''${SUTURA_SERVED_PROOF_AUDIENCE:?set SUTURA_SERVED_PROOF_AUDIENCE to the audience the served-proof provider accepts}"

        rm -rf "$home"
        mkdir -p "$home/data" "$state"
        for part in bin lib conf providers themes; do
          ln -s "${pkgs.keycloak}/$part" "$home/$part"
        done

        admin_user=tier-admin
        admin_password="$(generated)"
        password_seed="$(generated)"
        export KC_BOOTSTRAP_ADMIN_USERNAME="$admin_user"
        export KC_BOOTSTRAP_ADMIN_PASSWORD="$admin_password"

        set -m
        ( exec {start_fd}>"$home/served-proof.lock"
          flock -x "$start_fd"
          exec kc.sh start --optimized --cache=local --http-enabled=true \
            --http-host=127.0.0.1 --http-port=0 --http-management-port=0 \
            --hostname="$hostname" --hostname-admin="$hostname"
        ) >"$log" 2>&1 &
        echo "$!" > "$pidfile"
        set +m

        port=
        for _ in $(seq 1 90); do
          port="$(published_port)"
          [ -n "$port" ] && break
          if ! running; then
            echo "served-proof tier: the server exited before it published a port" >&2
            tail -20 "$log" >&2
            exit 1
          fi
          sleep 2
        done
        if [ -z "$port" ]; then
          echo "served-proof tier: no port published within the timeout" >&2
          tail -20 "$log" >&2
          stop
          exit 1
        fi

        provision "$port"
        sutura-tier-endpoint publish "$root" served-proof 127.0.0.1 "$port"
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
        sutura-tier-endpoint withdraw "$root" served-proof || true
        rm -f "$realmfile"
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

  # Proves the mechanism this module's own header claims, network-free: a THROWAWAY key stands in
  # for the CI secret, and no Google pool is involved - what is checked is that this tier's realm
  # signs with the key it was given, under the kid it was given, and mints the two claims Google's
  # side reads (`preferred_username`) and this deployment's own gate reads (`scope`). What it does
  # NOT check is answered nowhere in a sandbox: whether Google accepts a token this realm mints.
  check = pkgs.runCommand "served-proof-tier"
    {
      nativeBuildInputs = [ tier endpoints.script pkgs.jq pkgs.curl pkgs.openssl pkgs.python3 ];
    }
    ''
      tree="$NIX_BUILD_TOP/worktree"
      mkdir -p "$tree"
      cd "$tree"

      usage_state=0
      sutura-served-proof-tier >/dev/null 2>&1 || usage_state=$?
      test "$usage_state" = 2

      openssl genrsa -out throwaway-key.pem 2048 2>/dev/null
      export SUTURA_SERVED_PROOF_SIGNING_KEY_FILE="$tree/throwaway-key.pem"
      export SUTURA_SERVED_PROOF_AUDIENCE="//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/sutura/providers/served-proof"

      sutura-served-proof-tier start
      sutura-served-proof-tier status

      endpoints=.sutura-dev/endpoints.json
      test -f "$endpoints"
      test "$(jq -r '.services."served-proof".provisioner' "$endpoints")" = nix
      port="$(jq -r '.services."served-proof".port' "$endpoints")"
      test "$port" -gt 0

      realm=.sutura-dev/served-proof-realm.json
      test "$(jq -r '.issuer' "$realm")" = "${issuer}"
      test "$(jq -r '.kid' "$realm")" = "${kid}"
      test "$(jq -r '.capability_scope' "$realm")" = "${capabilityScope}"
      test "$(jq -r '.resource' "$realm")" = "${resource}"
      test "$(jq -r '.callers | length' "$realm")" = "${builtins.toString (builtins.length callers)}"

      base="http://127.0.0.1:$port"
      client_id="$(jq -r '.client.id' "$realm")"
      client_secret="$(jq -r '.client.secret' "$realm")"
      caller="$(jq -r '.callers[0].username' "$realm")"
      password="$(jq -r '.callers[0].password' "$realm")"
      token="$(curl -sS -X POST "$base/realms/${realm}/protocol/openid-connect/token" \
        -d grant_type=password -d client_id="$client_id" -d client_secret="$client_secret" \
        -d username="$caller" -d "password=$password" | jq -r .access_token)"
      python3 -c "
import base64, json, sys
def part(seg):
    seg += '=' * (-len(seg) % 4)
    return json.loads(base64.urlsafe_b64decode(seg))
header, payload = '$token'.split('.')[:2]
h, p = part(header), part(payload)
assert h['kid'] == '${kid}', h
assert p['iss'] == '${issuer}', p
assert p['preferred_username'] == '$caller', p
scopes = p['scope'].split(' ')
assert '${capabilityScope}' in scopes, p
aud = p['aud']
auds = aud if isinstance(aud, list) else [aud]
assert '$SUTURA_SERVED_PROOF_AUDIENCE' in auds, p
assert '${resource}' in auds, p
"

      sutura-served-proof-tier stop
      status=0
      sutura-served-proof-tier status >/dev/null 2>&1 || status=$?
      test "$status" = 1
      test ! -f "$realm"

      touch $out
    '';
}
