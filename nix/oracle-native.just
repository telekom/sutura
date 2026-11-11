# Oracle adapter, catalog, served password/split and sutura-app cells; the `oracle-tier` CI job runs it. Keep the CI app's filters aligned.
oracle-acceptance:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "oracle-acceptance: scope sutura-exec-oracle, sutura-catalog-rdbms, sutura-cli's served source-password/split cells and sutura-app's oracle cells. NOT a gate: run \`just test\` for the whole suite."
    export SUTURA_DEV_USER="${SUTURA_DEV_USER:-sutura}"
    export SUTURA_DEV_PASSWORD="${SUTURA_DEV_PASSWORD:-$(openssl rand -hex 12)}"
    just dev-up-oracle
    export SUTURA_DEV_REQUIRE_TIER=1
    export SUTURA_ORACLE_TIER_CA="$PWD/target/oracle-tier-tls/ca.pem"
    set +e
    cargo nextest run -p sutura-exec-oracle -p sutura-catalog-rdbms --all-features --run-ignored only --no-fail-fast -E 'test(/^acceptance::/) | test(/^oracle_provisioned::/)'
    status=$?
    cargo test -p sutura-cli --features agent,oracle --test served -- source_password::an_oracle_source_password_reaches_no_reply_and_no_log_line_of_a_served_binary --ignored --nocapture || status=$?
    bash nix/oracle-split-cells.sh || status=$?
    cargo test -p sutura-cli --features oracle,rdbms,sql --test served native_oracle_dictionary:: -- --ignored --nocapture || status=$?
    bash nix/oracle-app-cells.sh || status=$?
    exit "$status"

oracle-tier: oracle-acceptance

# Hosted-only feasibility probe for an actual 10G-only Oracle XE account.
oracle-legacy-probe:
    bash nix/oracle-legacy-probe.sh

oracle-native-probe:
    bash nix/oracle-native-probe.sh

# Real source and dictionary YAML logins against required native encryption and integrity.
oracle-native-acceptance:
    SUTURA_ORACLE_RUN_ACCEPTANCE=1 bash nix/oracle-native-probe.sh
    SUTURA_ORACLE_RUN_ACCEPTANCE=1 bash nix/oracle-legacy-probe.sh

# Hosted live base-red proof for both ignored product cells.
oracle-native-base-proof base:
    SUTURA_ORACLE_BASE_SHA={{quote(base)}} nix run .#oracle-native-acceptance

# Hosted killing mutations for the two served product cells, using one modern fixture.
oracle-native-claim-proof:
    SUTURA_ORACLE_CLAIM_PROOF=1 nix run .#oracle-native-acceptance

# Hosted proof of the served split's no-reader refusal and real reading-worker path.
oracle-split-claim-proof:
    SUTURA_ORACLE_SPLIT_CLAIM_PROOF=1 nix run .#oracle-acceptance
