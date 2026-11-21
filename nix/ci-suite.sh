#!/usr/bin/env bash
set -euo pipefail

case "${1:-}" in
keycloak-served) nix run .#keycloak-served-test -- --no-capture ;;
bigquery-driver) bash nix/bigquery-driver-check.sh ;;
multi-node) nix build .#checks.x86_64-linux.multi-node -L ;;
multi-node-postgres) nix build .#checks.x86_64-linux.multi-node-postgres -L ;;
multi-node-oracle) nix build .#checks.x86_64-linux.multi-node-oracle -L ;;
k3s-autoscaling) nix build .#checks.x86_64-linux.k3s-autoscaling .#checks.x86_64-linux.k3s-worker-pool -L ;;
oracle-tier) nix run .#oracle-acceptance -- --no-capture ;;
oracle-native) SUTURA_ORACLE_ACCEPTANCE_SHARD=native nix run .#oracle-native-acceptance ;;
oracle-legacy) SUTURA_ORACLE_ACCEPTANCE_SHARD=legacy nix run .#oracle-native-acceptance ;;
datahub-tier) nix run .#datahub-acceptance ;;
openmetadata-tier) nix run .#openmetadata-acceptance ;;
ci-cube-tier) nix run .#cube-acceptance ;;
kind-smoke) nix run .#kind-smoke ;;
*) printf 'unknown QA suite: %s\n' "${1:-}" >&2; exit 2 ;;
esac
