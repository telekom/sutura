"""Build the fleet's Oracle fixture from the committed CSV files; no capacity claim."""

import csv
import re
import sys
from pathlib import Path

root = Path(sys.argv[1])
password = Path(sys.argv[2]).read_text().strip()
if re.fullmatch(r"[0-9a-f]{32}", password) is None:
    raise ValueError("fixture password must be 32 lowercase hexadecimal characters")
print(f'CREATE USER "SUTURA" IDENTIFIED BY "{password}";')
print('GRANT CREATE SESSION, CREATE TABLE, UNLIMITED TABLESPACE TO "SUTURA";')
print(
    'CREATE TABLE "SUTURA"."dim_customer_split" ("customer_key" NUMBER(10,0), "customer_id" VARCHAR2(16), "segment" VARCHAR2(16), "region" VARCHAR2(16));'
)
print(
    'CREATE TABLE "SUTURA"."dim_region" ("region" VARCHAR2(16), "sales_area" VARCHAR2(16));'
)
for source, table, keys in [
    (
        "dim_customer.csv",
        "dim_customer_split",
        ["customer_key", "customer_id", "segment", "region"],
    ),
    ("dim_region.csv", "dim_region", ["region", "sales_area"]),
]:
    with (root / source).open(newline="") as handle:
        rows = list(csv.DictReader(handle))
    assert rows, source
    for row in rows:
        cells = [
            str(int(row[key]))
            if key == "customer_key"
            else "'" + row[key].replace("'", "''") + "'"
            for key in keys
        ]
        print(f'INSERT INTO "SUTURA"."{table}" VALUES ({", ".join(cells)});')
print("COMMIT;")
print(
    "BEGIN DBMS_STATS.SET_TABLE_STATS('SUTURA', '\"dim_customer_split\"', numrows => 1200000, avgrlen => 48); END;\n/"
)
# SQL trace is per real source session. It records parse/execute/fetch separately, so a metadata
# describe that fetches zero rows cannot be confused with a completed bucket read.
print("""CREATE OR REPLACE TRIGGER sutura_trace_logon AFTER LOGON ON DATABASE
BEGIN
  IF SYS_CONTEXT('USERENV', 'SESSION_USER') = 'SUTURA' THEN
    EXECUTE IMMEDIATE 'ALTER SESSION SET tracefile_identifier = ''SUTURA_' ||
      REPLACE(SYS_CONTEXT('USERENV', 'IP_ADDRESS'), '.', '_') || '''';
    DBMS_SESSION.SESSION_TRACE_ENABLE(waits => FALSE, binds => FALSE);
  END IF;
END;
/
DECLARE
  failures NUMBER;
BEGIN
  SELECT COUNT(*) INTO failures FROM USER_ERRORS WHERE NAME = 'SUTURA_TRACE_LOGON';
  IF failures > 0 THEN RAISE_APPLICATION_ERROR(-20001, 'fixture trace trigger did not compile'); END IF;
END;
/
""")
