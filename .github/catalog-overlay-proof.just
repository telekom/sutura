test-only $filter $profile='':
    #!/usr/bin/env bash
    set -euo pipefail
    test "$profile" = ci
    test "${GITHUB_ACTIONS:-}" = true
    test "${SUTURA_PROOF_BOUND:-}" = 1
    : "${SUTURA_CATALOG_SOURCE:?the exact product checkout is required}"
    catalog_just="$(command -v just)"
    case "$catalog_just" in /nix/store/*/bin/just) ;; *) exit 2 ;; esac
    : "${SUTURA_CATALOG_JUSTFILE:?the reviewed helper task file is required}"
    cd "$SUTURA_CATALOG_SOURCE"
    case "$filter" in
        'binary_id(=sutura-catalog-rdbms) | binary_id(=sutura-cli::served) | binary_id(=sutura-config)' | 'binary_id(=sutura-catalog-rdbms)' | 'binary_id(=sutura-cli::served)' | 'binary_id(=sutura-config)') ;;
        *) echo 'catalog-proof: unexpected inventory filter' >&2; exit 2 ;;
    esac
    regular_status=0
    env -u JUST_JUSTFILE "$catalog_just" --justfile "$SUTURA_CATALOG_SOURCE/justfile" test-only "$filter" "$profile" || regular_status=$?
    native_status=0
    case "$filter" in
        *'binary_id(=sutura-cli::served)'*)
            env -u JUST_JUSTFILE "$catalog_just" --justfile "$SUTURA_CATALOG_JUSTFILE" native-cell "$profile" || native_status=$?
            ;;
    esac
    printf 'catalog-proof-exits: regular=%s native=%s\n' "$regular_status" "$native_status"
    if [ "$regular_status" -ne 0 ]; then
        exit "$regular_status"
    fi
    exit "$native_status"

native-cell $profile:
    #!/usr/bin/env bash
    set -euo pipefail
    test "$profile" = ci
    test "${GITHUB_ACTIONS:-}" = true
    test "${SUTURA_PROOF_BOUND:-}" = 1
    : "${SUTURA_CATALOG_SOURCE:?the exact product checkout is required}"
    cd "$SUTURA_CATALOG_SOURCE"
    export SUTURA_DEV_RELAXED_TOLERANCE=1
    cargo nextest run --workspace --all-features --cargo-profile "$profile" --run-ignored only --success-output immediate -E 'binary_id(=sutura-cli::served) & test(=native_oracle_dictionary::descriptions::a_served_oracle_dictionary_layers_a_separate_read_only_description_table)'
