#!/usr/bin/env bash
# `cargo deny` over the vendored ClickHouse driver's own lock.
#
# `vendor/adbc_clickhouse` is outside the workspace and carries no lock; the lock it builds against is
# `nix/clickhouse-adbc.Cargo.lock`, which neither `cargo deny check` (the root lock) nor the `fuzz/`
# run reads. cargo-deny wants the lock beside the manifest, so both are copied into a scratch
# directory. Run from the repository root; extra arguments go to `cargo deny check`.
#
# `-A license-not-encountered` for the reason the `fuzz/` run gives: `deny.toml`'s root-only allow
# entries are unused in this smaller graph and `unused-allowed-license = "deny"` would refuse them
# for a reason that is not a finding. Advisories, bans and sources stay fully denied.
set -euo pipefail

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
cp -R vendor/adbc_clickhouse/. "$scratch"
cp nix/clickhouse-adbc.Cargo.lock "$scratch/Cargo.lock"
cargo deny --manifest-path "$scratch/Cargo.toml" --config "$PWD/deny.toml" check -A license-not-encountered "$@"
