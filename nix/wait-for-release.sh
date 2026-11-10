#!/usr/bin/env bash
set -euo pipefail

run=""
for _ in $(seq 1 60); do
  run="$(gh run list --workflow release.yml --event push --commit "$RELEASE_SHA" \
    --limit 1 --json databaseId --jq '.[0].databaseId // empty')"
  [ -z "$run" ] || break
  sleep 5
done
if [ -z "$run" ]; then
  echo '::error::no release workflow started for this release commit'
  exit 1
fi

gh run watch "$run" --exit-status --interval 30
tag="$(gh run view "$run" --json headBranch --jq .headBranch)"
if [ "$(gh release view "$tag" --json isDraft --jq .isDraft)" != false ]; then
  echo "::error::${tag} was not published"
  exit 1
fi
printf 'release-status: %s is published\n' "$tag"
