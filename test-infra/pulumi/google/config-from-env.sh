#!/usr/bin/env bash
# Map the deployment's environment into this stack's Pulumi config, WITHOUT hardcoding any
# identifier in the tree. Every `SUTURA_GOOGLE_<KEY>` variable becomes a
# `sutura-google-test-infra:<key>` config value; the project and the pool come from the
# GitHub `e2e-gcp` environment, never from a committed file.
#
# The Pulumi provider's OWN credentials come from `GOOGLE_APPLICATION_CREDENTIALS`, which the
# workflow points at a gitignored file written from a secret - not something this script places.
#
# Usage (from test-infra/pulumi/google):
#   SUTURA_GOOGLE_PROJECT=... SUTURA_GOOGLE_WORKLOAD_POOL_ID=... ... bash config-from-env.sh [--stack NAME]
#
# `require` keys are baked into the Pulumi program; the script refuses to run without them so a
# half-configured `e2e-gcp` environment fails loudly rather than previewing a broken stack.
set -euo pipefail

cd "$(dirname "$0")"

STACK="dev"
if [ "${1:-}" = "--stack" ]; then
  STACK="$2"
fi

# The values the program requires, in human order, and the one config key each maps to.
declare -a REQUIRED=(
  "PROJECT:project"
  "REGION:region"
  "DATASET:dataset"
  "TABLE:table"
  "GROUP_COLUMN:group_column"
  "WORKLOAD_POOL_ID:workload_pool_id"
  "WORKLOAD_PROVIDER_ID:workload_provider_id"
  "WORKLOAD_ISSUER_URI:workload_issuer_uri"
  "WORKLOAD_ALLOWED_AUDIENCES:workload_allowed_audiences"
)

for pair in "${REQUIRED[@]}"; do
  var="${pair%%:*}"
  key="${pair##*:}"
  value="${!var:-}"
  if [ -z "$value" ]; then
    echo "e2e-gcp: no \$SUTURA_GOOGLE_${var} - this stack cannot be previewed/up'd without it" >&2
    exit 1
  fi
  pulumi config set --stack "$STACK" "sutura-google-test-infra:${key}" "$value"
done

echo "e2e-gcp: stack $STACK configured from the environment (nothing printed here is disclosed)"
