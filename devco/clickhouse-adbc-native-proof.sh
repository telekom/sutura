#!/usr/bin/env bash
set -euo pipefail

# Hosted-only proof for the vendored ClickHouse ADBC transport. The source checkout is
# deliberately separate from this helper checkout: every mutation is applied, measured,
# and restored in the source tree, while the locked Nix check remains the only test runner.

source_sha=${SOURCE_SHA:?}
source_tree=${SOURCE_TREE:?}
out=${PROOF_OUT:?}
mkdir -p "$out"

test "$(git rev-parse HEAD)" = "$source_sha"
test "$(git rev-parse 'HEAD^{tree}')" = "$source_tree"
test -z "$(git status --porcelain=v1)"
index_before=$(git write-tree)
status_before=$(git status --porcelain=v1)
trap 'test "$(git write-tree)" = "$index_before"; test "$(git status --porcelain=v1)" = "$status_before"' EXIT

check=.#checks.x86_64-linux.vendored-clickhouse-adbc
run_check() {
  local label=$1
  set +e
  nix build "$check" -L >"$out/$label.log" 2>&1
  local status=$?
  set -e
  printf '%s\n' "$status" >"$out/$label.exit"
  return "$status"
}

run_check baseline
printf '%s\n' "$source_sha" >"$out/source-sha.txt"
printf '%s\n' "$source_tree" >"$out/source-tree.txt"
printf '%s\n' "$index_before" >"$out/index-tree.txt"

mutate_and_check() {
  local label=$1 path=$2 before=$3 after=$4
  test "$(git rev-parse HEAD)" = "$source_sha"
  test "$(git rev-parse 'HEAD^{tree}')" = "$source_tree"
  test -z "$(git status --porcelain=v1)"
  python3 - "$path" "$before" "$after" <<'PY'
from pathlib import Path
import sys
path, before, after = sys.argv[1:]
file = Path(path)
text = file.read_text()
assert text.count(before) == 1, (path, text.count(before))
file.write_text(text.replace(before, after, 1))
PY
  if run_check "$label"; then
    echo "mutation unexpectedly passed: $label" >&2
    return 1
  fi
  grep -F 'a_custom_tls_connection_answers_and_retires_its_idle_socket' "$out/$label.log"
  grep -F 'FAILED' "$out/$label.log"
  git checkout -- "$path"
  test "$(git rev-parse HEAD)" = "$source_sha"
  test "$(git rev-parse 'HEAD^{tree}')" = "$source_tree"
  test -z "$(git status --porcelain=v1)"
}

mutate_and_check no-enforce-http vendor/adbc_clickhouse/src/tls.rs \
  $'        http.enforce_http(false);\n' ''
mutate_and_check no-pool-timer vendor/adbc_clickhouse/src/lib.rs \
  $'                    .pool_timer(hyper_util::rt::TokioTimer::new())\n' ''
mutate_and_check no-pool-idle-timeout vendor/adbc_clickhouse/src/lib.rs \
  $'                    .pool_idle_timeout(std::time::Duration::from_secs(2))\n' ''

run_check restored
printf '%s\n' "$(git write-tree)" >"$out/restored-index-tree.txt"
test "$(git write-tree)" = "$index_before"
test "$(git rev-parse 'HEAD^{tree}')" = "$source_tree"
test -z "$(git status --porcelain=v1)"
