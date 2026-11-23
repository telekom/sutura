#!/usr/bin/env bash
# Find the successful merge-queue verdict for the exact commit now on main.
set -euo pipefail

repo="repos/$GITHUB_REPOSITORY"
sha="$GITHUB_SHA"
[ "$(git rev-parse HEAD)" = "$sha" ] || exit 0

run=$(gh api "$repo/actions/workflows/ci.yml/runs?event=merge_group&head_sha=$sha&per_page=20" |
  jq -ce --arg sha "$sha" '
    [.workflow_runs[] | select(.event == "merge_group" and .head_sha == $sha
      and .path == ".github/workflows/ci.yml")]
    | max_by(.run_number)
    | select(.status == "completed" and .conclusion == "success")') || exit 0

run_id=$(jq -er '.id | select(type == "number")' <<< "$run") || exit 0
attempt=$(jq -er '.run_attempt | select(type == "number")' <<< "$run") || exit 0
green=$(gh api "$repo/actions/runs/$run_id/jobs?filter=latest&per_page=100" |
  jq -ce --arg sha "$sha" --argjson attempt "$attempt" '
    select(.total_count == (.jobs | length))
    | .jobs | sort_by(.name) | group_by(.name)
    | map(select(length == 1) | .[0]
      | select(.head_sha == $sha and .run_attempt == $attempt and .status == "completed"
        and .conclusion == "success")
      | .name)') || exit 0

jq -e 'index("ci-aggregate") != null and index("qa / integration-verdict") != null' <<< "$green" >/dev/null || exit 0
echo "tested_commit=$sha" >> "$GITHUB_OUTPUT"
echo "green_jobs=$green" >> "$GITHUB_OUTPUT"
printf 'Using merge-queue run %s as the main commit QA receipt\n' "$run_id"
