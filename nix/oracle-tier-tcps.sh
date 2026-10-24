#!/bin/bash
# A start-up script of the compose `oracle` service: adds a TCPS listener on 2484 over the wallet
# `nix/oracle-tier-tls.sh` wrote and the service mounts at /opt/oracle/tls, then asks whether the
# image accepts an OAuth 2.0 issuer. The image's healthcheck holds until this returns, and a failure
# here stops the container, so a healthy tier serves TCPS and trusts no issuer.
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

# `identity_provider_type = OAUTH2` is the configurable provider a database needs before it accepts
# an end-user security context token from an issuer such as the Keycloak tier. The pinned image
# answers `ORA-00096`. Any other answer fails this script, so the `oracle-tier` job goes red in the
# change that brings in an image that answers otherwise: that change is where the end-user
# context's live success cell gets built. The grant is the privilege the end-user context's
# acceptance cell runs its fixture user under.
answer=$(echo 'ALTER SYSTEM SET identity_provider_type = OAUTH2 SCOPE = MEMORY;' | sqlplus -s / as sysdba)
case "$answer" in
  *ORA-00096*)
    echo "oracle-tier-tcps: identity_provider_type refuses OAUTH2 (ORA-00096), so this image trusts no OAuth 2.0 issuer"
    ;;
  *)
    echo 'ALTER SYSTEM SET identity_provider_type = NONE SCOPE = MEMORY;' | sqlplus -s / as sysdba > /dev/null
    echo "oracle-tier-tcps: identity_provider_type answered OAUTH2 with: $answer" >&2
    echo "oracle-tier-tcps: this image may accept an OAuth 2.0 issuer - build the end-user context's live success cell for it" >&2
    exit 1
    ;;
esac
printf '%s\n' 'WHENEVER SQLERROR EXIT FAILURE' 'ALTER SESSION SET CONTAINER = FREEPDB1;' \
  "GRANT CREATE END USER SECURITY CONTEXT TO ${APP_USER};" | sqlplus -s / as sysdba
