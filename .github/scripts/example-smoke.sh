#!/usr/bin/env bash
#
# Runs one example the way its README says, and checks the answer to its question.
#
#   example-smoke.sh single-player|federated
#
# `SUTURA_VERSION` selects the released image, as in the READMEs; unset is `latest`. So this
# proves the example against that RELEASE, never against the commit under test. Linux only: the
# examples serve on the host network, which Docker Desktop turns off by default.
#
# Each example is compared with a different expectation: single-player with the answer block of its
# walkthrough page `docs/examples/single-player.md` (section 5); federated asks the question its
# README posts (`examples/federated/README.md`), whose expected rows are a measured run over the CSV
# rows of `examples/single-player/data`, since neither the README nor `docs/examples/federated.md`
# shows an answer. The `null` row is the subscription whose customer `dim_customer` does not contain.
set -euo pipefail

example=${1:?usage: example-smoke.sh single-player|federated}
version=${SUTURA_VERSION:-latest}
cd "$(dirname "$0")/../../examples/$example"

serve=
cleanup() {
  status=$?
  if [ "$status" -ne 0 ]; then
    echo "::group::logs"
    [ -z "$serve" ] || docker logs "$serve" || true
    docker compose logs --no-color || true
    echo "::endgroup::"
  fi
  [ -z "$serve" ] || docker rm -f "$serve" >/dev/null || true
  docker compose down --volumes --remove-orphans || true
}
trap cleanup EXIT

case "$example" in
  single-player)
    question='{"metrics":["recurring_revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"dimensions":["region"]}'
    expected='{"outcome":"answer","columns":["region","period","recurring_revenue"],"rows":[["central","2026-06-01","51739"],["east","2026-06-01","32598"],["north","2026-06-01","42157"],["null","2026-06-01","4999"],["south","2026-06-01","21203"],["west","2026-06-01","49425"]]}'
    SUTURA_VERSION=$version docker compose up --detach
    ;;
  federated)
    question='{"metrics":["recurring_revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},"dimensions":["segment"]}'
    expected='{"outcome":"answer","columns":["segment","period","recurring_revenue"],"rows":[["business","2026-06-01","66630"],["consumer","2026-06-01","123119"],["null","2026-06-01","4999"],["wholesale","2026-06-01","7373"]]}'
    ./setup.sh
    docker compose up --detach --wait postgres clickhouse datahub
    docker compose run --rm federated-loader
    serve=$(docker run --detach --network host --user "$(id -u):$(id -g)" \
      --volume "$PWD/conf:/conf:ro" --volume "$PWD/.local:/run/sutura:ro" \
      --env SUTURA_CONFIG_DIR=/conf \
      "ghcr.io/telekom/sutura:$version-musl" serve)
    ;;
  *)
    echo "unknown example: $example" >&2
    exit 2
    ;;
esac

curl --silent --fail --retry 30 --retry-delay 2 --retry-connrefused --retry-all-errors \
  http://127.0.0.1:8080/health >/dev/null

answer=$(curl --silent --fail --request POST http://127.0.0.1:8080/v1/query \
  --header 'content-type: application/json' --data "$question")
actual=$(jq --compact-output '{outcome, columns, rows: (.rows | sort)}' <<<"$answer")
if [ "$actual" != "$expected" ]; then
  printf 'expected %s\nactual   %s\n' "$expected" "$actual" >&2
  exit 1
fi
echo "$example answered its question with the expected rows"
