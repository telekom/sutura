#!/usr/bin/env bash
# Realise the selected core and tier checks in one graph.
set -euo pipefail

full=false
case "${1:-}" in
  --full) full=true ;;
  '') ;;
  *) printf 'Unknown core graph option: %s\n' "$1" >&2; exit 2 ;;
esac

checks=()
if [ "$full" = true ] || [ "${RUST:-false}" = true ]; then
  checks+=(.#checks.x86_64-linux.nextest .#checks.x86_64-linux.doctest .#checks.x86_64-linux.crap)
fi
if [ "$full" = true ] || [ "${TIERS:-false}" = true ] || [ "${IDENTITY:-false}" = true ]; then
  checks+=(.#checks.x86_64-linux.keycloak-tier)
fi
if [ "$full" = true ] || [ "${TIERS:-false}" = true ] || [ "${POSTGRES:-false}" = true ]; then
  checks+=(.#checks.x86_64-linux.postgres-tier)
fi
if [ "$full" = true ] || [ "${TIERS:-false}" = true ] || [ "${CLICKHOUSE:-false}" = true ]; then
  checks+=(.#checks.x86_64-linux.clickhouse-tier .#checks.x86_64-linux.vendored-clickhouse-adbc)
fi
if [ "$full" = true ] || [ "${TIERS:-false}" = true ] || [ "${ORACLE:-false}" = true ]; then
  checks+=(.#checks.x86_64-linux.vendored-oracledb)
fi
if [ "$full" = true ] || [ "${API:-false}" = true ]; then
  checks+=(.#checks.x86_64-linux.api-docs)
fi
if [ "$full" = true ]; then
  checks+=(
    .#checks.x86_64-linux.reuse
    .#checks.x86_64-linux.helm-chart
    .#checks.x86_64-linux.adbc-driver-bigquery
    .#checks.x86_64-linux.adbc-driver-postgresql
    .#checks.x86_64-linux.adbc-driver-duckdb
    .#checks.x86_64-linux.adbc-driver-clickhouse
    .#checks.x86_64-linux.postgres-linked-driver
  )
fi
if [ "${#checks[@]}" -gt 0 ]; then
  nix build -L --keep-going "${checks[@]}"
fi
