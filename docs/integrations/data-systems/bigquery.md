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

| Key                               | Type    | Default  | Meaning                                                                                                       |
| --------------------------------- | ------- | -------- | ------------------------------------------------------------------------------------------------------------- |
| `max_bytes_billed`                | integer | required | The most bytes that one job may bill for. 1 byte to 1 TiB                                                     |
| `billing.service_account_project` | string  | none     | `shared-service-user` only, and required there: the project that BigQuery bills every job to                  |
| `billing.by_group`                | list    | none     | `impersonation-at-source` only: `group` and `project` pairs. The first group the caller holds names the payer |

Every model names its table as `project.dataset.table`. A job on an `impersonation-at-source` source
bills, in order: the project of the caller's first `billing.by_group` group, then the one project
that the job's tables are in. If no step decides a payer, the question is refused.

`impersonation-at-source` needs exactly one of `workload_identity`, `google_login` or
`domain_delegation`. `workload_identity` has these keys:

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

With `domain_delegation`, the query runs as the Google account with the caller's verified email
address. The deployment vouches for the caller: by default sutura takes the address from the `email`
claim of the caller's verified token, and only when `email_verified` is `true`. sutura uses the
address as it is written. A caller without a verified address is refused. The deployment's own
application default credentials sign as the delegating account, which acts as that address with the
read-only scope `https://www.googleapis.com/auth/bigquery.readonly` only. These are its keys:

| Key                                    | Type   | Meaning                                                            |
| -------------------------------------- | ------ | ------------------------------------------------------------------ |
| `domain_delegation.delegating_account` | string | The service account: `<account>@<project>.iam.gserviceaccount.com` |
| `domain_delegation.address_domains`    | list   | Optional. The only domains that the source runs a caller as        |

With `address_domains`, sutura refuses a caller whose address is in no listed domain with
`address_outside_domains` (`403`), before it gets a credential for any source. A domain matches
only as a whole, and ASCII case is ignored: for `example.com`, `user@Example.com` matches, and
`user@sub.example.com` and `user@evil-example.com` do not. Write an internationalized domain in its
ASCII (`xn--`) form. An empty list is refused at start.

`security.inbound.address_claim` can name another claim than `email` for the address. See
[The caller's address](../identity.md#the-callers-address).

`domain_delegation` has these requirements:

- The identity provider must not let users edit their own email address, or the claim that
  `address_claim` names. A user who can set it to another person's address runs queries as that
  person. `address_domains` limits this to the listed domains and does not remove it.
- A Google Workspace or Cloud Identity administrator authorizes the delegating account's client ID
  for domain-wide delegation, with the scope `https://www.googleapis.com/auth/bigquery.readonly`
  only.
- The deployment's own identity has `iam.serviceAccounts.signJwt` on the delegating account (the
  role `roles/iam.serviceAccountTokenCreator`, granted on that account only).
- Each caller's Google account has its own BigQuery grants. The delegating account can act as any
  user in the domain, so protect the deployment's credential as you protect the domain's
  administrator.

The read-only scope limits what that credential can do as a user. Google refuses it `jobs.insert`,
so it cannot start a load, copy or extract job. Google also refuses a `CREATE TABLE`, `INSERT`,
`UPDATE` or `DROP TABLE` statement that it sends through `jobs.query`. sutura starts each query
with `jobs.query` and reads the rows through the Storage Read API, which the scope allows.

Google also refuses it `jobs.cancel`, so sutura cannot cancel a `domain_delegation` job. When the
caller's deadline ends and sutura stops waiting, the job runs on until it ends or until BigQuery
stops it at the job timeout sutura sent. `max_bytes_billed` still caps the bytes it bills. Nothing
caps its slot time.

The musl and glibc release binaries link the BigQuery driver. In the development shell, a `cargo`
build can select the feature:

```bash
cargo build --release -p sutura-cli --features bigquery
```

That build links no driver. It loads the driver from the absolute path in
`SUTURA_BIGQUERY_ADBC_DRIVER`.

## Job location

sutura sends no job location and declares no default dataset. Each model names its table as
`project.dataset.table`. BigQuery [determines the job location from the resources in the request](https://cloud.google.com/bigquery/docs/locations#specify_locations),
including the datasets those tables belong to. sutura leaves this selection to BigQuery rather
than looking up table locations itself.

For a request with no resource from which BigQuery can determine a location, BigQuery uses its
configured default location, or the `US` multi-region when no default is configured. That is a
service default, not a sutura setting.

This source has no setting that constrains a job to an explicit region. Google also warns that a
query sent without a location may be temporarily stored in BigQuery router logs while the service
determines where to run it. Location inference therefore provides no explicit residency guarantee
from sutura.

## Example

The [multi player](../../examples/multi-player.md) example writes this source with Pulumi:

```yaml
sources:
  warehouse:
    kind: "bigquery"
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

For one shared account, use `posture: shared-service-user`, leave out `workload_identity` and set
`billing.service_account_project`. The
driver then uses the application default credentials of the process.

## Identity

This data system supports `shared-service-user` and secure-impersonation.

With `shared-service-user`, every query runs as one identity: the application default
credentials of the process.

With `workload_identity`, each caller has their own principal in the pool, and the query runs
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

### Job labels

Each query job that sutura runs for a request has three labels, in each posture:

| Label                | Value                                                                                                                                      |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `sutura_client`      | The OAuth client of the caller's token: its `azp`, else its `client_id`. Empty if it has neither                                           |
| `sutura_actor`       | The immediate actor of the token's `act` claim, in the masked form of the audit record. Empty if it has none                               |
| `sutura_correlation` | A random id that sutura makes for the request: 32 hex digits. It is never the caller's `x-correlation-id`. Empty if the surface makes none |

The audit record of the request writes the same three values. Thus an auditor can join a job in
the BigQuery audit log or in `INFORMATION_SCHEMA.JOBS` to the audit record on
`sutura_correlation`, and see which agent asked for which user. The audit record also writes the
caller's `x-correlation-id` as `correlation`, so it joins the logs of the caller too. The id has
128 random bits, so two requests have the same id only with a negligible probability, also across
restarts and replicas.

A label value has at most 63 characters: lowercase letters, digits, `_` and `-`. sutura keeps a
value that fits. It changes a value that does not fit to its first 54 characters, in lowercase and
with `_` for each other character, then `-` and the first 8 hex digits of the SHA-256 of the full
value.

Whether a job names the user depends on the posture. With `impersonation-at-source`, the
principal of the job is the user's own principal, so the job names the user. With
`shared-service-user`, the job runs as the deployment's own principal, so the job does not
name the user. Only a join of `sutura_correlation` to the audit record names the user.

With `domain_delegation`, the audit record also writes `delegated_as`: the source and the address
that the job runs as, in the masked form of the audit record, for example
`warehouse=a***.o***@e***.c***`. The record has this field also when the call fails, for example
when BigQuery does not answer. The full address is the `user_email` of the job in
`INFORMATION_SCHEMA.JOBS`, which an auditor finds on `sutura_correlation`.

The labels never hold the user's subject in either posture, because each principal that can
list the jobs of the billing project can read the labels. They never hold a part of the
question. A job that sutura runs at startup has no labels.

Each principal that can list the jobs of the billing project also sees the masked actor. The
masked actor shows the first character of each part of a human actor's id and where its `.` and
`@` separators are. The same principal sees the `azp` (or `client_id`) of the caller's token as it
is.

If the host cannot supply randomness for the id, sutura refuses an HTTP request with
`503 no_randomness` before it runs any job, so a job for an HTTP request always has an id. A job
that the stdio surface or the CLI runs has an empty `sutura_correlation`.

## Integration modes

An `impersonation-at-source` source declares one of three modes. A `shared-service-user` source
declares none. The table compares the four. `workload_identity` has two rows: with `delegation`, and
without it.

| Mode                                                  | Runs as                                             | Operator setup                                                                               | Operational advantage                                                       | Operational challenge                                                                   | Security advantage                                                                                                                                                 | Security limit                                                                                                                                                                                          |
| ----------------------------------------------------- | --------------------------------------------------- | -------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [`shared-service-user`](#identity)                    | The process's application default credentials       | Set `billing.service_account_project`. In `multi-user` mode, also set `acknowledged_because` | Callers need no grants of their own                                         | One identity bills and reads for every caller                                           | The mode sends no caller token to Google                                                                                                                           | BigQuery cannot tell callers apart                                                                                                                                                                      |
| [`workload_identity`](#identity) with `delegation`    | The caller's own principal in the pool              | Set up a pool, a client and a secret file per hop, and `direct` inbound                      | Callers need no Google account                                              | Each caller needs a pool principal and grants                                           | No service account at any step                                                                                                                                     | The pool provider must accept only the pool audience                                                                                                                                                    |
| [`workload_identity`](#settings) without `delegation` | The caller's own principal in the pool              | Set up a pool. With `direct` inbound, write `inbound_token_at_pool: accepted`                | Callers need no Google account. No hop and no client secret                 | Each caller needs a pool principal and grants                                           | No service account at any step                                                                                                                                     | The pool gets the token that sutura verified, as it is. With `direct` inbound, the pool provider must accept the audience of sutura itself                                                              |
| [`google_login`](#settings)                           | The caller's linked Google account                  | Set up the broker endpoint and `direct` inbound                                              | Grants that name Google accounts apply as they are                          | Each caller links an account. A long job can outlive the token                          | Grants on the caller's own Google account decide                                                                                                                   | The identity provider keeps the Google token                                                                                                                                                            |
| [`domain_delegation`](#settings)                      | The Google account with the caller's verified email | Authorize the delegating account for `bigquery.readonly` only and grant `signJwt` on it      | Grants that name Google accounts apply as they are. Callers link no account | The identity provider must not let users edit their own email. sutura cannot check this | Each caller keeps their own Google grants. The token has only the read-only scope: Google refuses it `jobs.insert`, so it cannot start a load, copy or extract job | The deployment vouches for the caller. The delegating account can act as any user in the domain. sutura cannot cancel its jobs: a job runs on after the deadline, and `max_bytes_billed` caps its bytes |

A caller that a mode cannot run is refused. No mode falls back to another.

## Sizing

One result has at most 1 000 000 rows and 256 MiB. A time column must be `DATE`: sutura refuses
`TIMESTAMP` and `DATETIME` time columns. `governance.per_replica_spend_ceiling` cannot be used with
a BigQuery source.
