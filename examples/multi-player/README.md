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

`question.json` (revenue in June 2026) is the offline question
`crates/sutura-catalog-datahub/tests/multi_player.rs` compiles against the recorded DataHub fixture.
