#!/usr/bin/env bash
# Re-export this stack's outputs into the bq-test GitHub environment's secrets and vars, so a
# fresh `just infra-up` (e.g. after an `infra-down`) leaves CI pointed at current credentials
# without hand-editing. Idempotent: safe to run again.
#
# Reads the LOCAL file-backend state - `pulumi stack output` reads the `.pulumi/` state, so NO
# GCP credential is needed. Pushes each output with the `gh` CLI, which must be installed and
# authenticated with write access to the repo's chosen environment.
#
# No value is ever printed: the secret keys are base64-decoded (pulumi `-j` encodes secret outputs)
# into files under a mode-0600 temp dir that a trap removes, and `gh secret set` reads them from
# stdin.
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

# Secret keys: pulumi `-j` base64-encodes secret outputs, so decode each to a key document file.
SECRETS = {
    "SVC_SUTURUA_BQ_CI": "ci_key",
    "SVC_SUTURUA_BQ_PRINCIPAL_A": "principal_a_key",
    "SVC_SUTURUA_BQ_PRINCIPAL_B": "principal_b_key",
}
need(*SECRETS.values())
paths = {}
for gh_name, out in SECRETS.items():
    f = f"{tmp}/k-{gh_name}.json"
    doc = base64.b64decode(d[out]).decode("utf-8")
    json.loads(doc)  # must be a service-account key document; refuse anything else
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
    "SUTURA_BQ_WORKLOAD_AUDIENCE": "workload_audience",
    "SUTURA_BQ_PRINCIPAL_A_EMAIL": "principal_a_email",
    "SUTURA_BQ_PRINCIPAL_B_EMAIL": "principal_b_email",
}
need(*VARS.values())
for gh_name, out in VARS.items():
    with open(os.devnull, "wb") as null:
        subprocess.run(["gh", "variable", "set", gh_name, "-e", env, "--body", str(d[out])],
                       stdout=null, timeout=60, check=True)
    print(f"sync: var {gh_name} = {d[out]}")

print(f"sync: environment '{env}' is up to date with stack {os.environ['STACK']}")
PY
