#!/usr/bin/env bash
# Look up a singleton queue entry's latest successful ordinary PR run.
set -euo pipefail

sha() { [[ "$1" =~ ^[0-9a-f]{40}$ ]]; }

# The suffix and event base must name the sole parent. Batched or unfamiliar queue shapes run fresh.
[[ "$QUEUE_REF" =~ ^refs/heads/gh-readonly-queue/main/pr-([1-9][0-9]*)-([0-9a-f]{40})$ ]] || exit 0
pr_number=${BASH_REMATCH[1]}
ref_base=${BASH_REMATCH[2]}
sha "$QUEUE_HEAD" && [ "$QUEUE_HEAD" = "$GITHUB_SHA" ] && [ "$QUEUE_BASE" = "$ref_base" ] || exit 0
[ "$(git rev-parse HEAD)" = "$QUEUE_HEAD" ] || exit 0
[ "$(git show -s --format=%P HEAD)" = "$QUEUE_BASE" ] || exit 0

repo="repos/$GITHUB_REPOSITORY"
pr_head=$(gh api "$repo/pulls/$pr_number" |
  jq -er --arg repo "$GITHUB_REPOSITORY" '
    select(.state == "open" and .base.ref == "main" and .base.repo.full_name == $repo
      and .head.repo.full_name == $repo) | .head.sha') || exit 0
sha "$pr_head" || exit 0

# A newer failed, cancelled, or pending run supersedes an older green run.
run=$(gh api "$repo/actions/workflows/ci.yml/runs?event=pull_request&head_sha=$pr_head&per_page=20" |
  jq -ce --arg sha "$pr_head" --argjson pr "$pr_number" '
    [.workflow_runs[] | select(.event == "pull_request" and .head_sha == $sha
      and .path == ".github/workflows/ci.yml")]
    | max_by(.run_number)
    | select(.status == "completed" and .conclusion == "success")
    | select((.pull_requests | length) == 1 and
      (.pull_requests[0] | .number == $pr and .head.sha == $sha and .base.ref == "main"))') || exit 0
run_id=$(jq -er '.id | select(type == "number")' <<< "$run") || exit 0
run_attempt=$(jq -er '.run_attempt | select(type == "number")' <<< "$run") || exit 0
green=$(gh api "$repo/actions/runs/$run_id/jobs?filter=latest&per_page=100" |
  jq -ce --arg sha "$pr_head" --argjson attempt "$run_attempt" '
    select(.total_count == (.jobs | length))
    | .jobs | sort_by(.name) | group_by(.name)
    | map(select(length == 1) | .[0]
      | select(.head_sha == $sha and .run_attempt == $attempt and .status == "completed"
        and .conclusion == "success") | .name)') || exit 0
tested_commit=$(jq -er '
    map(select(test("^QA checkout \\([0-9a-f]{40}\\)$"))) | select(length == 1)
    | .[0] | capture("\\((?<sha>[0-9a-f]{40})\\)").sha' <<< "$green") || exit 0
git cat-file -e "$tested_commit^{commit}" 2>/dev/null ||
  git fetch --no-tags origin "$tested_commit" || exit 0
# Actions' pull_requests base SHA may lag the base used by the tested merge checkout.
tested_parents=$(git show -s --format=%P "$tested_commit") || exit 0
read -r tested_base tested_pr_head extra <<< "$tested_parents"
sha "$tested_base" && [ "$tested_pr_head" = "$pr_head" ] && [ -z "$extra" ] || exit 0
git merge-base --is-ancestor "$tested_base" "$QUEUE_BASE" || exit 0

# Bind the current PR head to this queue tree. A force update or a different merge result
# needs a new run even if the queue ref still carries the same PR number.
expected_tree=$(git merge-tree --write-tree "$QUEUE_BASE" "$pr_head") || exit 0
[ "$(git rev-parse "$QUEUE_HEAD^{tree}")" = "$expected_tree" ] || exit 0

# Evidence is usable only under the same workflow, selector and build policy as the queue base.
# Changes to the evidence-producing workflow itself therefore cannot publish a reusable verdict.
policy=(.github nix xtask devco .cargo Cargo.toml Cargo.lock flake.nix flake.lock rust-toolchain.toml .gitattributes)
git diff --quiet "$QUEUE_BASE" "$tested_commit" -- "${policy[@]}" || exit 0
git diff --quiet "$QUEUE_BASE" "$QUEUE_HEAD" -- "${policy[@]}" || exit 0
jq -e 'index("ci") != null and index("ci-aggregate") != null and index("qa / integration-verdict") != null' <<< "$green" >/dev/null ||
  exit 0

printf 'tested_commit=%s\ngreen_jobs=%s\n' "$tested_commit" "$green" >> "$GITHUB_OUTPUT"
printf 'Using PR #%s run %s as a candidate for suite reuse\n' "$pr_number" "$run_id"

# Causality judges the topic's patch from its merge base. Whitespace-sensitive patch IDs also
# have to match, and control trailers keep their per-commit proof in the queue.
jq -e 'index("causality") != null and index("causality-receipt") != null' <<< "$green" >/dev/null || exit 0
pr_base=$(git merge-base "$tested_base" "$pr_head") || exit 0
for range in "$pr_base..$pr_head" "$QUEUE_BASE..$QUEUE_HEAD"; do
  messages=$(git log --format=%B "$range") || exit 0
  if grep -Eq '^[[:space:]]*(Claim-Cell|Weakens-Test|Live-Cell|Cleanup-Split):' <<< "$messages"; then
    exit 0
  fi
done

patch_id() {
  git diff --binary --full-index --no-ext-diff --no-textconv --no-renames "$2" "$3" -- |
    git patch-id "$1" | awk '{print $1}'
}
for mode in --stable --verbatim; do
  from_pr=$(patch_id "$mode" "$pr_base" "$pr_head") || exit 0
  from_queue=$(patch_id "$mode" "$QUEUE_BASE" "$QUEUE_HEAD") || exit 0
  sha "$from_pr" && [ "$from_pr" = "$from_queue" ] || exit 0
done
printf 'causality_reused=true\n' >> "$GITHUB_OUTPUT"
printf 'Reusing conclusive PR causality for the matching queue patch\n'
