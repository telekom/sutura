# The ClickHouse tier as ONE provisioner in two places: nixpkgs' `clickhouse`, started from the same
# script by the `checks.nextest` sandbox and by `just test` in the dev shell - `nix/postgres-tier.nix`'s
# arrangement, which `compose.services.yaml`'s `clickhouse` block named as the shape this would take.
#
# It is what makes ClickHouse's golden families EXECUTED rather than rendered: the golden matrix's
# `clickhouse` cells load the example corpus into this server and pin what it answers
# (`crates/sutura-app/tests/golden/dialects.rs`'s `Evidence::Executed`). The nix build sandbox has no
# network and no docker socket; this server needs neither - one loopback HTTP listener on a port
# the operating system allocates, published only once the server answers on it.
#
# `pkgs.clickhouse` is the same 26.7 series `compose.services.yaml` pins for the by-hand docker
# service; nothing compares the two, so a bump of either is a bump of one venue.
{ pkgs }:
let
  endpoints = import ./tier-endpoints.nix { inherit pkgs; };
in
rec {
  package = pkgs.clickhouse;

  tier = pkgs.writeShellApplication {
    name = "sutura-clickhouse-tier";
    runtimeInputs = [ pkgs.clickhouse endpoints.script pkgs.coreutils pkgs.curl pkgs.flock pkgs.gnugrep pkgs.python3 ];
    text = builtins.readFile ./clickhouse-tier-provision.sh;
  };

  # The tier's state machine, driven end to end - `checks.nextest` starts and stops this tier, which
  # says a server came up and nothing about whether `status` and `endpoints.json` agree, whether the
  # published credential is the one the server checks, or whether a second `start` re-credentials a
  # server a suite is connected to. Declared in `flake.nix` as one line pointing here, for the reason
  # `nix/postgres-tier.nix` gives at its own `check`.
  check = pkgs.runCommand "clickhouse-tier"
    {
      nativeBuildInputs = [ tier endpoints.script pkgs.jq pkgs.curl ];
    }
    ''
      tree="$NIX_BUILD_TOP/worktree"
      mkdir -p "$tree"
      cd "$tree"
      endpoints=.sutura-dev/endpoints.json

      tier_state() {
        state=0
        sutura-clickhouse-tier status || state=$?
        printf '%s' "$state"
      }

      expect_state() {
        got="$(tier_state)"
        if [ "$got" != "$1" ]; then
          echo "status answered $got, expected $1 - $2" >&2
          exit 1
        fi
      }

      expect_entry() {
        got=absent
        if [ -f "$endpoints" ]; then
          got="$(jq -r '.services | has("clickhouse")' "$endpoints")"
        fi
        if [ "$got" != "$1" ]; then
          echo "the clickhouse entry is '$got', expected '$1' - $2" >&2
          exit 1
        fi
      }

      expect_state 1 "nothing has been started"

      sutura-clickhouse-tier start
      expect_state 0 "a server that is running and published"
      expect_entry true "start publishes the service it brought up"
      port="$(jq -r '.services.clickhouse.port' "$endpoints")"

      # --- the published credential is the one the SERVER checks, and no other is ---
      published="$(sutura-clickhouse-tier credentials)"
      eval "$published"
      if [ -z "$SUTURA_CLICKHOUSE_TIER_USER" ] || [ -z "$SUTURA_CLICKHOUSE_TIER_PASSWORD" ]; then
        echo "credentials published an empty value:" >&2
        printf '%s\n' "$published" >&2
        exit 1
      fi
      answer="$(curl -fsS -u "$SUTURA_CLICKHOUSE_TIER_USER:$SUTURA_CLICKHOUSE_TIER_PASSWORD" \
        "http://127.0.0.1:$port/" --data-binary 'SELECT 41 + 1')"
      if [ "$answer" != 42 ]; then
        echo "the published credential did not run a query: '$answer'" >&2
        exit 1
      fi
      # The published password with one character more: any value but the published one.
      wrong="$SUTURA_CLICKHOUSE_TIER_PASSWORD-"
      if curl -fsS -u "$SUTURA_CLICKHOUSE_TIER_USER:$wrong" "http://127.0.0.1:$port/" \
        --data-binary 'SELECT 1' >/dev/null 2>&1; then
        echo "the server accepted a password the tier never published" >&2
        exit 1
      fi

      # --- ONE credential per running server: a second start must not re-credential it ---
      sutura-clickhouse-tier start
      if [ "$(sutura-clickhouse-tier credentials)" != "$published" ]; then
        echo "a second start republished a different credential" >&2
        exit 1
      fi

      # --- a running server nothing publishes is unclaimed (3), and start heals it ---
      sutura-tier-endpoint withdraw "$tree" clickhouse
      expect_entry absent "the last service out takes the file with it"
      expect_state 3 "a running server nothing publishes is unclaimed, not up"
      sutura-clickhouse-tier start
      expect_state 0 "start republished the entry for the server already running"
      if [ "$(jq -r '.services.clickhouse.port' "$endpoints")" != "$port" ]; then
        echo "the republished entry names a port the server is not on" >&2
        exit 1
      fi

      # --- an entry naming ANOTHER port is state 3 too, and start repairs it rather than trusting it ---
      sutura-tier-endpoint publish "$tree" clickhouse 127.0.0.1 1
      expect_state 3 "a server whose entry names another port is unclaimed, not up"
      sutura-clickhouse-tier start
      expect_state 0 "start republished the entry onto the port the server answers on"

      # --- stop takes the server, the claim and the credential with it ---
      sutura-clickhouse-tier stop
      expect_state 1 "a stopped tier"
      expect_entry absent "a stop that took withdraws the claim"
      if curl -fsS "http://127.0.0.1:$port/ping" >/dev/null 2>&1; then
        echo "a server still answers on $port after stop" >&2
        exit 1
      fi
      if [ -e .sutura-dev/clickhouse ]; then
        echo "stop left the home behind, credential and all" >&2
        exit 1
      fi
      if sutura-clickhouse-tier credentials >/dev/null 2>&1; then
        echo "credentials answered for a tier that is not there" >&2
        exit 1
      fi

      # A stop over nothing is a no-op, not a failure.
      sutura-clickhouse-tier stop

      touch "$out"
    '';
}
