#!/bin/bash
# A start-up script of the compose `oracle` service: adds a TCPS listener on 2484 over the wallet
# `nix/oracle-tier-tls.sh` wrote and the service mounts at /opt/oracle/tls. The image's healthcheck
# holds until this returns, and a failure here stops the container, so a healthy tier serves TCPS.
set -euo pipefail

if [ ! -s /opt/oracle/tls/cwallet.sso ]; then
  echo "oracle-tier-tcps: no wallet at /opt/oracle/tls - run \`just dev-up-oracle\`, which writes it" >&2
  exit 1
fi
admin="${ORACLE_BASE_HOME}/network/admin"
wallet='WALLET_LOCATION = (SOURCE = (METHOD = FILE)(METHOD_DATA = (DIRECTORY = /opt/oracle/tls)))
SSL_CLIENT_AUTHENTICATION = FALSE'
cat > "$admin/listener.ora" <<ORA
LISTENER =
  (DESCRIPTION_LIST =
    (DESCRIPTION = (ADDRESS = (PROTOCOL = IPC)(KEY = EXTPROC_FOR_${ORACLE_SID})))
    (DESCRIPTION = (ADDRESS = (PROTOCOL = TCP)(HOST=)(PORT = 1521)))
    (DESCRIPTION = (ADDRESS = (PROTOCOL = TCPS)(HOST=)(PORT = 2484)))
  )

DEFAULT_SERVICE_LISTENER = ${ORACLE_SID}
$wallet
ORA
grep -q '^WALLET_LOCATION' "$admin/sqlnet.ora" || printf '%s\n' "$wallet" >> "$admin/sqlnet.ora"
# `lsnrctl reload` keeps the endpoints the listener started with, so a new address needs a restart.
lsnrctl stop
lsnrctl start
echo 'ALTER SYSTEM REGISTER;' | sqlplus -s / as sysdba
