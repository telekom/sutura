#!/bin/sh
# The Kerberos tier: an MIT KDC and a PostgreSQL that admits only GSSAPI-encrypted GSSAPI sign-in,
# in <dir>, on loopback. Needs krb5's `kdb5_util`, `kadmin.local`, `krb5kdc` and postgres's
# `initdb`, `pg_ctl`, `psql` on PATH, and a non-root user (postgres refuses root).
#
#   kerberos-tier.sh <dir>     # then: . <dir>/env
#
# `<dir>/env` exports what a client needs: `KRB5_CONFIG`, `KRB5_CLIENT_KTNAME` (the `sutura`
# principal's keytab, so nothing runs `kinit`), `KRB5CCNAME` (a memory cache) and
# `SUTURA_KERBEROS_TIER_PORT`. The server's principal is `postgres/localhost@SUTURA.TEST`, the
# client's `sutura@SUTURA.TEST`, signing in as role `sutura` (`include_realm=0`).
#
# Unprivileged ports, because the nix build sandbox runs it as a build user. The realm is the
# tier's own: nothing here reads DNS (`dns_lookup_*`, `dns_canonicalize_hostname` and `rdns` off,
# and `qualify_shortname` empty, so a resolver's search domain is never appended to `localhost`).
set -eu
dir="$1"
kdc_port="${SUTURA_KERBEROS_TIER_KDC_PORT:-18088}"
pg_port="${SUTURA_KERBEROS_TIER_PORT:-15432}"
mkdir -p "$dir"
dir="$(cd "$dir" && pwd)"

cat > "$dir/krb5.conf" <<EOF
[libdefaults]
 default_realm = SUTURA.TEST
 dns_lookup_kdc = false
 dns_lookup_realm = false
 dns_canonicalize_hostname = false
 qualify_shortname = ""
 rdns = false
[realms]
 SUTURA.TEST = {
  kdc = 127.0.0.1:$kdc_port
 }
EOF
cat > "$dir/kdc.conf" <<EOF
[kdcdefaults]
 kdc_listen = 127.0.0.1:$kdc_port
 kdc_tcp_listen = 127.0.0.1:$kdc_port
[realms]
 SUTURA.TEST = {
  database_name = $dir/principal
  key_stash_file = $dir/stash
 }
[logging]
 kdc = FILE:$dir/kdc.log
EOF
export KRB5_CONFIG="$dir/krb5.conf" KRB5_KDC_PROFILE="$dir/kdc.conf"
kdb5_util create -s -r SUTURA.TEST -P tier-master-key > /dev/null
for principal in postgres/localhost sutura; do
    kadmin.local -q "addprinc -randkey $principal" > /dev/null 2>&1
done
kadmin.local -q "ktadd -k $dir/server.keytab postgres/localhost" > /dev/null
kadmin.local -q "ktadd -k $dir/client.keytab sutura" > /dev/null
krb5kdc -P "$dir/kdc.pid"

initdb -D "$dir/data" -U postgres --auth=trust > /dev/null
cat >> "$dir/data/postgresql.conf" <<EOF
listen_addresses = '127.0.0.1'
port = $pg_port
unix_socket_directories = '$dir'
krb_server_keyfile = '$dir/server.keytab'
EOF
cat > "$dir/data/pg_hba.conf" <<EOF
local all postgres trust
hostgssenc all all 127.0.0.1/32 gss include_realm=0
EOF
pg_ctl -D "$dir/data" -l "$dir/postgres.log" -w start > /dev/null
psql -h "$dir" -p "$pg_port" -U postgres -qc 'CREATE ROLE sutura LOGIN'

cat > "$dir/env" <<EOF
export KRB5_CONFIG='$dir/krb5.conf'
export KRB5_CLIENT_KTNAME='$dir/client.keytab'
export KRB5CCNAME='MEMORY:sutura-kerberos-tier'
export SUTURA_KERBEROS_TIER_PORT='$pg_port'
EOF
