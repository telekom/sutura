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
    '''  diagnostic_out="$PWD/target/oracle-native-probe/cohort-$SUTURA_ORACLE_DIAGNOSTIC_COHORT"
  diagnostic_iteration=1
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
  { head -n 400 "$trace"; tail -n 400 "$trace"; } | awk '/^ERROR #/ { if (match($0, /err=[0-9]+/)) { err=substr($0, RSTART, RLENGTH); if (match($0, /tim=[0-9]+/)) print err, substr($0, RSTART, RLENGTH) } }'
done
printf 'trace_files=%s head_lines=400 tail_lines=400\\n' "$diagnostic_traces"
DIAGNOSTIC''',
)
replace(
    "nix/oracle-native-probe.sh",
    "SET PAGESIZE 100 LINESIZE 200",
    "EXEC DBMS_MONITOR.SERV_MOD_ACT_TRACE_ENABLE(service_name => SYS_CONTEXT('USERENV', 'SERVICE_NAME'), waits => FALSE, binds => FALSE);\nSET PAGESIZE 100 LINESIZE 200",
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
    '''            for diagnostic_cohort in 1 2 3; do
              SUTURA_ORACLE_DIAGNOSTIC_COHORT="$diagnostic_cohort" bash nix/oracle-native-probe.sh
            done
''',
)
