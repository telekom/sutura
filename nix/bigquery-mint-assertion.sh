#!/usr/bin/env bash
# Mints one Google-issued ID token for a service account, from that account's own key document -
# the subject assertion the workload-identity pool verifies (telekom/sutura#376's "no new
# long-lived secret" design, needed again now that the transport federates the asker's assertion).
#
# WHY A SCRIPT AND NOT A FLAKE APP. The app that used to do this
# (`crates/sutura-exec-bigquery/examples/mint_subject_assertion.rs`) went with the HTTP transport,
# and this crate ships no HTTP client any more - the driver owns the transport. Restoring a Rust
# minting example would mean restoring an outbound client for a job step. `openssl` and `curl` are
# on every runner this dispatches on, and `cargo xtask check-venues` reconciles the SECRET names
# rather than the tool, so this uses what is already there.
#
# WHAT IT DOES, in the two steps Google documents: self-sign a JWT with the account's own private
# key asserting `target_audience`, then exchange it at the token endpoint under the
# `jwt-bearer` grant for an `id_token` the pool's OIDC provider will accept.
#
# WHAT IT DOES NOT DO: keep anything. The key is read from a path the caller owns and removes, the
# assertion is written to a path the caller owns and removes, and neither ever reaches `argv` - both
# are file paths. The intermediate self-signed JWT lives in a shell variable for one `curl` and
# goes out over `--data-urlencode @-`, never on a command line.
#
#   bigquery-mint-assertion.sh <key.json> <target-audience> <out-file>
set -euo pipefail

if [ "$#" -ne 3 ]; then
    echo "usage: bigquery-mint-assertion.sh <key.json> <target-audience> <out-file>" >&2
    exit 2
fi
key_file="$1"
audience="$2"
out_file="$3"

for tool in openssl curl python3; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "bigquery-mint-assertion: $tool is not on PATH, and this mint needs it" >&2
        exit 1
    }
done

# The two fields this needs out of the key document, read by a parser rather than by `grep`: a
# private key is PEM with embedded newlines and a line-based read would truncate it.
client_email="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["client_email"])' "$key_file")"
token_uri="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1])).get("token_uri","https://oauth2.googleapis.com/token"))' "$key_file")"

# The self-signed assertion. `target_audience` is what makes the exchanged token an ID token FOR the
# pool rather than an access token for Google's own APIs.
signed="$(
    python3 - "$key_file" "$client_email" "$token_uri" "$audience" <<'PY'
import base64, json, os, subprocess, sys, tempfile, time

key_file, client_email, token_uri, audience = sys.argv[1:5]
private_key = json.load(open(key_file))["private_key"]


def segment(value):
    return base64.urlsafe_b64encode(json.dumps(value, separators=(",", ":")).encode()).rstrip(b"=")


now = int(time.time())
head = segment({"alg": "RS256", "typ": "JWT"})
body = segment(
    {
        "iss": client_email,
        "aud": token_uri,
        "target_audience": audience,
        "iat": now,
        "exp": now + 600,
    }
)
payload = head + b"." + body
# ONE signing path, and the key goes to a 0600 file rather than a stream because `openssl dgst
# -sign` takes the key by PATH and the data on stdin - there is no invocation that takes both on
# one stream. The file is created with the mode already narrowed and unlinked in a `finally`, so a
# failure in `openssl` does not leave a private key behind.
with tempfile.NamedTemporaryFile(delete=False) as key_pem:
    os.chmod(key_pem.name, 0o600)
    key_pem.write(private_key.encode())
    key_pem.flush()
    try:
        signature = subprocess.run(
            ["openssl", "dgst", "-sha256", "-sign", key_pem.name, "-binary"],
            input=payload,
            capture_output=True,
            check=True,
        )
    finally:
        os.unlink(key_pem.name)
sys.stdout.write((payload + b"." + base64.urlsafe_b64encode(signature.stdout).rstrip(b"=")).decode())
PY
)"

# The exchange. The assertion goes over stdin rather than in `argv`, and the answer is parsed for
# `id_token` rather than echoed - a failure prints Google's own `error_description` and nothing else.
printf 'grant_type=%s&assertion=%s' "urn:ietf:params:oauth:grant-type:jwt-bearer" "$signed" |
    curl --silent --show-error --fail-with-body --data-binary @- \
        --header 'content-type: application/x-www-form-urlencoded' \
        "$token_uri" |
    python3 -c '
import json, sys
answered = json.load(sys.stdin)
token = answered.get("id_token")
if not token:
    sys.exit("bigquery-mint-assertion: the token endpoint returned no id_token: " + answered.get("error_description", "no reason given"))
sys.stdout.write(token)
' >"$out_file"

umask 077
chmod 600 "$out_file"
echo "bigquery-mint-assertion: minted an assertion for $client_email, $(wc -c <"$out_file") bytes"
