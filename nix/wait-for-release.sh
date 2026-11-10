#!/usr/bin/env bash
set -euo pipefail

for workflow in release.yml fuzz.yml; do
  run=""
  for _ in $(seq 1 60); do
    run="$(gh run list --workflow "$workflow" --event push --commit "$RELEASE_SHA" \
      --limit 1 --json databaseId --jq '.[0].databaseId // empty')"
    [ -z "$run" ] || break
    sleep 5
  done
  if [ -z "$run" ]; then
    echo "::error::${workflow} did not start for this release commit"
    exit 1
  fi

  gh run watch "$run" --exit-status --interval 30
  if [ "$workflow" = release.yml ]; then
    tag="$(gh run view "$run" --json headBranch --jq .headBranch)"
  fi
done
if [ "$(gh release view "$tag" --json isDraft --jq .isDraft)" != false ]; then
  echo "::error::${tag} was not published"
  exit 1
fi
printf 'release-status: %s is published\n' "$tag"
