---
title: BigQuery
description: Answer questions over a BigQuery dataset, as one service account or as each caller's own account (built, not proven).
---

# BigQuery

The `bigquery` data system answers questions over a BigQuery dataset. sutura renders each plan as
GoogleSQL and runs it through the ADBC BigQuery driver. The crate is `sutura-exec-bigquery`, and
the source kind is `bigquery`. It is the one data system with a path to run a query as the caller.
That path is built and not proven: see [Identity](#identity).

## When to use it

- Your data is in BigQuery, and BigQuery grants and row access policies control who sees what.
- You want each caller's query to run as that caller's own service account.
  `posture: impersonation-at-source` does this. It is built, and no recorded run has yet shown
  Google accept the caller's identity.
- Or you want all queries to run as one service account. `posture: shared-service-user` does
  this.

## Settings

The entry reads the [settings that every data system has](../../integrations.md#data-system-settings)
and these keys:

| Key                | Type          | Default  | Meaning                                                                                              |
| ------------------ | ------------- | -------- | ---------------------------------------------------------------------------------------------------- |
| `billing_project`  | string        | required | The project that BigQuery bills the job to. 6 to 30 characters: `a-z`, `0-9`, `-`                    |
| `dataset`          | string        | required | The dataset where unqualified table names resolve. It must be in `billing_project`                   |
| `credential_file`  | absolute path | required | Required as an absolute path. The driver finds its own credential, so sutura does not read this file |
| `max_bytes_billed` | integer       | required | The most bytes that one job may bill for. 1 byte to 1 TiB                                            |

`impersonation-at-source` needs a `workload_identity` block:

| Key                                               | Type          | Default  | Meaning                                                                                                                                          |
| ------------------------------------------------- | ------------- | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `workload_identity.audience`                      | string        | required | The workload identity pool provider: `//iam.googleapis.com/projects/<number>/locations/global/workloadIdentityPools/<pool>/providers/<provider>` |
| `workload_identity.scope`                         | string        | required | Write `https://www.googleapis.com/auth/bigquery`                                                                                                 |
| `workload_identity.impersonate`                   | map           | required | Caller subject (`sub`) to service account email. A caller who is not in the map is refused                                                       |
| `workload_identity.delegation.token_endpoint`     | URL           | none     | The token endpoint of your identity provider, for a token exchange                                                                               |
| `workload_identity.delegation.client_id`          | string        | none     | The client ID that sutura uses for the exchange                                                                                                  |
| `workload_identity.delegation.client_secret_file` | absolute path | none     | A file that holds the client secret                                                                                                              |
| `workload_identity.delegation.audience`           | string        | none     | The audience that the exchanged token must carry                                                                                                 |

With `delegation`, sutura exchanges the caller's token at your identity provider for a token that
the pool accepts. `delegation` needs `security.inbound.mode: direct`. Without `delegation`, sutura
sends the caller's verified token to the pool as it is. Do not write `expected_issuer` or
`expected_audience`: sutura refuses to start with them.

The musl and glibc release binaries link the BigQuery driver. A `cargo` build loads the driver
from the absolute path in `SUTURA_BIGQUERY_ADBC_DRIVER`.

## Example

The [multi player](../../examples/multi-player.md) example writes this source with Pulumi:

```yaml
sources:
  warehouse:
    kind: "bigquery"
    billing_project: "my-project"
    dataset: "sutura_example"
    credential_file: "/nonexistent/not-read.json"
    max_bytes_billed: 1073741824
    posture: "impersonation-at-source"
    workload_identity:
      audience: "//iam.googleapis.com/projects/123456789/locations/global/workloadIdentityPools/sutura/providers/keycloak"
      scope: "https://www.googleapis.com/auth/bigquery"
      impersonate:
        0d6f2a1e-5b3c-4e8a-9f21-7c4b8e2d1a01: "sutura-mp-alice@my-project.iam.gserviceaccount.com"
        0d6f2a1e-5b3c-4e8a-9f21-7c4b8e2d1a02: "sutura-mp-bob@my-project.iam.gserviceaccount.com"
      delegation:
        token_endpoint: "http://127.0.0.1:8180/realms/sutura-example/protocol/openid-connect/token"
        client_id: "https://sutura.example.com"
        client_secret_file: "/run/sutura/exchange-secret"
        audience: "sutura-gcp-pool"
```

For one shared account, use `posture: shared-service-user` and leave out `workload_identity`. The
driver then uses the application default credentials of the process.

## Identity

With `shared-service-user`, every query runs as the application default credentials of the
process.

With `impersonation-at-source`, sutura gives the driver the caller's verified assertion. Google
checks it against the pool in `audience`, and the query runs as the service account that
`impersonate` maps to the caller. sutura refuses an anonymous caller and a caller who is not in
the map. It never runs their query as the deployment. This path is built, and no recorded run has
yet shown Google accept the caller's identity.

## Sizing

One result has at most 1 000 000 rows and 256 MiB. A time column must be `DATE`: sutura refuses
`TIMESTAMP` and `DATETIME` time columns. `governance.per_replica_spend_ceiling` cannot be used with
a BigQuery source.
