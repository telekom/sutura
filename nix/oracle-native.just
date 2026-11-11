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
