---
title: Multi player
description: Serve a DataHub catalog over BigQuery for several users, and ask each question as the caller's own Google service account (built, not proven).
---

# Multi player

This example runs sutura for several users. Keycloak is the identity provider. DataHub is the
catalog, read with one token for every caller. BigQuery is the data system, and sutura asks it as
the service account that it maps to each caller.

!!! warning "Built, not proven against Google"

    The per-caller BigQuery path is built. No test has yet observed it run against Google, so this
    example is the first place where you can see it work. See
    [BigQuery](../integrations/data-systems/bigquery.md#identity).

The example is in
[`examples/multi-player`](https://github.com/telekom/sutura/tree/main/examples/multi-player):

| Path             | Contents                                                                       |
| ---------------- | ------------------------------------------------------------------------------ |
| `compose.yaml`   | Keycloak, DataHub with its services, a loader, and sutura                      |
| `setup.sh`       | Writes local secrets to `.env`, starts Keycloak and exports its public keys    |
| `keycloak/`      | The realm with the users `alice`, `bob` and `carol`                            |
| `infra/`         | A Pulumi program for the Google side: pool, accounts, dataset and row policies |
| `dbt/`           | The dbt manifest that the loader writes into DataHub                           |
| `loader/`        | Writes the Keycloak keys and sutura's secrets, and loads the DataHub model     |
| `conf/base.yaml` | The sutura settings                                                            |

All data is synthetic.

## What you need

- Docker, Pulumi, and a Google Cloud test project.
- A Pulumi credential for the test project. It must enable APIs. It must create and remove a
  bucket, a dataset, tables, a workload identity pool, a provider, service accounts, IAM grants
  and row access policies. Use this credential for setup and teardown only.
- A sutura image that reads `workload_identity.delegation`. An older release refuses that key. In
  that case, build the image from this tree with `nix build .#oci && ./result | docker load`, and
  tag `sutura:latest` as `ghcr.io/telekom/sutura:<version>`.

In an enterprise deployment, operators create the accounts, the identity provider, the pool, the
data and the grants in advance. sutura then needs no administrator or provisioning role. The
catalog needs its own read credential.

## 1. Start Keycloak

Run all commands from `examples/multi-player`.

```bash
SUTURA_VERSION=<version> ./setup.sh
```

`setup.sh` writes random secrets to `.env`, starts Keycloak, and writes the realm's public keys to
`.local/jwks.json`. Run it again after `docker compose down -v`, because that deletes the realm key.

## 2. Create the Google side

```bash
cd infra
pulumi stack init local
pulumi config set project <your-project-id>
pulumi up
pulumi stack output settings > ../conf/development.yaml
cd ..
```

The program creates a workload identity pool that trusts the Keycloak realm, one service account
for `alice` and one for `bob`, and a dataset whose row access policies give them different rows.
It loads the CSV files of the [single player](single-player.md) example into that dataset. Its
output is the BigQuery source for sutura, in `conf/development.yaml`.

## 3. Start the stack

```bash
docker compose up -d
```

sutura reads `conf/base.yaml`, and then `conf/development.yaml` on top of it. This is
`conf/base.yaml`:

```yaml
--8<-- "examples/multi-player/conf/base.yaml"
```

## 4. Ask a question as each caller

Get a token for `alice` from Keycloak, and ask a question with it. Without the host network of
Docker Desktop, run these commands in a container with `--network host`.

```bash
TOKEN=$(curl -s http://127.0.0.1:8180/realms/sutura-example/protocol/openid-connect/token \
  -d grant_type=password -d client_id=sutura -d username=alice -d password=alice | jq -r .access_token)
curl -s http://127.0.0.1:8080/v1/query -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"metrics":["recurring_revenue"],"grain":"month","range":{"start":"2026-01-01","end":"2026-07-01"}}'
```

| Caller  | Expected result                                                              |
| ------- | ---------------------------------------------------------------------------- |
| `alice` | Her rows. BigQuery runs the query as her service account                     |
| `bob`   | Other rows, from the same question                                           |
| `carol` | A refusal. The source maps no account to her, so sutura runs nothing for her |

## 5. Remove it

```bash
docker compose down -v
cd infra && pulumi destroy
```

## Related

- [Inbound identity](../integrations/identity.md): how sutura verifies a caller's token.
- [BigQuery](../integrations/data-systems/bigquery.md): the source settings that `infra/` writes.
- [DataHub](../integrations/catalogs/datahub.md): the catalog settings in `conf/base.yaml`.
- [Single player](single-player.md): one user, local files, no identity provider.
