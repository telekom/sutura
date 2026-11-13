#!/usr/bin/env bash
set -euo pipefail
source_sha=a70aa4fbc16a1a7bd7735675f1a2e7419adb771e
action_sha=3bc9bbbc9b832cc5202709f0f5d0112559d2ee0ee4589787b45d87f01b167350
out="$PWD/target/cache-root-observation"
mkdir -p "$out"
exec > >(tee "$out/observation.log") 2>&1
trap 'status=$?; printf "%s\n" "$status" > "$out/exit-status"; git status --porcelain > "$out/final-status"; exit "$status"' EXIT
printf '%s\n' "${GITHUB_SHA:-}" > "$out/helper-sha"
printf '%s\n' "${REQUEST_ID:-}" > "$out/request-id"
sha256sum ../nix/measure-cache-roots.sh ../.github/workflows/format.yml > "$out/helper.sha256"
test "${GITHUB_ACTIONS:-}" = true
test "${REQUEST_ID:-}" = 'cache-root-a70-v1'
test "$(git rev-parse HEAD)" = "$source_sha"
test -z "$(git status --porcelain)"
printf '%s  %s\n' "$action_sha" .github/actions/nix-store-cache/action.yml | sha256sum --check
git rev-parse HEAD > "$out/source-sha"
sha256sum .github/actions/nix-store-cache/action.yml > "$out/action.sha256"
git rev-parse 'HEAD^{tree}' > "$out/source-tree"
nix --version > "$out/nix-version"
system="$(nix config show system)"
printf '%s\n' "$system" > "$out/system"
printf 'phase\telapsed_ns\texit_status\n' > "$out/timing.tsv"
measure() {
  local phase="$1" started ended status=0
  shift
  started="$(date +%s%N)"
  "$@" > "$out/$phase.json" || status=$?
  ended="$(date +%s%N)"
  printf '%s\t%s\t%s\n' "$phase" "$((ended-started))" "$status" >> "$out/timing.tsv"
  return "$status"
}
baseline() {
  local phase="$1"
  measure "$phase-checks" nix eval --json ".#checks.$system" --apply 'checks: map (check: check.drvPath) (builtins.attrValues checks)'
  measure "$phase-apps" nix eval --json ".#apps.$system" --apply 'apps: builtins.concatMap (app: builtins.attrNames (builtins.getContext app.program)) (builtins.attrValues apps)'
  jq -cs add "$out/$phase-checks.json" "$out/$phase-apps.json" > "$out/$phase.json"
}
combined() {
  measure "$1" nix eval --json .#. --apply "outputs:
    map (check: check.drvPath) (builtins.attrValues outputs.checks.\"$system\")
    ++ builtins.concatMap (app: builtins.attrNames (builtins.getContext app.program)) (builtins.attrValues outputs.apps.\"$system\")
  "
}
# Initial input fetching is recorded separately from the warm AB/BA comparison.
baseline cold-baseline
baseline warm-a-baseline
combined warm-a-combined
combined warm-b-combined
baseline warm-b-baseline
for phase in warm-a-baseline warm-a-combined warm-b-combined warm-b-baseline; do
  jq -c . "$out/$phase.json" > "$out/$phase.canonical.json"
  cmp "$out/cold-baseline.json" "$out/$phase.canonical.json"
done
measure archive nix flake archive --dry-run --json
jq -r '.. | .path? // empty' "$out/archive.json" > "$out/archive-roots"
for phase in cold-baseline warm-a-combined warm-b-combined; do
  jq -r '.[]' "$out/$phase.json" > "$out/$phase.roots"
  cat "$out/archive-roots" >> "$out/$phase.roots"
  sort -u "$out/$phase.roots" > "$out/$phase.unique-roots"
done
cmp "$out/cold-baseline.unique-roots" "$out/warm-a-combined.unique-roots"
cmp "$out/cold-baseline.unique-roots" "$out/warm-b-combined.unique-roots"
wc -l "$out/cold-baseline.unique-roots" > "$out/root-count"
sha256sum "$out"/*.json "$out"/*.roots "$out"/*.unique-roots "$out/timing.tsv" > "$out/evidence.sha256"
cat "$out/timing.tsv" "$out/root-count"
printf '%s\n' 'cache roots: exact ordered projections and full unique root sets agree'
