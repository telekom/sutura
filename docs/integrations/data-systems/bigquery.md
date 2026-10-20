---
title: BigQuery
description: Answer questions over a BigQuery dataset, as one service account or as each caller's own workload identity principal.
---

# BigQuery

<span class="sutura-badge sutura-badge--recommended">Recommended</span>

The `bigquery` data system answers questions over a BigQuery dataset. sutura renders each plan as
GoogleSQL and runs it through the ADBC BigQuery driver. The crate is `sutura-exec-bigquery`, and
the source kind is `bigquery`. It can run a query as the caller. See [Identity](#identity).

## When to use it

- Your data is in BigQuery, and BigQuery grants and row access policies control who sees what.
- You want each caller's query to run as that caller's own principal in a workload identity pool.
  `posture: impersonation-at-source` does this (secure-impersonation). No caller needs a service
  account.
- Or you want all queries to run as one service account. `posture: shared-service-user` does
  this.

## Settings

The entry reads the [settings that every data system has](../../integrations.md#data-system-settings)
and these keys:

| Key                | Type    | Default  | Meaning                                                                            |
| ------------------ | ------- | -------- | ---------------------------------------------------------------------------------- |
| `billing_project`  | string  | required | The project that BigQuery bills the job to. 6 to 30 characters: `a-z`, `0-9`, `-`  |
| `dataset`          | string  | required | The dataset where unqualified table names resolve. It must be in `billing_project` |
| `max_bytes_billed` | integer | required | The most bytes that one job may bill for. 1 byte to 1 TiB                          |

`impersonation-at-source` needs exactly one of `workload_identity`, `google_login` or
`domain_delegation`. sutura runs `workload_identity` and `google_login`. `workload_identity` has
these keys:

| Key                                                 | Type          | Default          | Meaning                                                                                                                                                                                  |
| --------------------------------------------------- | ------------- | ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `workload_identity.audience`                        | string        | required         | The workload identity pool provider: `//iam.googleapis.com/projects/<number>/locations/global/workloadIdentityPools/<pool>/providers/<provider>`                                         |
| `workload_identity.delegation`                      | list          | none             | The hops that exchange the caller's token, in order. Not empty. Must not be present without a value                                                                                      |
| `workload_identity.delegation[].token_endpoint`     | URL           | required         | The token endpoint of the identity provider for this hop                                                                                                                                 |
| `workload_identity.delegation[].client_id`          | string        | required         | The client ID that sutura uses at this identity provider                                                                                                                                 |
| `workload_identity.delegation[].client_secret_file` | absolute path | required         | A file that holds the client secret. sutura reads it at startup                                                                                                                          |
| `workload_identity.delegation[].grant`              | string        | `token-exchange` | `token-exchange` (RFC 8693), `on-behalf-of` (Microsoft Entra ID), or `broker-token` (Keycloak sends back the token of an external identity provider). The last hop is not `broker-token` |
| `workload_identity.delegation[].audience`           | string        | required         | The audience that the token from this hop must carry. With `on-behalf-of`, sutura asks for the scope `<audience>/.default`.                                                              |

With `delegation`, the first hop exchanges the caller's token at its identity provider, and each
next hop exchanges the token from the hop before it. The token from the last hop is the one that
the pool accepts. `delegation` needs `security.inbound.mode: direct`. Do not write
`expected_issuer` or `expected_audience`: sutura refuses to start with them.

With `security.inbound.mode: direct`, declare `delegation`. Then the pool gets a token for its own
audience. Configure the pool provider to accept only that audience. If you do not declare
`delegation`, sutura does not start. The message names two fixes:

1. Declare `delegation`. This is the recommended fix.
2. Write `inbound_token_at_pool: accepted` on the source, next to `workload_identity`. Then sutura
   sends the caller's verified token to the pool as it is, so the pool provider must accept the
   audience of sutura itself. `accepted` is the only value. sutura logs one `WARN` line for the
   source at each start.

sutura refuses to start when `inbound_token_at_pool` is written on a source that declares
`delegation`, on a source without `workload_identity`, or on a deployment that is not `direct`. It
also refuses to start when the last `delegation` hop asks for the audience of sutura itself
(`security.inbound.resource` or `security.inbound.audience`). This check compares the configured
text only.

With `google_login`, the query runs as the Google account that the caller linked at the identity
provider. sutura sends the caller's verified token to the identity provider's broker token
endpoint (Keycloak's Identity Brokering API v2) and gives BigQuery the Google access token that
comes back. `google_login` needs `security.inbound.mode: direct`. A caller who has not linked a
Google account gets `403 identity_not_linked`. These are its keys:

| Key                               | Type          | Meaning                                                                         |
| --------------------------------- | ------------- | ------------------------------------------------------------------------------- |
| `google_login.token_endpoint`     | URL           | The broker token endpoint: `https://<host>/realms/<realm>/broker/<alias>/token` |
| `google_login.client_id`          | string        | The client ID that sutura uses at the identity provider                         |
| `google_login.client_secret_file` | absolute path | A file that holds the client secret. sutura reads it at startup                 |

The identity provider keeps the Google token and refreshes it. Its Google provider must store the
token, and the sutura client must be allowed to retrieve it. sutura gives the token to BigQuery as
it is and does not refresh it during a query.

sutura reads and checks the `domain_delegation` block, and then refuses to start with it:

| Key                                    | Type   | Meaning                                                            |
| -------------------------------------- | ------ | ------------------------------------------------------------------ |
| `domain_delegation.delegating_account` | string | The service account: `<account>@<project>.iam.gserviceaccount.com` |

The musl and glibc release binaries link the BigQuery driver. In the development shell, a `cargo`
build can select the feature:

```bash
cargo build --release -p sutura-cli --features bigquery
```

That build links no driver. It loads the driver from the absolute path in
`SUTURA_BIGQUERY_ADBC_DRIVER`.

## Example

The [multi player](../../examples/multi-player.md) example writes this source with Pulumi:

```yaml
sources:
  warehouse:
    kind: "bigquery"
    billing_project: "my-project"
    dataset: "sutura_example"
    max_bytes_billed: 1073741824
    posture: "impersonation-at-source"
    workload_identity:
      audience: "//iam.googleapis.com/projects/123456789/locations/global/workloadIdentityPools/sutura/providers/keycloak"
      delegation:
        - token_endpoint: "http://127.0.0.1:8180/realms/sutura-example/protocol/openid-connect/token"
          client_id: "https://sutura.example.com"
          client_secret_file: "/run/sutura/exchange-secret"
          audience: "sutura-gcp-pool"
```

For one shared account, use `posture: shared-service-user` and leave out `workload_identity`. The
driver then uses the application default credentials of the process.

## Identity

This data system supports `shared-service-user` and secure-impersonation.

With `shared-service-user`, every query runs as one identity: the application default
credentials of the process.

With `impersonation-at-source`, each caller has their own principal in the pool, and the query runs
with that principal's own grants. No service account is used at any step:

1. sutura verifies the caller's token.
2. With `delegation`, sutura exchanges that token through each hop in order, for a token that the
   pool accepts.
3. The driver sends the token to the Google Security Token Service (STS). STS accepts it only
   when the pool provider in `audience` trusts its issuer, its signature is valid and its audience
   matches. This is how the token proves who the caller is.
4. STS gives a federated token for the caller's principal:
   `principal://iam.googleapis.com/projects/<number>/locations/global/workloadIdentityPools/<pool>/subject/<subject>`.
   `<subject>` is the `google.subject` that the pool provider maps, for example the token's `sub`.
5. BigQuery runs the query as that principal. The BigQuery grants and row access policies on that
   principal decide which rows the caller sees.

Each caller must exist in BigQuery, as their own pool principal with their own grants. Give each
caller's principal the roles `roles/bigquery.jobUser`, `roles/bigquery.readSessionUser` and
`roles/bigquery.dataViewer`, and name it in your row access policies. BigQuery refuses the query
of a caller who has no grant. To limit which callers can get a federated token, set an attribute
condition on the pool provider. sutura refuses an anonymous caller. It never runs their query as
the deployment.

!!! note

    Callers can share one dataset, because BigQuery applies each caller's own grants and row access
    policies to the query.

## Sizing

One result has at most 1 000 000 rows and 256 MiB. A time column must be `DATE`: sutura refuses
`TIMESTAMP` and `DATETIME` time columns. `governance.per_replica_spend_ceiling` cannot be used with
a BigQuery source.
