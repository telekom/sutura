#!/usr/bin/env bash
# Resource values stay outside source; register them before an explicit live task emits output.
set -euo pipefail
if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
  for key in SUTURA_BQ_CROSS_BILLING_PROJECT SUTURA_BQ_DATASET \
    SUTURA_BQ_RLS_PROJECT SUTURA_BQ_RLS_DATASET \
    SUTURA_BQ_CROSS_DATASET_PROJECT SUTURA_BQ_CROSS_DATASET \
    SUTURA_BQ_MIRROR_PROJECT SUTURA_BQ_MIRROR_DATASET; do
    value="${!key-}"
    if [[ -n "$value" ]]; then
      value="${value//'%'/'%25'}"
      value="${value//$'\r'/'%0D'}"
      value="${value//$'\n'/'%0A'}"
      printf '::add-mask::%s\n' "$value"
    fi
  done
fi
