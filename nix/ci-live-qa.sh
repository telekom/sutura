#!/usr/bin/env bash
# Run one live QA suite inside its caller's environment-protected job.
set -euo pipefail

suite=${1:-}
case "$suite" in
  bigquery-conformance|e2e-datahub-adbc) ;;
  *) printf 'Unknown live QA suite: %s\n' "$suite" >&2; exit 2 ;;
esac
if [ -z "${SUTURA_BQ_KEY:-}" ]; then
  echo "bq-test has no CI credential" >&2
  exit 1
fi
if [ -z "${SUTURA_BQ_DATASET:-}" ]; then
  echo "bq-test has no SUTURA_BQ_DATASET" >&2
  exit 1
fi

umask 077
credential=$(mktemp "$RUNNER_TEMP/sutura-bq-key.XXXXXXXX")
trap 'rm -f "$credential"' EXIT
printf '%s\n' "$SUTURA_BQ_KEY" > "$credential"
unset SUTURA_BQ_KEY
export GOOGLE_APPLICATION_CREDENTIALS="$credential"
project=$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))["project_id"])' "$credential" 2>/dev/null) ||
  { echo "the CI credential names no project_id" >&2; exit 1; }
if [ -z "$project" ]; then
  echo "the CI credential names an empty project_id" >&2
  exit 1
fi
printf '::add-mask::%s\n' "$project" "$SUTURA_BQ_DATASET"

case "$suite" in
  bigquery-conformance)
    export SUTURA_BQ_PROJECT="$project"
    env -u SUTURA_BQ_REGION nix run .#bigquery-provision -- --no-capture
    nix run .#bigquery-conformance -- --no-capture
    ;;
  e2e-datahub-adbc)
    nix run .#e2e-datahub-adbc -- --no-capture --no-fail-fast
    ;;
esac
