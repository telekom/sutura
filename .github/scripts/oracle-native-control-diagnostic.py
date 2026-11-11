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
for old, stage in [
    ('connection.map_err(read_err)', 'connect'),
    ('connection.execute("SET TRANSACTION READ ONLY", &[]).map_err(read_err)?;', 'read_only'),
    ('connection.query(&self.statement, &params).map_err(read_err)?;', 'query'),
    ('row.map_err(read_err)?', 'rows'),
    ('connection.rollback().map_err(read_err)?;', 'rollback'),
]:
    labeled = old.replace('map_err(read_err)', f'map_err(|on| {{ eprintln!("native-control-diagnostic dictionary_stage={stage}"); read_err(on) }})')
    replace("crates/sutura-catalog-rdbms/src/oracle_reader.rs", old, labeled)
replace(
    "nix/oracle-login-fixtures.sh",
    '''  SUTURA_ORACLE_NATIVE_EXPECT=success cargo test --profile "${SUTURA_ORACLE_CARGO_PROFILE:-dev}" -p sutura-cli --features oracle,rdbms --test served \\
    oracle_native::oracle_native_logins_require_both_strong_services_on_both_production_paths -- --ignored --nocapture''',
    '''  diagnostic_out="$PWD/target/oracle-native-probe/cohort-$SUTURA_ORACLE_DIAGNOSTIC_COHORT"
  for diagnostic_iteration in $(seq 1 100); do
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
    if [ "$diagnostic_status" -ne 0 ]; then
      break
    fi
  done
  return 0''',
)
replace(
    "nix/oracle-native-probe.sh",
    'docker rm -f "$name" >/dev/null 2>&1 || true',
    'timeout 20s docker rm -fv "$name" >/dev/null 2>&1',
)
replace(
    "nix/oracle-native-probe.sh",
    'out="$PWD/target/oracle-native-probe"',
    'out="$PWD/target/oracle-native-probe/cohort-$SUTURA_ORACLE_DIAGNOSTIC_COHORT"',
)
replace(
    "nix/oracle-native-probe.sh",
    'docker logs "$name" > "$out/container.log" 2>&1 || true',
    '''timeout 10s docker logs --tail 200 --timestamps "$name" 2>&1 | awk '{ stamp=$1; for (i=2; i<=NF; i++) if ($i ~ /^(ORA-|TNS-|NI)[0-9]+[:,]?$/) print stamp, $i }' > "$out/container.log" || true
  timeout 10s docker exec "$name" sh -c 'for log in /opt/oracle/diag/tnslsnr/*/listener/trace/listener.log; do tail -n 200 "$log"; done' 2>/dev/null | awk -F '[*]' 'NF >= 6 && $NF ~ /^[[:space:]]*[0-9]+[[:space:]]*$/ { print $1 " | " $(NF-2) " | " $(NF-1) " | " $NF }' > "$out/listener-status.log" || true
  timeout 10s docker exec -i "$name" sh > "$out/server-error-codes.log" 2>/dev/null <<'DIAGNOSTIC' || true
diagnostic_traces=0
for trace in /opt/oracle/diag/rdbms/*/*/trace/*.trc; do
  [ -f "$trace" ] || continue
  diagnostic_traces=$((diagnostic_traces + 1))
  { head -n 400 "$trace"; tail -n 400 "$trace"; } | awk '/^\\*\\*\\* [0-9][0-9][0-9][0-9]-/ { stamp=$2 } /^ERROR #/ { if (match($0, /err=[0-9]+/)) { err=substr($0, RSTART, RLENGTH); if (match($0, /tim=[0-9]+/)) print err, substr($0, RSTART, RLENGTH) } } /ORA-01466/ { print stamp, "ORA-01466" }'
done
printf 'trace_files=%s head_lines=400 tail_lines=400\\n' "$diagnostic_traces"
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
replace(
    "flake.nix",
    "            bash nix/oracle-native-probe.sh\n",
    '            SUTURA_ORACLE_DIAGNOSTIC_COHORT=1 bash nix/oracle-native-probe.sh\n',
)
