#!/usr/bin/env bash
set -euo pipefail
test "${GITHUB_ACTIONS:-}" = true
test -z "$(git status --porcelain)"
out="$PWD/target/oracle-native-claim-proof"
mkdir -p "$out"
git rev-parse HEAD > "$out/source-sha"
git rev-parse 'HEAD^{tree}' > "$out/source-tree"
export SUTURA_ORACLE_NATIVE_EXPECT=success
cells=(
  oracle_native_logins_require_both_strong_services_on_both_production_paths
  oracle_legacy_logins_reach_both_production_paths_only_with_the_opt_in
)
run_cells() {
  cargo test --profile "${SUTURA_ORACLE_CARGO_PROFILE:-dev}" -p sutura-cli --features oracle,rdbms --test served \
    "$1" -- --ignored --nocapture
}
green() {
  run_cells oracle_native:: 2>&1 | tee "$out/$1.log"
  for cell in "${cells[@]}"; do
    grep -Fxq "test oracle_native::$cell ... ok" "$out/$1.log"
  done
}
active_patch=
cleanup() {
  if [ -n "$active_patch" ]; then
    git apply --reverse "$active_patch"
  fi
}
trap cleanup EXIT
green head
for cell in "${cells[@]}"; do
  patch="devco/claim-mutations/$cell.patch"
  sha256sum "$patch" >> "$out/mutations.sha256"
  git apply --check "$patch"
  git apply "$patch"
  active_patch="$patch"
  git diff --binary > "$out/$cell.patch"
  set +e
  run_cells "oracle_native::$cell" 2>&1 | tee "$out/$cell.log"
  status=${PIPESTATUS[0]}
  set -e
  printf '%s\n' "$status" > "$out/$cell.status"
  git apply --reverse "$patch"
  active_patch=
  test "$status" -ne 0
  grep -q '^test result: FAILED\.' "$out/$cell.log"
  grep -Fxq "test oracle_native::$cell ... FAILED" "$out/$cell.log"
done
test -z "$(git status --porcelain)"
green restored
echo 'Both named Oracle cells pass, fail under their committed wiring mutations, and pass restored.'
