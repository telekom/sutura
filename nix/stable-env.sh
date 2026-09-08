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
# definition of "run this the way CI runs it". Direct local Rust gates require the dev shell's
# configured toolchain. Nix checks and apps supply their own pinned tools without this helper.
# This trusts the configured binaries, not arbitrary wrappers or compiler overrides.
if [ -z "${SUTURA_STABLE_BIN:-}" ]; then
    echo "stable-env: SUTURA_STABLE_BIN is unset or empty; enter devenv shell before running local Rust gates" >&2
    exit 1
fi
for sutura_stable_tool in cargo rustc rustdoc cargo-fmt rustfmt cargo-clippy clippy-driver; do
    if [ ! -f "$SUTURA_STABLE_BIN/$sutura_stable_tool" ] || [ ! -x "$SUTURA_STABLE_BIN/$sutura_stable_tool" ]; then
        echo "stable-env: SUTURA_STABLE_BIN lacks executable $sutura_stable_tool; enter devenv shell" >&2
        exit 1
    fi
done
unset sutura_stable_tool

# Exit above, not return: hook entries source this before a semicolon, without errexit.
PATH="$SUTURA_STABLE_BIN:$PATH"
export PATH

# cranelift is nightly-only: stable cargo refuses to start with these set.
unset CARGO_UNSTABLE_CODEGEN_BACKEND CARGO_PROFILE_DEV_CODEGEN_BACKEND

# Keep the local gate artifacts separate from the nightly inner loop.
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target}/stable"
export CARGO_TARGET_DIR
