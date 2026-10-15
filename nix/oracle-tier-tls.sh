#!/usr/bin/env bash
# The Oracle tier's test PKI: a CA, and the server's auto-login wallet holding a leaf it signed.
# `just dev-up-oracle` and `nix run .#oracle-acceptance` run this before the compose `oracle`
# service starts; the service mounts `wallet/` and serves TCPS over it (`nix/oracle-tier-tcps.sh`),
# and the cells read `ca.pem` as their only `transport_anchors`. Generated, never tracked.
#
# Why a second image: the pinned `gvenzl/oracle-free` image ships `orapki` with its Java runtime
# removed, and its listener opens only an `ewallet.p12` (whose password it is never given) or an
# Oracle `cwallet.sso`, which only `orapki` writes - a PKCS#12 `openssl` wrote is refused
# (ORA-29113). So `nix/oracle-tier-wallet.sh` runs once in Oracle's own image, whose `orapki` runs.
#
# A wallet still valid for a day is kept, so a tier already running keeps the CA it was started with.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
out="$root/target/oracle-tier-tls"
# PINNED BY DIGEST for the `oracle` service's reason in `compose.services.yaml`, at the release that
# service pins.
image="container-registry.oracle.com/database/free:23.26.3.0@sha256:f988b0c04c4c386cd306a2a914c0d7a9702d83acc31b064a28ad8eb6278a8fba"

if [ -s "$out/wallet/cwallet.sso" ] && openssl x509 -checkend 86400 -noout -in "$out/ca.pem" >/dev/null 2>&1; then
  echo "oracle-tier-tls: keeping the wallet in $out"
  exit 0
fi
mkdir -p "$out/wallet"
rm -f "$out/ca.pem" "$out/wallet/"*
docker run --rm --user "$(id -u):$(id -g)" -e HOME=/out \
  -v "$root/nix/oracle-tier-wallet.sh:/oracle-tier-wallet.sh:ro" -v "$out:/out" \
  --entrypoint bash "$image" /oracle-tier-wallet.sh
echo "oracle-tier-tls: wrote $out/ca.pem and $out/wallet"
