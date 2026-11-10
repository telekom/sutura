#!/usr/bin/env bash
# Installs the same source and dictionary corpus for each verifier, then checks the stored type.
set -euo pipefail
name="$1"
service="$2"
verifier="$3"
export SUTURA_ORACLE_ACCEPTANCE_USER=FIXTURE12
export SUTURA_ORACLE_ACCEPTANCE_PASSWORD="$ORACLE_PASSWORD"
export SUTURA_ORACLE_ACCEPTANCE_SERVICE="$service"
export SUTURA_ORACLE_ACCEPTANCE_VERIFIER="$verifier"
if [ "$verifier" = 10G ]; then
  export SUTURA_ORACLE_ACCEPTANCE_LEGACY_USER=SCOTT
  export SUTURA_ORACLE_ACCEPTANCE_LEGACY_PASSWORD=TIGER
  create_legacy="CREATE USER SCOTT IDENTIFIED BY VALUES 'F894844C34402B67';"
else
  export SUTURA_ORACLE_ACCEPTANCE_LEGACY_USER=FIXTURE11
  export SUTURA_ORACLE_ACCEPTANCE_LEGACY_PASSWORD="$ORACLE_PASSWORD"
  create_legacy="DECLARE
    hash VARCHAR2(40);
  BEGIN
    SELECT RAWTOHEX(STANDARD_HASH(UTL_RAW.CONCAT(UTL_RAW.CAST_TO_RAW('$ORACLE_PASSWORD'), HEXTORAW('00112233445566778899')), 'SHA1')) INTO hash FROM dual;
    EXECUTE IMMEDIATE 'CREATE USER FIXTURE11 IDENTIFIED BY VALUES ''S:' || hash || '00112233445566778899''';
  END;
  /"
fi
legacy_user="$SUTURA_ORACLE_ACCEPTANCE_LEGACY_USER"
docker exec -i "$name" sqlplus -s / as sysdba <<SQL
WHENEVER SQLERROR EXIT FAILURE
SET ECHO OFF VERIFY OFF
ALTER SESSION SET CONTAINER=$service;
CREATE USER FIXTURE12 IDENTIFIED BY "$ORACLE_PASSWORD";
$create_legacy
GRANT CREATE SESSION TO FIXTURE12, $legacy_user;
ALTER USER FIXTURE12 QUOTA UNLIMITED ON USERS;
ALTER USER $legacy_user QUOTA UNLIMITED ON USERS;
CREATE TABLE FIXTURE12.EVENTS (ID NUMBER(10) NOT NULL PRIMARY KEY, PAYLOAD VARCHAR2(2000) NOT NULL, MONTH DATE NOT NULL);
INSERT INTO FIXTURE12.EVENTS SELECT LEVEL, LPAD(TO_CHAR(LEVEL), 2000, 'x'), DATE '2026-06-01' FROM dual CONNECT BY LEVEL <= 128;
CREATE TABLE $legacy_user.EVENTS AS SELECT * FROM FIXTURE12.EVENTS;
CREATE TABLE $legacy_user.COLUMNS (
  environment VARCHAR2(64), catalog_name VARCHAR2(64), schema_name VARCHAR2(64),
  table_name VARCHAR2(64), model_name VARCHAR2(64), table_description VARCHAR2(256),
  column_name VARCHAR2(64), column_ordinal NUMBER(10), column_type VARCHAR2(64),
  column_description VARCHAR2(256), is_primary_key NUMBER(1), is_deleted NUMBER(1));
INSERT INTO $legacy_user.COLUMNS VALUES ('test', NULL, 'FIXTURE12', 'EVENTS', 'events',
  'The login fixture.', 'ID', 1, 'NUMBER', 'An event key.', 1, 0);
INSERT INTO $legacy_user.COLUMNS VALUES ('test', NULL, 'FIXTURE12', 'EVENTS', 'events',
  'The login fixture.', 'PAYLOAD', 2, 'VARCHAR2', 'A multi-record payload.', 0, 0);
INSERT INTO $legacy_user.COLUMNS VALUES ('test', NULL, 'FIXTURE12', 'EVENTS', 'events',
  'The login fixture.', 'MONTH', 3, 'DATE', 'The fixture month.', 0, 0);
CREATE TABLE FIXTURE12.COLUMNS AS SELECT * FROM $legacy_user.COLUMNS;
COMMIT;
SELECT username, password_versions FROM dba_users WHERE username IN ('FIXTURE12', '$legacy_user');
DECLARE
  versions VARCHAR2(100);
BEGIN
  SELECT TRIM(password_versions) INTO versions FROM dba_users WHERE username='$legacy_user';
  IF versions <> '$verifier' OR versions IS NULL THEN
    RAISE_APPLICATION_ERROR(-20001, 'legacy account does not hold only the expected verifier');
  END IF;
END;
/
EXIT
SQL

# Oracle refuses a read-only snapshot older than the fixture DDL (ORA-01466).
readable=false
for _ in $(seq 1 30); do
  if docker exec -i "$name" sqlplus -s / as sysdba <<SQL
WHENEVER SQLERROR EXIT FAILURE
ALTER SESSION SET CONTAINER=$service;
SET TRANSACTION READ ONLY;
SELECT COUNT(*) FROM $legacy_user.COLUMNS;
ROLLBACK;
EXIT
SQL
  then
    readable=true
    break
  fi
  sleep 2
done
test "$readable" = true
if [ "$SUTURA_ORACLE_ACCEPTANCE_MODE" = native ] && [ "${SUTURA_ORACLE_BASE_PROOF:-0}" != 1 ]; then
  SUTURA_ORACLE_NATIVE_EXPECT=success cargo test --profile "${SUTURA_ORACLE_CARGO_PROFILE:-dev}" -p sutura-cli --features oracle,rdbms --test served \
    oracle_native::oracle_native_logins_require_both_strong_services_on_both_production_paths -- --ignored --nocapture
fi
oracle_test_filter=oracle_native::oracle_legacy_logins_reach_both_production_paths_only_with_the_opt_in
if [ "${SUTURA_ORACLE_BASE_PROOF:-0}" = 1 ]; then
  oracle_test_filter=oracle_native::
fi
cargo test --profile "${SUTURA_ORACLE_CARGO_PROFILE:-dev}" -p sutura-cli --features oracle,rdbms --test served \
  "$oracle_test_filter" -- --ignored --nocapture
