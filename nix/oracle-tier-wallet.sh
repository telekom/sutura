#!/usr/bin/env bash
# Runs inside Oracle's own database image, for `nix/oracle-tier-tls.sh`: writes `/out/ca.pem` and
# the server's auto-login wallet `/out/wallet`. The key pair is `orapki`'s own, because the listener
# cannot read a key bag `openssl` wrote; `openssl` signs it, since only it writes the IP SAN the
# cells verify `127.0.0.1` against. Thirty days, like the postgres tier's PKI.
set -euo pipefail

orapki="$ORACLE_HOME/bin/orapki"
work="$(mktemp -d)"
cd "$work"
password="$(openssl rand -hex 16)"

openssl genrsa -out ca.key 2048
# Its own config: the image's default one adds a second `basicConstraints`, which no verifier accepts.
printf '%s\n' '[req]' 'distinguished_name=dn' 'x509_extensions=ca' 'prompt=no' '[dn]' 'CN=sutura-oracle-tier-ca' \
  '[ca]' 'basicConstraints=critical,CA:TRUE' 'keyUsage=critical,keyCertSign,cRLSign' 'subjectKeyIdentifier=hash' > ca.cnf
openssl req -x509 -new -key ca.key -out ca.pem -days 30 -sha256 -config ca.cnf

"$orapki" wallet create -wallet /out/wallet -pwd "$password" -auto_login
"$orapki" wallet add -wallet /out/wallet -dn "CN=oracle-tier" -keysize 2048 -pwd "$password"
"$orapki" wallet export -wallet /out/wallet -dn "CN=oracle-tier" -request server.csr -pwd "$password"
printf '%s\n' 'basicConstraints=critical,CA:FALSE' 'keyUsage=critical,digitalSignature,keyEncipherment' \
  'extendedKeyUsage=serverAuth' 'subjectAltName=DNS:localhost,IP:127.0.0.1' 'authorityKeyIdentifier=keyid' > server.ext
openssl x509 -req -in server.csr -CA ca.pem -CAkey ca.key -CAcreateserial -out server.pem -days 30 -sha256 -extfile server.ext
"$orapki" wallet add -wallet /out/wallet -trusted_cert -cert ca.pem -pwd "$password"
"$orapki" wallet add -wallet /out/wallet -user_cert -cert server.pem -pwd "$password"

rm -f /out/wallet/*.lck
cp ca.pem /out/ca.pem
# The listener runs as the image's `oracle` user, not this one.
chmod 755 /out/wallet
chmod 644 /out/ca.pem /out/wallet/*
