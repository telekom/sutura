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

# The state backend is the `file://` URL in `PULUMI_BACKEND_URL` (set by the justfile or the
# workflow), never pulumi cloud, and `PULUMI_CONFIG_PASSPHRASE` supplies the stack secrets
# passphrase - so neither a cloud account nor a committed secret is involved. `.pulumi/`
# holds the state and is gitignored. Both are required: pulumi refuses a file backend without
# a passphrase, and this script refusing loudly beats a half-configured stack.
test -n "${PULUMI_BACKEND_URL:-}" || { echo "config-from-env: set PULUMI_BACKEND_URL (file://</path/to/.pulumi>)" >&2; exit 1; }
test -n "${PULUMI_CONFIG_PASSPHRASE:-}" || { echo "config-from-env: set PULUMI_CONFIG_PASSPHRASE" >&2; exit 1; }

STACK="dev"
if [ "${1:-}" = "--stack" ]; then
  STACK="$2"
else
  # No --stack given: target the ACTIVE stack, so `just infra-preview` driven by the machine
  # env needs no extra argument once a stack exists.
  STACK="$(pulumi stack --show-name 2>/dev/null || true)"
  test -n "$STACK" || { echo "config-from-env: no active stack - init/select one or pass --stack NAME" >&2; exit 1; }
fi

# With the file backend the state is keyed by stack name, so a stack that already exists must be
# SELECTED rather than re-initialised (a unique name, as CI uses, is a fresh init).
pulumi stack select --stack "$STACK" 2>/dev/null \
  || pulumi stack init --stack "$STACK"

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
  cap="${pair%%:*}"
  key="${pair##*:}"
  var="SUTURA_GOOGLE_${cap}"
  value="${!var:-}"
  if [ -z "$value" ]; then
    echo "e2e-gcp: no \$SUTURA_GOOGLE_${cap} - this stack cannot be previewed/up'd without it" >&2
    exit 1
  fi
  pulumi config set --stack "$STACK" "sutura-google-test-infra:${key}" "$value"
done

echo "e2e-gcp: stack $STACK configured from the environment (nothing printed here is disclosed)"
