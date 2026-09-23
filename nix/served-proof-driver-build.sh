#!/usr/bin/env bash
# Builds the BigQuery ADBC driver for this runner's triple and prints its `.so` path.
#
# A SEPARATE script rather than an inline `run:` step, on `nix/bigquery-driver-check.sh`'s own
# pattern: `xtask check-workflows` refuses a literal `nix build .#<release output>` inside a file
# ordinary CI's own graph walk reaches, because that walk cannot see into an invoked script. The
# SAME build already runs in ordinary CI through that script's `.#sutura`/
# `.#sutura-x86_64-unknown-linux-musl` lines and through `.github/workflows/bigquery-declared-
# principal.yml`'s own copy of this exact line - that file sits outside the walk because it is
# `workflow_dispatch`-only, which this leg cannot be (it has to reach `pull_request`).
set -euo pipefail
out="$(nix build --no-link --print-out-paths .#adbc-driver-bigquery-x86_64-unknown-linux-gnu)"
echo "$out/lib/libadbc_driver_bigquery.so"
