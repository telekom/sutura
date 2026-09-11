#!/usr/bin/env bash
# Re-export this stack's outputs into the bq-test GitHub environment's secrets and vars, so a
# fresh `just infra-up` (e.g. after an `infra-down`) leaves CI pointed at current credentials
# without hand-editing. Idempotent: safe to run again.
#
# Reads the LOCAL file-backend state - `pulumi stack output` reads the `.pulumi/` state, so NO
# GCP credential is needed. Pushes each output with the `gh` CLI, which must be installed and
# authenticated with write access to the repo's chosen environment.
#
# No SECRET value is ever printed: each is written into a file under a mode-0600 temp dir that a
# trap removes, and `gh secret set` reads it from stdin rather than from an argv. A var's value IS
# printed, which is part of why the three identity names below are secrets.
#
# Usage (from this directory):
#   STACK=sutura-test BQ_TEST_ENV=bq-test \
#     PULUMI_BACKEND_URL=file://... PULUMI_CONFIG_PASSPHRASE=... bash sync-bq-test-env.sh
set -euo pipefail
cd "$(dirname "$0")"

: "${STACK:?set STACK = the pulumi stack whose outputs to export (e.g. sutura-test)}"
: "${BQ_TEST_ENV:=bq-test}"
: "${PULUMI_BACKEND_URL:?set PULUMI_BACKEND_URL (file://.../test-infra/pulumi/google)}"
: "${PULUMI_CONFIG_PASSPHRASE:?set PULUMI_CONFIG_PASSPHRASE}"
command -v gh >/dev/null || { echo "sync: gh CLI not on PATH" >&2; exit 1; }

umask 077
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

pulumi stack output --stack "$STACK" --show-secrets -j > "$TMP/out.json"

STACK="$STACK" BQ_TEST_ENV="$BQ_TEST_ENV" TMP="$TMP" python3 - <<'PY'
import base64, json, os, subprocess, sys

tmp = os.environ["TMP"]
env = os.environ["BQ_TEST_ENV"]
d = json.load(open(f"{tmp}/out.json", encoding="utf-8"))

def need(*names):
    miss = [n for n in names if n not in d]
    if miss:
        print("sync: stack lacks output(s) " + ", ".join(miss)
              + "; run `just infra-up` first", file=sys.stderr)
        sys.exit(1)

# The three service-account keys, plus the three values that NAME an identity in the acceptance
# project. Secrets rather than vars for the identity names too: they are not credential material,
# and they are still identifiers of that project and of two accounts in it, which a job log and a
# `gh variable list` must not carry. The var loop below prints every value it sets; this one prints
# none, and `gh secret set` reads each from stdin rather than from an argv the process table shows.
SECRETS = {
    "SVC_SUTURUA_BQ_CI": "ci_key",
    "SVC_SUTURUA_BQ_PRINCIPAL_A": "principal_a_key",
    "SVC_SUTURUA_BQ_PRINCIPAL_B": "principal_b_key",
    "SUTURA_BQ_WORKLOAD_AUDIENCE": "workload_audience",
    "SUTURA_BQ_PRINCIPAL_A_EMAIL": "principal_a_email",
    "SUTURA_BQ_PRINCIPAL_B_EMAIL": "principal_b_email",
}
# Which of those stack outputs is a key document: pulumi `-j` base64-encodes a `secret` output, so
# those three decode to a JSON document and the other three are plain strings pushed as they are.
KEY_DOCS = {"ci_key", "principal_a_key", "principal_b_key"}
need(*SECRETS.values())
paths = {}
for gh_name, out in SECRETS.items():
    f = f"{tmp}/gh-{gh_name}"
    if out in KEY_DOCS:
        doc = base64.b64decode(d[out]).decode("utf-8")
        json.loads(doc)  # must be a service-account key document; refuse anything else
    else:
        doc = str(d[out])
    open(f, "w", encoding="utf-8").write(doc)
    paths[gh_name] = f

for gh_name, f in paths.items():
    with open(f, "rb") as fh, open(os.devnull, "wb") as null:
        subprocess.run(["gh", "secret", "set", gh_name, "-e", env],
                       stdin=fh, stdout=null, timeout=60, check=True)
    print(f"sync: secret {gh_name} set")

VARS = {
    "SUTURA_BQ_DATASET": "ci_dataset",
    "SUTURA_BQ_TABLE": "ci_table",
    # The five the two-principal cell has to be pointed at: the policied dataset and table, the
    # column the two row access policies filter on, and the grouping value each policy grants. Vars
    # rather than secrets: none of them is credential material. The workflow registers its two
    # resource names for redaction, but the runner prints that step's resolved `env` before its body
    # runs, so their first occurrence is visible and only subsequent ones are masked (#527).
    #
    # The policied dataset is NOT the one the acceptance legs run against, and that is an assertion
    # here only because `__main__.py` refuses `dataset == ci_dataset` - it used to be four
    # independent config keys distinct by placeholder value alone. The reason it has to hold: the
    # acceptance legs render `CREATE OR REPLACE TABLE`, which drops a table's row access policies.
    "SUTURA_BQ_RLS_DATASET": "dataset",
    "SUTURA_BQ_RLS_TABLE": "table",
    "SUTURA_BQ_GROUP_COLUMN": "group_column",
    "SUTURA_BQ_PRINCIPAL_A_ROWS": "principal_a_rows",
    "SUTURA_BQ_PRINCIPAL_B_ROWS": "principal_b_rows",
}
need(*VARS.values())
for gh_name, out in VARS.items():
    with open(os.devnull, "wb") as null:
        subprocess.run(["gh", "variable", "set", gh_name, "-e", env, "--body", str(d[out])],
                       stdout=null, timeout=60, check=True)
    print(f"sync: var {gh_name} = {d[out]}")

print(f"sync: environment '{env}' is up to date with stack {os.environ['STACK']}")
PY
