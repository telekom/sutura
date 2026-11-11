#!/usr/bin/env bash
# The served split cell uses the Oracle tier the acceptance task already provisioned. The optional
# hosted mutation pass records named assertion failures, then proves the restored source again.
set -euo pipefail
cell=an_oracle_lookup_split_across_readers_equals_the_whole_and_refuses_uncertified_readers
run_cell() {
  cargo test "$@" -p sutura-cli --features oracle,distributed --test served \
    "oracle_split::$cell" -- --exact --ignored --nocapture
}
if [ "${SUTURA_ORACLE_SPLIT_CLAIM_PROOF:-0}" != 1 ]; then
  exec cargo test "$@" -p sutura-cli --features oracle,distributed --test served \
    "oracle_split::$cell" -- --exact --ignored --nocapture
fi
test "${GITHUB_ACTIONS:-}" = true
test -z "$(git status --porcelain)"
out="$PWD/target/oracle-split-claim-proof"
mkdir -p "$out"
git rev-parse HEAD > "$out/source-sha"
git rev-parse 'HEAD^{tree}' > "$out/source-tree"
active_patch=
cleanup() {
  if [ -n "$active_patch" ]; then
    git apply --reverse "$active_patch"
  fi
}
trap cleanup EXIT
green() {
  local phase="$1"
  shift
  run_cell "$@" 2>&1 | tee "$out/$phase.log"
  grep -Fxq "test oracle_split::$cell ... ok" "$out/$phase.log"
}
green head "$@"
for mutation in whole reader; do
  if [ "$mutation" = whole ]; then
    patch="devco/claim-mutations/$cell.patch"
    assertion='a granted Oracle lookup with no resolved reader must refuse'
  else
    patch=nix/oracle-split-reader.patch
    assertion='the granted Oracle lookup must answer through its reading workers'
  fi
  sha256sum "$patch" >> "$out/mutations.sha256"
  git apply --check "$patch"
  git apply "$patch"
  active_patch="$patch"
  git diff --binary > "$out/$mutation.patch"
  set +e
  run_cell "$@" 2>&1 | tee "$out/$mutation.log"
  status=${PIPESTATUS[0]}
  set -e
  printf '%s\n' "$status" > "$out/$mutation.status"
  git apply --reverse "$patch"
  active_patch=
  test "$status" -ne 0
  grep -q '^test result: FAILED\.' "$out/$mutation.log"
  grep -Fxq "test oracle_split::$cell ... FAILED" "$out/$mutation.log"
  grep -Eq 'panicked at crates/sutura-cli/tests/served/oracle_split.rs:[0-9]+:[0-9]+' "$out/$mutation.log"
  grep -Fq "$assertion" "$out/$mutation.log"
done
test -z "$(git status --porcelain)"
green restored "$@"
echo 'The served Oracle split cell passes, fails by assertion under each committed mutation, and passes restored.'
