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
replace(
    "nix/oracle-login-fixtures.sh",
    '''  SUTURA_ORACLE_NATIVE_EXPECT=success cargo test --profile "${SUTURA_ORACLE_CARGO_PROFILE:-dev}" -p sutura-cli --features oracle,rdbms --test served \\
    oracle_native::oracle_native_logins_require_both_strong_services_on_both_production_paths -- --ignored --nocapture''',
    '''  diagnostic_out="$PWD/target/oracle-native-probe"
  for diagnostic_iteration in $(seq 1 30); do
    diagnostic_log="$(mktemp)"
    printf '%s iteration=%s start\\n' "$(date -u --iso-8601=ns)" "$diagnostic_iteration" | tee -a "$diagnostic_out/iterations.tsv"
    set +e
    SUTURA_ORACLE_NATIVE_EXPECT=success timeout --kill-after=10s 60s cargo test --profile "${SUTURA_ORACLE_CARGO_PROFILE:-dev}" -p sutura-cli --features oracle,rdbms --test served \\
      oracle_native::oracle_native_logins_require_both_strong_services_on_both_production_paths -- --ignored --nocapture > "$diagnostic_log" 2>&1
    diagnostic_status=$?
    set -e
    grep -E 'native-control-diagnostic (path=|packet_type=)|unprotected native control packet|test result:|panicked at|oracle_native::.* \\.\\.\\. (ok|FAILED)' "$diagnostic_log" > "$diagnostic_out/iteration-$diagnostic_iteration.log" || true
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
    '''docker logs --tail 200 --timestamps "$name" 2>&1 | awk '{ stamp=$1; for (i=2; i<=NF; i++) if ($i ~ /^(ORA-|TNS-|NI)[0-9]+[:,]?$/) print stamp, $i }' > "$out/container.log" || true
  docker exec "$name" sh -c 'for log in /opt/oracle/diag/tnslsnr/*/listener/trace/listener.log; do tail -n 200 "$log"; done' 2>/dev/null | awk -F ' \\* ' 'NF >= 4 { print $1 " | " $(NF-2) " | " $(NF-1) " | " $NF }' > "$out/listener-status.log" || true''',
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
