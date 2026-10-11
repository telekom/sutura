# Metrics layer

DataHub as the catalog, Cube as the only data source, and Keycloak as the identity provider. Cube
computes each metric, and Cube decides which rows each caller reads. Read the walkthrough in
[`docs/examples/metrics-layer.md`](../../docs/examples/metrics-layer.md).

| File                                | What it holds                                                                     |
| ----------------------------------- | --------------------------------------------------------------------------------- |
| `conf/impersonation-at-source.yaml` | Each caller's query runs with that caller's own token in Cube                     |
| `conf/shared-service-user.yaml`     | Every caller's query runs with the token of the deployment's own client           |
| `keycloak/realm.json`               | Two callers, `alice` (region `north`) and `bob` (region `south`), and the clients |
| `cube/model.yml`                    | The `orders` and `customers` cubes, and the access policy on `orders`             |
| `clickhouse/init.sql`               | The tables that Cube reads                                                        |

`just dev-up-cube` starts the services in the `cube` profile of `compose.services.yaml`:
`cube-keycloak` with this realm, and `cube-metrics`, which serves `cube/model.yml` and verifies each
token with the keys of the realm.
