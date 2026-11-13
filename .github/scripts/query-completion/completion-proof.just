set shell := ["bash", "-euo", "pipefail", "-c"]

proof-census venue:
    #!/usr/bin/env bash
    set -euo pipefail
    case '{{venue}}' in
      offline) cargo nextest list --cargo-profile ci --workspace --all-features --message-format json ;;
      vendor) cargo nextest list --cargo-profile ci -p datafusion-distributed --lib --all-features --message-format json ;;
      bigquery-cli) cargo nextest list --cargo-profile ci -p sutura-cli --bin sutura --all-features --message-format json ;;
      bigquery-adapter) cargo nextest list --cargo-profile ci -p sutura-exec-bigquery --test declared_principal --all-features --message-format json ;;
      native-oracle) cargo nextest list --cargo-profile ci -p sutura-exec-oracle --test acceptance --test flashback --all-features --message-format json ;;
      oracle-served) cargo nextest list --cargo-profile ci -p sutura-cli --test served --all-features --message-format json ;;
      *) exit 1 ;;
    esac

proof-offline binary:
    #!/usr/bin/env bash
    set -euo pipefail
    source nix/with-tier.sh
    sutura_tier_up
    export SUTURA_DEV_RELAXED_TOLERANCE=1
    cargo nextest run --cargo-profile ci --workspace --all-features --no-fail-fast -E 'binary_id(={{binary}})'

proof-vendor-registry:
    #!/usr/bin/env bash
    set -euo pipefail
    vendor="$(nix build .#checks.x86_64-linux.nextest.cargoVendorDir --no-link --print-out-paths)"
    printf '%s\n' "$vendor"
    install -m 644 "$vendor/config.toml" "$CARGO_HOME/config.toml"

proof-vendor binary:
    cargo nextest run --cargo-profile ci -p datafusion-distributed --lib --all-features --no-fail-fast -E 'binary_id(={{binary}})'

proof-bigquery-cli binary:
    cargo nextest run --cargo-profile ci -p sutura-cli --bin sutura --all-features --run-ignored only --no-fail-fast -E 'binary_id(={{binary}})'

proof-bigquery-adapter binary:
    cargo nextest run --cargo-profile ci -p sutura-exec-bigquery --test declared_principal --all-features --run-ignored only --no-fail-fast -E 'binary_id(={{binary}})'

proof-postgres:
    nix build .#checks.x86_64-linux.multi-node-postgres --no-link --print-out-paths -L
    nix build --rebuild .#checks.x86_64-linux.multi-node-postgres --no-link --print-out-paths -L

proof-postgres-log:
    #!/usr/bin/env bash
    set -euo pipefail
    drv="$(nix path-info --derivation .#checks.x86_64-linux.multi-node-postgres)"
    nix log "$drv"

proof-oracle-setup just_app:
    "{{just_app}}" dev-up-oracle

proof-oracle-adapter binary:
    cargo nextest run --cargo-profile ci -p sutura-exec-oracle --test acceptance --test flashback --all-features --run-ignored all --no-fail-fast -E 'binary_id(={{binary}})'

proof-oracle-served-default-census binary:
    cargo nextest list --cargo-profile ci -p sutura-cli --test served --all-features --message-format json -E 'binary_id(={{binary}})'

proof-oracle-served-live-census binary name:
    cargo nextest list --cargo-profile ci -p sutura-cli --test served --all-features --run-ignored only --message-format json -E 'binary_id(={{binary}}) & test(={{name}})'

proof-oracle-served-default binary:
    #!/usr/bin/env bash
    set -euo pipefail
    source nix/with-tier.sh
    sutura_tier_up
    cargo nextest run --cargo-profile ci -p sutura-cli --test served --all-features --no-fail-fast -E 'binary_id(={{binary}})'

proof-oracle-served-live binary name:
    #!/usr/bin/env bash
    set -euo pipefail
    source nix/with-tier.sh
    sutura_tier_up
    cargo nextest run --cargo-profile ci -p sutura-cli --test served --all-features --run-ignored only --no-fail-fast -E 'binary_id(={{binary}}) & test(={{name}})'
