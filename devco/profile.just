# Hosted diagnostic only; product source and tests remain unchanged.
ci-only filter:
    #!/usr/bin/env bash
    set -euo pipefail
    source nix/with-tier.sh
    sutura_tier_up
    export SUTURA_DEV_RELAXED_TOLERANCE=1
    cargo nextest run --workspace --all-features --cargo-profile ci -E "{{filter}}"
