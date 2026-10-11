---
title: Multi player
description: Serve a DataHub catalog over BigQuery for several users, and ask each question as the caller's own workload identity principal.
---

# Multi player

This example runs sutura for several users. Keycloak is the identity provider. DataHub is the
catalog, read with one token for every caller. BigQuery is the data system, and sutura asks it as
each caller's own principal in a workload identity pool. No caller has a service account.

The example is in
[`examples/multi-player`](https://github.com/telekom/sutura/tree/main/examples/multi-player):

| Path             | Contents                                                                     |
| ---------------- | ---------------------------------------------------------------------------- |
| `compose.yaml`   | Keycloak, DataHub with its services, a loader, and sutura                    |
| `setup.sh`       | Writes local secrets to `.env`, starts Keycloak and exports its public keys  |
| `keycloak/`      | The realm with the users `alice`, `bob` and `carol`                          |
| `infra/`         | A Pulumi program for the Google side: pool, grants, dataset and row policies |
| `dbt/`           | The dbt manifest that the loader writes into DataHub                         |
| `loader/`        | Writes the Keycloak keys and sutura's secrets, and loads the DataHub model   |
| `conf/base.yaml` | The sutura settings                                                          |

All data is synthetic.

## What you need

- Docker, Pulumi, and a Google Cloud test project.
- A Pulumi credential for the test project. It must enable APIs. It must create and remove a
  bucket, a dataset, tables, a workload identity pool, a provider, IAM grants and row access
  policies. Use this credential for setup and teardown only.

In an enterprise deployment, operators create the users, the identity provider, the pool, the data
and the grants in advance. sutura then needs no administrator or provisioning role. The
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

The program creates a workload identity pool that trusts the Keycloak realm, and a dataset. Its
grants and row access policies name the pool principals of `alice` and `bob`, and give them
different rows. Each caller must exist in BigQuery: `alice` and `bob` do, as their own pool
principals with their own grants. `carol` gets no grant. The two principals read one dataset,
and BigQuery applies each caller's own row access policy.
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

sutura exchanges the token at Keycloak for a token that the pool accepts. Google STS gives a
federated token for the caller's own pool principal, and BigQuery runs the query as that principal.

| Caller  | Expected result                                                          |
| ------- | ------------------------------------------------------------------------ |
| `alice` | Her rows, the annual contracts. BigQuery runs the query as her principal |
| `bob`   | Other rows, the monthly contracts, from the same question                |
| `carol` | A refusal. BigQuery refuses her question, because she has no grant       |

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
