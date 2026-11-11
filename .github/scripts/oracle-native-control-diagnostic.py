from pathlib import Path


def replace(path, old, new):
    file = Path(path)
    text = file.read_text()
    assert text.count(old) == 1, path
    file.write_text(text.replace(old, new))


replace(
    "vendor/rust-oracledb/src/transport.rs",
    'return Err(Error::native_encryption_refused("unprotected native control packet"));',
    '''eprintln!(
                "native-control-diagnostic packet_type={} length={} residual={} full_header={}",
                packet.packet_type, self.last_packet_bytes, self.residual_bytes, self.full_packet_size,
            );
            return Err(Error::native_encryption_refused("unprotected native control packet"));''',
)
replace(
    "crates/sutura-cli/tests/served/oracle_native.rs",
    'let case = format!("oracle-native-{expected}-{path}");',
    '''let case = format!("oracle-native-{expected}-{path}");
        eprintln!("native-control-diagnostic path={path} expected={expected}");''',
)
for old, new in [
    ('let connection = self.connect()?;', 'eprintln!("native-control-diagnostic dictionary_stage=connect");\n        let connection = self.connect()?;\n        eprintln!("native-control-diagnostic dictionary_stage=read_only");'),
    ('let binds = self.binds();', 'eprintln!("native-control-diagnostic dictionary_stage=query");\n        let binds = self.binds();'),
    ('let dictionary = assemble(', 'eprintln!("native-control-diagnostic dictionary_stage=rows");\n        let dictionary = assemble('),
    ('connection.rollback().map_err(read_err)?;', 'eprintln!("native-control-diagnostic dictionary_stage=rollback");\n        connection.rollback().map_err(read_err)?;'),
]:
    replace("crates/sutura-catalog-rdbms/src/oracle_reader.rs", old, new)
replace(
    "nix/oracle-login-fixtures.sh",
    '''  SUTURA_ORACLE_NATIVE_EXPECT=success cargo test --profile "${SUTURA_ORACLE_CARGO_PROFILE:-dev}" -p sutura-cli --features oracle,rdbms --test served \\
    oracle_native::oracle_native_logins_require_both_strong_services_on_both_production_paths -- --ignored --nocapture''',
    '''  diagnostic_out="$PWD/target/oracle-native-probe"
  for diagnostic_iteration in $(seq 1 6); do
    diagnostic_variant=baseline
    diagnostic_modern=''
    if [ "$((diagnostic_iteration % 2))" -eq 0 ]; then
      diagnostic_variant=modern-readiness
      diagnostic_modern='SELECT COUNT(*) FROM FIXTURE12.COLUMNS;'
    fi
    printf '%s iteration=%s variant=%s cold-ddl\\n' "$(date -u --iso-8601=ns)" "$diagnostic_iteration" "$diagnostic_variant" | tee -a "$diagnostic_out/cold-rounds.tsv"
    timeout 20s docker exec -i "$name" sqlplus -s / as sysdba <<SQL
WHENEVER SQLERROR EXIT FAILURE
SET ECHO OFF VERIFY OFF
ALTER SESSION SET CONTAINER=$service;
DROP TABLE FIXTURE12.COLUMNS PURGE;
CREATE TABLE FIXTURE12.COLUMNS AS SELECT * FROM $legacy_user.COLUMNS;
EXIT
SQL
    readable=false
    for diagnostic_readiness in $(seq 1 30); do
      diagnostic_probe_log="$(mktemp)"
      set +e
      timeout 20s docker exec -i "$name" sqlplus -s / as sysdba > "$diagnostic_probe_log" 2>&1 <<SQL
WHENEVER SQLERROR EXIT FAILURE
ALTER SESSION SET CONTAINER=$service;
SET TRANSACTION READ ONLY;
SELECT COUNT(*) FROM $legacy_user.COLUMNS;
$diagnostic_modern
ROLLBACK;
EXIT
SQL
      diagnostic_probe_status=$?
      set -e
      grep -oE 'ORA-[0-9]+' "$diagnostic_probe_log" | sed "s/^/iteration=$diagnostic_iteration variant=$diagnostic_variant /" | tee -a "$diagnostic_out/readiness-codes.log" || true
      rm -f "$diagnostic_probe_log"
      printf '%s iteration=%s variant=%s readiness=%s exit=%s\\n' "$(date -u --iso-8601=ns)" "$diagnostic_iteration" "$diagnostic_variant" "$diagnostic_readiness" "$diagnostic_probe_status" | tee -a "$diagnostic_out/cold-rounds.tsv"
      if [ "$diagnostic_probe_status" -eq 0 ]; then
        readable=true
        break
      fi
      sleep 2
    done
    test "$readable" = true
    diagnostic_log="$(mktemp)"
    printf '%s iteration=%s start\\n' "$(date -u --iso-8601=ns)" "$diagnostic_iteration" | tee -a "$diagnostic_out/iterations.tsv"
    set +e
    SUTURA_ORACLE_NATIVE_EXPECT=success timeout --kill-after=10s 60s cargo test --profile "${SUTURA_ORACLE_CARGO_PROFILE:-dev}" -p sutura-cli --features oracle,rdbms --test served \\
      oracle_native::oracle_native_logins_require_both_strong_services_on_both_production_paths -- --ignored --nocapture > "$diagnostic_log" 2>&1
    diagnostic_status=$?
    set -e
    grep -E 'native-control-diagnostic (path=|packet_type=|dictionary_stage=)|unprotected native control packet|test result:|panicked at|oracle_native::.* \\.\\.\\. (ok|FAILED)' "$diagnostic_log" > "$diagnostic_out/iteration-$diagnostic_iteration.log" || true
    rm -f "$diagnostic_log"
    cat "$diagnostic_out/iteration-$diagnostic_iteration.log"
    printf '%s iteration=%s exit=%s\\n' "$(date -u --iso-8601=ns)" "$diagnostic_iteration" "$diagnostic_status" | tee -a "$diagnostic_out/iterations.tsv"
    if [ "$diagnostic_status" -eq 124 ] || [ "$diagnostic_status" -eq 137 ]; then
      break
    fi
  done
  return 0''',
)
replace(
    "nix/oracle-native-probe.sh",
    'docker logs "$name" > "$out/container.log" 2>&1 || true',
    '''timeout 10s docker logs --tail 200 --timestamps "$name" 2>&1 | awk '{ stamp=$1; for (i=2; i<=NF; i++) if ($i ~ /^(ORA-|TNS-|NI)[0-9]+[:,]?$/) print stamp, $i }' > "$out/container.log" || true
  timeout 10s docker exec "$name" sh -c 'for log in /opt/oracle/diag/tnslsnr/*/listener/trace/listener.log; do tail -n 200 "$log"; done' 2>/dev/null | awk -F '[*]' 'NF >= 6 && $NF ~ /^[[:space:]]*[0-9]+[[:space:]]*$/ { print $1 " | " $(NF-2) " | " $(NF-1) " | " $NF }' > "$out/listener-status.log" || true
  timeout 10s docker exec -i "$name" sh > "$out/server-error-codes.log" 2>/dev/null <<'DIAGNOSTIC' || true
for trace in /opt/oracle/diag/rdbms/*/*/trace/*.trc; do
  { head -n 400 "$trace"; tail -n 400 "$trace"; } | awk '/^\\*\\*\\* [0-9][0-9][0-9][0-9]-/ { stamp=$2 } /ORA-01466/ { print stamp, "ORA-01466" }'
done
DIAGNOSTIC''',
)
replace(
    "nix/oracle-native-probe.sh",
    "SET PAGESIZE 100 LINESIZE 200",
    "ALTER SYSTEM SET EVENTS '1466 trace name errorstack level 1';\nSET PAGESIZE 100 LINESIZE 200",
)
replace(
    "nix/oracle-native-probe.sh",
    'bash nix/oracle-native-cells.sh "$name"',
    ': # The diagnostic runs only the repeated SUCCESS cell above.',
)
replace(
    "flake.nix",
    "            bash nix/oracle-legacy-probe.sh",
    "            : # The diagnostic runs only the native probe.",
)
