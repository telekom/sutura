#!/usr/bin/env bash
# Computes the JWKS `test-infra/pulumi/google/__main__.py`'s served-proof provider needs, from a
# PEM RSA private key - run ONCE, by the owner, OFFLINE. The private key never reaches this
# script's output and never reaches pulumi: what this prints is `served_proof_jwks_json` for
# `Pulumi.<stack>.yaml`, and the key itself becomes the `SUTURA_SERVED_PROOF_SIGNING_KEY` CI
# secret that `nix/served-proof-tier.nix` passes to Keycloak's own `rsa` key provider.
#
# Usage: nix/served-proof-jwks.sh <private-key.pem>
#
# The `kid` below is a LITERAL, not read from the key: `test-infra/pulumi/google/__main__.py` and
# `nix/served-proof-tier.nix` both hardcode the same string, because neither file can read the
# other's source, and the pool has to agree with the realm about which key a token was signed
# with. Changing it means editing all three by hand.
set -euo pipefail
key="${1:?usage: nix/served-proof-jwks.sh <private-key.pem>}"
kid="served-proof-signing-key-1"

# `openssl rsa -text` rather than parsing DER by hand: it already prints `modulus:` and
# `publicExponent:` as decimal/hex text, which is the one openssl invocation this needs. Captured
# into a variable and handed to python as an ARGUMENT, not over stdin - stdin is not free to
# reuse for it once a caller might want to pipe the key in from elsewhere.
key_text="$(openssl rsa -in "$key" -noout -text)"

python3 -c '
import base64
import json
import re
import sys

def b64url(raw: bytes) -> str:
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode("ascii")

text, kid = sys.argv[1], sys.argv[2]
modulus_block = re.search(r"modulus:\s*\n((?:\s+[0-9a-f:]+\n)+)", text)
exponent = re.search(r"publicExponent:\s*(\d+)", text)
if not modulus_block or not exponent:
    sys.exit("served-proof-jwks: openssl -text carried no modulus/publicExponent - is this an RSA key?")
modulus_hex = re.sub(r"[\s:]", "", modulus_block.group(1))
n = bytes.fromhex(modulus_hex)
# openssl prints a leading 00 sign byte whenever the real modulus'\''s high bit is set - strip it,
# or the encoded value is one byte wider than the key size it names.
if n and n[0] == 0:
    n = n[1:]
exponent_value = int(exponent.group(1))
e = exponent_value.to_bytes((exponent_value.bit_length() + 7) // 8, "big")
jwk = {"kty": "RSA", "use": "sig", "alg": "RS256", "kid": kid, "n": b64url(n), "e": b64url(e)}
print(json.dumps({"keys": [jwk]}))
' "$key_text" "$kid"
