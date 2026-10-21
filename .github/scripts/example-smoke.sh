#!/usr/bin/env bash
#
# Runs one example the way its README says, and checks the answer to its question.
#
#   example-smoke.sh single-player|federated|cube|metrics-layer
#
# `SUTURA_VERSION` selects the released image, as in the READMEs; unset is `latest`. So this
# proves the example against that RELEASE, never against the commit under test. Linux only: the
# examples serve on the host network, which Docker Desktop turns off by default.
#
# Each expectation comes from:
#   single-player  - the answer block of docs/examples/single-player.md, section 5
#   federated      - a measured run over the CSV rows of examples/single-player/data (neither its
#                    README nor docs/examples/federated.md shows an answer)
#   cube           - the `202121` for June 2026 in examples/cube/README.md (equal to the sum of the
#                    CSV rows)
#   metrics-layer  - the numbers in examples/metrics-layer/README.md and
#                    docs/examples/metrics-layer.md (alice 262345, bob 150000, shared-service-user
#                    412345) and the rows of examples/metrics-layer/clickhouse/init.sql
set -euo pipefail

example=${1:?usage: example-smoke.sh single-player|federated|cube|metrics-layer}
version=${SUTURA_VERSION:-latest}
cd "$(dirname "$0")/../../examples/$example"

ask() { # ask <label> <expected-json> [<bearer-token>]
  local label=$1 expected=$2 token=${3-}
  local args=(--silent --fail --request POST http://127.0.0.1:8080/v1/query
    --header 'content-type: application/json')
  [ -z "$token" ] || args+=(--header "authorization: Bearer $token")
  local answer
  answer=$(curl "${args[@]}" --data "$question")
  local actual
  actual=$(jq --compact-output '{outcome, columns, rows: (.rows | sort)}' <<<"$answer")
  if [ "$actual" != "$expected" ]; then
    printf 'expected %s\nactual   %s\n' "$expected" "$actual" >&2
    exit 1
  fi
  echo "$label answered with the expected rows"
}

ready() {
  curl --silent --fail --retry 30 --retry-delay 2 --retry-connrefused --retry-all-errors \
    http://127.0.0.1:8080/health >/dev/null
}

token() { # token <user>: an access token of the realm's `sutura` client; the password is the user
  curl --silent --fail http://127.0.0.1:8180/realms/metrics-layer/protocol/openid-connect/token \
    -d grant_type=password -d client_id=sutura -d "username=$1" -d "password=$1" \
    | jq --raw-output .access_token
}

serve_metrics_layer() { # serve_metrics_layer <settings file>: the README's `cp` and `docker run`
  cp "conf/$1.yaml" .local/conf/base.yaml
  docker run --detach --network host --add-host cube.example.com:127.0.0.1 \
    --user "$(id -u):$(id -g)" \
    --volume "$PWD/.local:/run/sutura:ro" --env SUTURA_CONFIG_DIR=/run/sutura/conf \
    "ghcr.io/telekom/sutura:$version" serve
}

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
  cube)
    question='{"metrics":["recurring_revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}'
    expected='{"outcome":"answer","columns":["period","recurring_revenue"],"rows":[["2026-06-01","202121"]]}'
    ./setup.sh
    docker compose up --detach --wait
    serve=$(docker run --detach --network host --user "$(id -u):$(id -g)" -w / \
      --volume "$PWD/conf:/conf:ro" --volume "$PWD/../single-player:/examples/single-player:ro" \
      --volume "$PWD/.local:/run/sutura:ro" --env SUTURA_CONFIG_DIR=/conf \
      "ghcr.io/telekom/sutura:$version" serve)
    ;;
  metrics-layer)
    question='{"metrics":["revenue"],"grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}'
    ./setup.sh
    docker compose up --detach --wait datahub keycloak cube-tls
    docker compose run --rm loader
    mkdir -p .local/conf
    serve=$(serve_metrics_layer impersonation-at-source)
    ;;
  *)
    echo "unknown example: $example" >&2
    exit 2
    ;;
esac

ready
case "$example" in
  metrics-layer)
    rows() { printf '{"outcome":"answer","columns":["period","revenue"],"rows":[["2026-06-01","%s"]]}' "$1"; }
    ask alice "$(rows 262345)" "$(token alice)"
    ask bob "$(rows 150000)" "$(token bob)"
    status=$(curl --silent --output /dev/null --write-out '%{http_code}' --request POST \
      http://127.0.0.1:8080/v1/query --header 'content-type: application/json' --data "$question")
    [ "$status" = 401 ] || { echo "no token answered $status, want 401" >&2; exit 1; }
    echo "no token: refused with 401"
    docker rm -f "$serve" >/dev/null
    serve=$(serve_metrics_layer shared-service-user)
    ready
    ask alice "$(rows 412345)" "$(token alice)"
    ;;
  *) ask "$example" "$expected" ;;
esac
