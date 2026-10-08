# Multi player

Keycloak as the identity provider, DataHub as a shared-service catalog, and BigQuery asked as each
caller's own service account (built, not yet proven against Google). Read the walkthrough in
[`docs/examples/multi-player.md`](../../docs/examples/multi-player.md), or on the
[documentation site](https://telekom.github.io/sutura/latest/examples/multi-player/).

This demo needs Docker, Pulumi and a Google Cloud test project. The Pulumi credential creates and
removes the test resources; use it for setup and teardown only. An enterprise deployment uses
accounts, a pool and grants that operators create in advance.

Quick start, from this directory:

```sh
SUTURA_VERSION=<version> ./setup.sh
cd infra && pulumi stack init local && pulumi config set project <your-project-id>
pulumi up && pulumi stack output settings > ../conf/development.yaml && cd ..
docker compose up -d
```

`question.json` (revenue in June 2026) is not this stack's question: it is the offline question
`crates/sutura-catalog-datahub/tests/multi_player.rs` compiles against the recorded DataHub fixture.
