# Multi player

Keycloak as the identity provider, DataHub as a shared-service catalog, and BigQuery asked as each
caller's own service account (built, not yet proven against Google). The walkthrough is in
progress; it will be on the
[documentation site](https://telekom.github.io/sutura/latest/examples/multi-player/).

Quick start, from this directory: `./setup.sh`, then the Pulumi program in `infra/`, then
`SUTURA_VERSION=<version> docker compose up -d`. The image must read
`workload_identity.delegation`, and a release can predate it. If `sutura serve` refuses that key,
build the image from this tree (`nix build .#oci && ./result | docker load`), then tag the loaded
`sutura:latest` as `ghcr.io/telekom/sutura:<version>`.

Ask one question as alice (bob reads other rows, carol is refused). Without Docker Desktop's host
networking, run these in a `--network host` container:

```sh
TOKEN=$(curl -s http://127.0.0.1:8180/realms/sutura-example/protocol/openid-connect/token \
  -d grant_type=password -d client_id=sutura -d username=alice -d password=alice | jq -r .access_token)
curl -s http://127.0.0.1:8080/v1/query -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"metrics":["recurring_revenue"],"grain":"month","range":{"start":"2026-01-01","end":"2026-07-01"}}'
```

`question.json` (revenue in June 2026) is not this stack's question: it is the offline question
`crates/sutura-catalog-datahub/tests/multi_player.rs` compiles against the recorded DataHub fixture.
