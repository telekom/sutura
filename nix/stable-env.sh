# shellcheck shell=bash
# Put the STABLE toolchain in front, for anything that gates.
#
# The dev shell's bare `cargo` is nightly, because cranelift is nightly-only and it is what
# makes the inner loop fast. Gates must not inherit that:
#
#   - Clippy's lint set differs between channels and this repo enables the whole `restriction`
#     category with `-D warnings`. Nightly clippy knows lints stable does not, so gating on
#     nightly means local failures CI cannot reproduce and local passes CI rejects.
#   - Alternating compilers in ONE target directory invalidates every artifact in it. Without
#     a separate directory each gate run would force a full rebuild, and so would going back
#     to the inner loop - which costs more than cranelift saves.
#
# Sourced by the devenv gate scripts and by the justfile gate recipes, so there is one
# definition of "run this the way CI runs it". Outside the dev shell SUTURA_STABLE_BIN is
# unset and this is a no-op beyond the target directory, which is what a bare-rustup or CI
# checkout needs.
if [ -n "${SUTURA_STABLE_BIN:-}" ]; then
    PATH="$SUTURA_STABLE_BIN:$PATH"
    export PATH
fi

# cranelift is nightly-only: stable cargo refuses to start with these set.
unset CARGO_UNSTABLE_CODEGEN_BACKEND CARGO_PROFILE_DEV_CODEGEN_BACKEND

# Only split the directory when there are two toolchains to keep apart. In CI there is one,
# and a nested path there would just miss the restored cache.
if [ -n "${SUTURA_STABLE_BIN:-}" ]; then
    CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target}/stable"
    export CARGO_TARGET_DIR
fi
