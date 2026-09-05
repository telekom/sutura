#!/usr/bin/env bash
#
# DOES THE PUBLISHED SERVER IMAGE ANSWER A QUESTION?
#
# `github.com/telekom/sutura#111`'s provable outcome is "a user runs one published artefact and asks
# a question over HTTP, with no Rust toolchain". So this asks one. An earlier version of this test
# probed `/health` and stopped there, which proves startup - the catalog loaded through its port and
# every declared anchor re-executed against the data system, since the service has no other
# constructor - and proves nothing about the question path: the image could lose `POST /v1/query`,
# break request decoding or break answer serialization and a liveness probe would still pass. Raised
# in review of the change that closed #111.
#
# THREE ASSERTIONS, and the third is what makes the second mean anything:
#
#   1. `/health` answers, so the process started and its anchors held.
#   2. The corpus question `examples/single-player/questions/recurring-revenue-by-month.yaml`
#      answers `200` with the certified numbers. Not just `200`: the January cell is compared by
#      value, because a `200` carrying an empty row set is the failure this is most likely to miss.
#   3. A question the catalog refuses answers `403 dimension_value_not_allowed`. Without it, a
#      build that answered `200` to everything would pass - and a refusal is a RESULT here rather
#      than an error, so that it is served at all is part of the surface #111 publishes.
#
# ONE SCRIPT, and `.github/actions/build-artefacts/action.yml` invokes the image smoke test - per
# target, at release and on a dispatch, and it is the only caller in the tree. Not inlined there: it
# was written twice and the two copies had already drifted on the catalog version before this file
# existed. A `.sh` also lands in the shellcheck list `nix/lint-workflows.sh` builds from tracked
# files, which an `action.yml` body does not.
#
# BOTH POINTERS ARE DERIVED NOW rather than written down - `cargo xtask check-guidance` resolves the
# file holding each mechanism and fails if a sentence names a different one. They used to name
# `ci.yml`, which has no image step and does not build that list, and nothing could see it: a bare
# filename with no slash is deliberately not path-checked, and `ci.yml` resolves anyway.
#
# `--network host` RATHER THAN `-p`, and it is a startup refusal that decides it. The default bind
# is loopback, and inside a container loopback is the container - so a published port reaches
# nothing. Binding `0.0.0.0` instead makes the deployment one other hosts can reach, which needs
# `security.access_token` AND a `security.tls_termination` naming the cleartext hop; without both the
# process refuses to start. With `--network host` the container's loopback IS the runner's, so this
# runs the single-player posture the example documents rather than a shape nobody would deploy.
#
# The image starts in `/examples`, so the embedded single-catalog defaults resolve `catalog/` from
# the mounted example. Only the source is overridden here; no stale singular catalog keys survive.
set -euo pipefail

image="${1:?usage: serve-smoke.sh <image-ref> [port]}"
port="${2:-18080}"
base="http://127.0.0.1:${port}"
examples="${PWD}/examples/single-player"

if [ ! -d "$examples" ]; then
  echo "::error::serve-smoke: ${examples} does not exist; run this from the repository root" >&2
  exit 1
fi

id="$(docker run -d --network host --user 65532:65532 \
  -w /examples \
  -v "${examples}:/examples:ro" \
  -e "SUTURA__SERVER__PORT=${port}" \
  -e SUTURA__SECURITY__IDENTITY=single-user \
  -e SUTURA__SECURITY__SINGLE_USER_BECAUSE="the release smoke test reads its own example files" \
  -e SUTURA__SOURCES__LOCAL__KIND=files \
  -e SUTURA__SOURCES__LOCAL__DATA_DIR=/examples/data \
  -e SUTURA__SOURCES__LOCAL__POSTURE=shared-service-user \
  "$image")"

# The log and the container go either way: on success the log is the startup output a first-time
# operator sees, and on failure it is the only thing that says why.
cleanup() {
  docker logs "$id" 2>&1 | tail -60 || true
  docker rm -f "$id" >/dev/null 2>&1 || true
}
trap cleanup EXIT

live=0
for _ in $(seq 1 30); do
  if curl -fsS --max-time 2 "${base}/health" >/dev/null 2>&1; then
    live=1
    break
  fi
  sleep 1
done
if [ "$live" -ne 1 ]; then
  echo "::error::serve-smoke: ${image} never answered ${base}/health" >&2
  exit 1
fi
echo "serve-smoke: ${image} started as 65532 and answered /health"

# No `authorization` header, and that is the configuration rather than an omission: the bind is
# loopback, so no access token is required and none is set.
answer="$(mktemp)"
status="$(curl -s -o "$answer" -w '%{http_code}' --max-time 20 \
  -X POST "${base}/v1/query" \
  -H 'content-type: application/json' \
  -d '{"metric":"recurring_revenue","grain":"month","range":{"start":"2026-01-01","end":"2026-07-01"}}')"
echo "serve-smoke: POST /v1/query -> ${status}"
cat "$answer"
echo
if [ "$status" != "200" ]; then
  echo "::error::serve-smoke: the question was not answered: HTTP ${status}" >&2
  exit 1
fi
for want in '"outcome":"answer"' '"recurring_revenue"' '"237320"'; do
  if ! grep -qF "$want" "$answer"; then
    echo "::error::serve-smoke: the answer does not contain ${want}" >&2
    exit 1
  fi
done
echo "serve-smoke: the answer carries the certified January figure and its provenance"

# THE CONTROL. `region` is filterable and `offshore` is not one of the values the metric declares
# for it, so this is a governance refusal served as a result - and its 403 is what says the 200
# above was a decision rather than a default.
refusal="$(mktemp)"
status="$(curl -s -o "$refusal" -w '%{http_code}' --max-time 20 \
  -X POST "${base}/v1/query" \
  -H 'content-type: application/json' \
  -d '{"metric":"recurring_revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"filters":[{"dimension":"region","value":"offshore"}]}')"
echo "serve-smoke: a refused question -> ${status}"
cat "$refusal"
echo
if [ "$status" != "403" ]; then
  echo "::error::serve-smoke: a question the catalog refuses returned HTTP ${status}, expected 403" >&2
  exit 1
fi
if ! grep -qF '"code":"dimension_value_not_allowed"' "$refusal"; then
  echo "::error::serve-smoke: the refusal does not name dimension_value_not_allowed" >&2
  exit 1
fi
echo "serve-smoke: ${image} answers a certified question and refuses one it may not answer"
