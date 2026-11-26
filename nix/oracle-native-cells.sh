#!/usr/bin/env bash
set -euo pipefail
name="$1"
native_cell() {
  SUTURA_ORACLE_NATIVE_EXPECT="$1" cargo test --profile "${SUTURA_ORACLE_CARGO_PROFILE:-dev}" -p sutura-cli --features oracle,rdbms --test served \
    oracle_native::oracle_native_logins_require_both_strong_services_on_both_production_paths -- --ignored --nocapture
}
# Both strong services are still required here - the sed calls below weaken them for the two
# cells after this one (#2084: a server-failing statement over native encryption surfaces its own
# ORA code, not the generic native-encryption refusal the unprotected break marker used to produce).
cargo test --profile "${SUTURA_ORACLE_CARGO_PROFILE:-dev}" -p sutura-cli --features oracle,rdbms --test served \
  oracle_native::a_server_failing_statement_over_native_encryption_logs_the_ora_code -- --ignored --nocapture
docker exec "$name" bash -c 'sed -i "s/ENCRYPTION_TYPES_SERVER=(AES256)/ENCRYPTION_TYPES_SERVER=(3DES168)/" "$ORACLE_BASE_HOME/network/admin/sqlnet.ora"'
native_cell encryption
docker exec "$name" bash -c 'sed -i "s/ENCRYPTION_TYPES_SERVER=(3DES168)/ENCRYPTION_TYPES_SERVER=(AES256)/;s/CRYPTO_CHECKSUM_TYPES_SERVER=(SHA256)/CRYPTO_CHECKSUM_TYPES_SERVER=(SHA1)/" "$ORACLE_BASE_HOME/network/admin/sqlnet.ora"'
native_cell integrity
