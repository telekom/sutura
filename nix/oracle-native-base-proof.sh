#!/usr/bin/env bash
set -euo pipefail
test "${GITHUB_ACTIONS:-}" = true
base="$1"
[[ "$base" =~ ^[0-9a-f]{40}$ ]]
head_root="$(git rev-parse --show-toplevel)"
head="$(git rev-parse HEAD)"
test "$base" != "$head"
git merge-base --is-ancestor "$base" "$head"
out="$head_root/target/oracle-native-base-proof"
mkdir -p "$out"
printf '%s\n' "$head" > "$out/source-sha"
git rev-parse 'HEAD^{tree}' > "$out/source-tree"
printf '%s\n' "$base" > "$out/base-sha"
git rev-parse "$base^{tree}" > "$out/base-tree"
base_root="$(mktemp -d "$head_root/target/oracle-native-base-XXXXXX")"
cleanup() {
  git -C "$head_root" worktree remove --force "$base_root"
}
git worktree add --detach "$base_root" "$base"
trap cleanup EXIT
test ! -e "$base_root/crates/sutura-cli/tests/served/oracle_native.rs"
files=(
  crates/sutura-cli/tests/served/oracle_native.rs
  nix/oracle-native-probe.sh
  nix/oracle-public-dh-probe.py
  nix/oracle-login-fixtures.sh
  nix/oracle-native-cells.sh
)
sha256sum "${files[@]}" > "$out/injected-files.sha256"
for file in "${files[@]}"; do
  cp "$head_root/$file" "$base_root/$file"
  git -C "$base_root" add -N "$file"
done
cat >> "$base_root/crates/sutura-cli/tests/served.rs" <<'RUST'

#[cfg(unix)]
#[cfg(test)]
#[cfg(all(feature = "oracle", feature = "rdbms"))]
#[path = "served/oracle_native.rs"]
mod oracle_native;
RUST
git -C "$base_root" diff --binary > "$out/injected.patch"
export SUTURA_ORACLE_BASE_PROOF=1
export SUTURA_ORACLE_NATIVE_EXPECT=success
cd "$base_root"
set +e
bash nix/oracle-native-probe.sh 2>&1 | tee "$out/base.log"
status=${PIPESTATUS[0]}
set -e
printf '%s\n' "$status" > "$out/base-status"
cp -R target/oracle-native-probe "$out/server"
test "$status" -ne 0
grep -q '^test result: FAILED\.' "$out/base.log"
for cell in \
  oracle_legacy_logins_reach_both_production_paths_only_with_the_opt_in \
  oracle_native_logins_require_both_strong_services_on_both_production_paths; do
  grep -Fxq "test oracle_native::$cell ... FAILED" "$out/base.log"
done
echo 'Both named Oracle product cells fail by assertion against the recorded base.'
