#!/usr/bin/env bash
# Adopt the resources a lost local file-backend state left behind (the cloud resources still
# exist; the state that named them to Pulumi did not) into a FRESH stack - one `pulumi import`
# per resource in `__main__.py` whose id is derivable from the env + the program's own naming.
# After this, `just infra-preview` should show only genuinely NEW resources, not replacements.
#
# Reads the SAME `SUTURA_GOOGLE_*` env as `config-from-env.sh` (run that, or `just infra-preview`,
# first - the stack must already be selected/init'd and configured). Prints resource NAMES and
# status only - never a project, dataset, pool or account id, which are this repo's disclosure
# surface even on a maintainer's own terminal.
#
# Usage (from this directory, after `pulumi login` and `pulumi stack init <org>/<stack>`):
#   STACK=<org>/<stack> SUTURA_GOOGLE_PROJECT=... SUTURA_GOOGLE_DATASET=... \
#     SUTURA_GOOGLE_TABLE=... SUTURA_GOOGLE_CI_DATASET=... \
#     SUTURA_GOOGLE_WORKLOAD_POOL_ID=... SUTURA_GOOGLE_WORKLOAD_PROVIDER_ID=... \
#     bash import-existing.sh
#
# Idempotent: a resource already present in the stack's state is skipped, not re-imported (a
# second `pulumi import` of the same URN errors, so this checks `pulumi stack export` first).
set -euo pipefail
cd "$(dirname "$0")"

: "${STACK:?set STACK = the pulumi stack to import into (e.g. <org>/<stack>)}"
: "${SUTURA_GOOGLE_PROJECT:?set SUTURA_GOOGLE_PROJECT}"
: "${SUTURA_GOOGLE_DATASET:?set SUTURA_GOOGLE_DATASET}"
: "${SUTURA_GOOGLE_TABLE:?set SUTURA_GOOGLE_TABLE}"
: "${SUTURA_GOOGLE_CI_DATASET:?set SUTURA_GOOGLE_CI_DATASET}"
: "${SUTURA_GOOGLE_WORKLOAD_POOL_ID:?set SUTURA_GOOGLE_WORKLOAD_POOL_ID}"
: "${SUTURA_GOOGLE_WORKLOAD_PROVIDER_ID:?set SUTURA_GOOGLE_WORKLOAD_PROVIDER_ID}"

PROJECT="$SUTURA_GOOGLE_PROJECT"
DATASET="$SUTURA_GOOGLE_DATASET"
TABLE="$SUTURA_GOOGLE_TABLE"
CI_DATASET="$SUTURA_GOOGLE_CI_DATASET"
POOL_ID="$SUTURA_GOOGLE_WORKLOAD_POOL_ID"
PROVIDER_ID="$SUTURA_GOOGLE_WORKLOAD_PROVIDER_ID"

# `__main__.py`'s `sutura_name()`: f"{pulumi.get_stack()}-{leaf}"[:30]. `pulumi.get_stack()` is the
# stack's OWN short name (no org/project prefix), so a `--stack org/stack` argument is trimmed the
# same way here before the leaf is appended.
STACK_SHORT="${STACK##*/}"
sutura_name() {
  local leaf="$1"
  local full="${STACK_SHORT}-${leaf}"
  printf '%s' "${full:0:30}"
}
# Google's service-account email is `{account_id}@{project}.iam.gserviceaccount.com` - the API
# documented at https://cloud.google.com/iam/reference/rest/v1/projects.serviceAccounts, which is
# the doc `serviceaccount/account.py`'s own class docstring links (this pinned pulumi_gcp 9.35.1
# build has NO "## Import" section for `Account` to quote instead - a documented gap, not a guess).
sa_email() { printf '%s@%s.iam.gserviceaccount.com' "$(sutura_name "$1")" "$PROJECT"; }

SA_A_EMAIL="$(sa_email sa-a)"
SA_B_EMAIL="$(sa_email sa-b)"
CI_SA_EMAIL="$(sa_email ci)"

# Every URN pulumi already knows about, fetched once. A resource declared directly in `__main__.py`
# (none of them set `parent=`) gets a flat `urn:pulumi:<stack>::<project>::<type>::<name>` - so a
# `::<type>::<name>"` suffix match is enough to know it is already in state, without parsing JSON.
STATE_EXPORT="$(pulumi stack export --stack "$STACK" 2>/dev/null || true)"

already_imported() {
  local type="$1" name="$2"
  [ -n "$STATE_EXPORT" ] && printf '%s' "$STATE_EXPORT" | grep -qF "::${type}::${name}\""
}

do_import() {
  local type="$1" name="$2" id="$3"
  if already_imported "$type" "$name"; then
    echo "import-existing: $name - already in state, skipped"
    return 0
  fi
  echo "import-existing: $name - importing"
  pulumi import --stack "$STACK" --yes --protect=false --skip-preview "$type" "$name" "$id" >/dev/null
}

# --- gcp.projects.Service --------------------------------------------------------------------
# Format quoted verbatim from this pinned build's `projects/service.py` "## Import":
#   `pulumi import gcp:projects/service:Service default {{project_id}}/{{service}}`
do_import "gcp:projects/service:Service" "api-serviceusage.googleapis.com" "${PROJECT}/serviceusage.googleapis.com"
do_import "gcp:projects/service:Service" "api-bigquery.googleapis.com" "${PROJECT}/bigquery.googleapis.com"
do_import "gcp:projects/service:Service" "api-iam.googleapis.com" "${PROJECT}/iam.googleapis.com"

# --- gcp.serviceaccount.Account --------------------------------------------------------------
# No "## Import" section for `Account` in this build (see `sa_email` above for the source of the
# email shape); the fully-qualified resource name shape `projects/{{project}}/serviceAccounts/{{email}}`
# is quoted from `serviceaccount/iam_member.py`'s own "## Import" example in this SAME pinned
# build (`"projects/{your-project-id}/serviceAccounts/{your-service-account-email}"`).
do_import "gcp:serviceaccount/account:Account" "sa-a" "projects/${PROJECT}/serviceAccounts/${SA_A_EMAIL}"
do_import "gcp:serviceaccount/account:Account" "sa-b" "projects/${PROJECT}/serviceAccounts/${SA_B_EMAIL}"
do_import "gcp:serviceaccount/account:Account" "ci-sa" "projects/${PROJECT}/serviceAccounts/${CI_SA_EMAIL}"

# --- gcp.bigquery.Dataset / Table -------------------------------------------------------------
# Dataset format quoted from `bigquery/dataset.py` "## Import": `projects/{{project}}/datasets/{{dataset_id}}`.
do_import "gcp:bigquery/dataset:Dataset" "dataset" "projects/${PROJECT}/datasets/${DATASET}"
# `bigquery/table.py` has no "## Import" section in this build either. The id used here is a
# strict PREFIX of `bigquery/row_access_policy.py`'s own documented format for the SAME pinned
# build (`projects/{{project}}/datasets/{{dataset_id}}/tables/{{table_id}}/rowAccessPolicies/{{policy_id}}`)
# - read off a sibling resource in this package rather than recalled, but still not a literal
# Table example, so verify against pulumi's own error message if this one is wrong.
do_import "gcp:bigquery/table:Table" "table" "projects/${PROJECT}/datasets/${DATASET}/tables/${TABLE}"

# --- gcp.bigquery.Job (seed-rows) --------------------------------------------------------------
# SKIPPED. `job_id` in `__main__.py` is `sha256(_seed_statement)[:16]` over an f-string built from
# `dataset_id`/`table_id`/`group_column`/`principal_a_rows`/`principal_b_rows`/stack - reproducing
# it here would duplicate that construction across two files and break silently the moment either
# drifts, which is a worse failure than skipping outright.
#
# What happens instead: since this job is never imported, the next `up` tries to CREATE it fresh,
# under the SAME job_id it would compute for identical config - and `__main__.py`'s own comment
# beside `seed_statement` already documents (as an unverified expectation, never reproduced) that a
# completed BigQuery job id is not reusable within a project. This stack's project is NOT fresh -
# the seed already ran once, under the same id, before the state was lost - so the adopting `up` is
# expected to fail on this ONE resource with `Already Exists`. There is no importer path around
# that; if it happens, `pulumi state delete 'urn:...bigquery/job:Job::seed-rows'` (after confirming
# the row count in the table is already correct) is the documented recovery, not a fix here.
echo "import-existing: seed-rows - NOT imported (job import id is not safely re-derivable; the next 'up' may fail with Already Exists - see the comment above this line in the script)"

# --- gcp.bigquery.RowAccessPolicy --------------------------------------------------------------
# Format quoted from `bigquery/row_access_policy.py` "## Import":
#   projects/{{project}}/datasets/{{dataset_id}}/tables/{{table_id}}/rowAccessPolicies/{{policy_id}}
# `policy_id` is a literal in `__main__.py` ("rap_a" / "rap_b"), not derived from env.
do_import "gcp:bigquery/rowAccessPolicy:RowAccessPolicy" "rap-a" "projects/${PROJECT}/datasets/${DATASET}/tables/${TABLE}/rowAccessPolicies/rap_a"
do_import "gcp:bigquery/rowAccessPolicy:RowAccessPolicy" "rap-b" "projects/${PROJECT}/datasets/${DATASET}/tables/${TABLE}/rowAccessPolicies/rap_b"

# --- gcp.projects.IAMMember / gcp.bigquery.DatasetIamMember ------------------------------------
# Neither `projects/iam_member.py` nor `bigquery/dataset_iam_member.py` documents the BASE (no
# condition) import id in this build - only the conditional-binding variant is shown
# (`"{{your-project-id}} roles/{{role_id}} condition-title"`). The shapes below drop the trailing
# condition token and add the member, by analogy with that example and with every other
# space-separated `<parent> <role> <member>` IAM-member id in this provider; NOT confirmed against
# a live import. If either is wrong, `pulumi import` reports the id shape it expected - rerun with
# that instead of the line below.
SA_A_MEMBER="serviceAccount:${SA_A_EMAIL}"
SA_B_MEMBER="serviceAccount:${SA_B_EMAIL}"
CI_SA_MEMBER="serviceAccount:${CI_SA_EMAIL}"

do_import "gcp:projects/iAMMember:IAMMember" "principal-a-jobuser" "${PROJECT} roles/bigquery.jobUser ${SA_A_MEMBER}"
do_import "gcp:bigquery/datasetIamMember:DatasetIamMember" "principal-a-dataviewer" "projects/${PROJECT}/datasets/${DATASET} roles/bigquery.dataViewer ${SA_A_MEMBER}"
do_import "gcp:projects/iAMMember:IAMMember" "principal-b-jobuser" "${PROJECT} roles/bigquery.jobUser ${SA_B_MEMBER}"
do_import "gcp:bigquery/datasetIamMember:DatasetIamMember" "principal-b-dataviewer" "projects/${PROJECT}/datasets/${DATASET} roles/bigquery.dataViewer ${SA_B_MEMBER}"
do_import "gcp:projects/iAMMember:IAMMember" "ci-bigquery-jobuser" "${PROJECT} roles/bigquery.jobUser ${CI_SA_MEMBER}"
do_import "gcp:bigquery/datasetIamMember:DatasetIamMember" "ci-bigquery-dataeditor" "projects/${PROJECT}/datasets/${DATASET} roles/bigquery.dataEditor ${CI_SA_MEMBER}"
do_import "gcp:bigquery/datasetIamMember:DatasetIamMember" "ci-bigquery-dataeditor-external" "projects/${PROJECT}/datasets/${CI_DATASET} roles/bigquery.dataEditor ${CI_SA_MEMBER}"

# --- gcp.serviceaccount.Key ---------------------------------------------------------------------
# NOT IMPORTABLE. `serviceaccount/key.py`'s own "## Import" section states plainly: "This resource
# does not support import" - the private key material is unrecoverable from the API by design, so
# there is nothing an id could name. The next `up` CREATES all three keys (key-a, key-b, ci-key)
# fresh: `just infra-set` (or `sync-bq-test-env.sh`) MUST be re-run afterwards to push the new
# `bq-test` secrets, and the PREVIOUS keys stay live in the cloud until deleted by hand, e.g.
# `gcloud iam service-accounts keys list --iam-account=<email>` then `... keys delete <key-id>
# --iam-account=<email>` for each account, once nothing still depends on the old key.
echo "import-existing: key-a, key-b, ci-key - NOT importable (no import support); the next 'up' creates all three fresh - re-run 'just infra-set' afterwards, and remove the old keys by hand"

# --- gcp.iam.WorkloadIdentityPool / WorkloadIdentityPoolProvider -------------------------------
# Formats quoted from `iam/workload_identity_pool.py` / `iam/workload_identity_pool_provider.py`
# "## Import".
do_import "gcp:iam/workloadIdentityPool:WorkloadIdentityPool" "workload-pool" "projects/${PROJECT}/locations/global/workloadIdentityPools/${POOL_ID}"
do_import "gcp:iam/workloadIdentityPoolProvider:WorkloadIdentityPoolProvider" "workload-provider" "projects/${PROJECT}/locations/global/workloadIdentityPools/${POOL_ID}/providers/${PROVIDER_ID}"

# --- gcp.serviceaccount.IAMMember (principal-{a,b}-workload-identity-user) ---------------------
# NOT imported, deliberately: these two bindings are the NEW addition (telekom/sutura#376's
# iamcredentials hop) this branch adds to `__main__.py` - they were never created under the lost
# state, so there is nothing existing to adopt. `just infra-preview` after this script is expected
# to show exactly these two as creates, plus the three keys above.
echo "import-existing: principal-a-workload-identity-user, principal-b-workload-identity-user - NOT imported (new resources, not yet created; the next 'up' creates them)"

echo "import-existing: done - run 'just infra-preview' and confirm it shows only the two new bindings and the three keys"
