---
title: Foundry
description: Experimental. Execute semantic plans over a Foundry OSv2 ontology with a deployment token or each caller's own Foundry token.
---

# Foundry

!!! warning "Experimental"

    No real Foundry tenant has exercised this adapter. SQL goldens and transport fakes do not
    establish tenant compatibility. The feature is default-off and absent from release builds.
    Build `sutura-cli` with `--features foundry` for evaluation.

The `foundry` data system renders semantic plans as Spark SQL and submits them to Foundry's preview
[Ontology SQL API](https://www.palantir.com/docs/foundry/api/sql-queries-v2-resources/sql-queries/execute-ontology-sql-query).
The crate is `sutura-exec-foundry`. It queries ontology objects on Object Storage v2, not dataset SQL.
The tenant must enable this beta API. The token a query presents needs `api:sql-queries-execute`
and `api:ontologies-read`, plus permission to read the selected objects.

The [Foundry catalog](../catalogs/foundry.md) reads metadata independently. A source can also execute
plans from a markdown catalog whose models name this source and its object/property API names.
Ontology metadata supplies no certified metrics, required filters or anchors; those must come from
the catalog that defines the questions.

## Settings

The entry reads the [settings that every data system has](../../integrations.md#data-system-settings)
and these keys:

| Key                                | Type          | Default  | Meaning                                                                                                                                                       |
| ---------------------------------- | ------------- | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `endpoint`                         | string        | required | `scheme://host[:port]`. HTTPS for remote hosts; HTTP only for a loopback IP address. No path, query or credentials.                                           |
| `ontology`                         | string        | required | The ontology API name or RID.                                                                                                                                 |
| `token_file`                       | absolute path | -        | `shared-service-user` only, and required there: a deployment bearer token, read once at startup. Restart to replace it. Refused on `impersonation-at-source`. |
| `foundry_login.token_endpoint`     | URL           | -        | `impersonation-at-source` only, and required there: the broker token endpoint `https://<host>/realms/<realm>/broker/<alias>/token`.                           |
| `foundry_login.client_id`          | string        | -        | The client ID that sutura uses at the identity provider.                                                                                                      |
| `foundry_login.client_secret_file` | absolute path | -        | A file that holds that client's secret. sutura reads it at startup.                                                                                           |

The source's token setting is separate from the catalog's authentication settings. Catalog OAuth
client credentials do not provide a query credential. The source uses `security.outbound` for its
TLS trust and optional client certificate, for Foundry and for the identity provider.

## Example

This source accompanies a catalog whose models name `source: ontology`:

```yaml
security:
  identity: "single-user"
  single_user_because: "one operator evaluating this ontology"
sources:
  ontology:
    kind: "foundry"
    endpoint: "https://foundry.example.com"
    ontology: "sales_ontology"
    token_file: "/run/sutura/foundry-query-token"
    posture: "shared-service-user"
```

## Execution and limits

The adapter sends `POST /api/v2/sqlQueries/executeOntology?preview=true`. Values remain native JSON
parameters beside the SQL statement. A preflight uses the API's parse-only mode; anchor verification
executes the plan and checks its returned rows.

Ontology SQL [caps output at 10,000 rows](https://www.palantir.com/docs/foundry/sql-warehousing/ontology-sql).
A response at the service or requested response boundary must be refused rather than treated as a
complete federated leg. Larger legs need a different completeness mechanism before they can run.
The adapter supports semantic plans and whole federated legs; raw SQL, split reads and compound-key
probes are unsupported.

Only uncompressed Arrow streams with supported scalar columns are accepted. Dictionary and nested
fields are refused. Response bodies have an 8 MiB cap; the configured runtime result budget also
applies before batch decoding and to accumulated results.

One request deadline covers the HTTP request, response and decode checks. Decoding is checked between
batches rather than interrupted inside one. Local deadline checks cannot establish
that Foundry stopped its server-side work after the client stopped waiting. SQL parsing goldens do
not prove Foundry's result types, date-bucket behavior, permissions or aggregate equivalence. A real
tenant must exercise those paths, anchors and refusal cases before this integration can leave draft.

## Identity

!!! warning "Built, not observed"

    Per-caller identity on Foundry is built and held by fakes only: a loopback identity provider
    and a recording fake of the Foundry transport. No Foundry tenant and no identity provider
    storing a Foundry login has run it, so it is not evidence that a tenant accepts a brokered
    token or that two callers read two different object sets.
    `docs/where-identity-is-proven.md` has no venue that answers yes for it.

A source runs its questions one of two ways.

**`shared-service-user`.** Every query presents the deployment token from `token_file`, so every
caller receives the data that token may read.

**`impersonation-at-source` with `foundry_login`.** Each question runs as the Foundry user the caller
linked at the identity provider. Register a Foundry third-party application with the
authorization-code grant, and add it to the identity provider as a brokered provider that stores
tokens. Each user links their Foundry account there once. Per question, sutura sends the caller's
verified inbound token to the identity provider's broker token endpoint (Keycloak's Identity
Brokering API v2, the same request as BigQuery's
[`google_login`](bigquery.md#settings)) and presents the Foundry access token that comes back as
the bearer of the Ontology SQL request and of its parse-only preflight. Foundry's own object
permissions and markings on that user decide what the question reads.

```yaml
security:
  inbound:
    mode: "direct"
    # resource, authorization_server, key set ... as for any served deployment
sources:
  ontology:
    kind: "foundry"
    endpoint: "https://foundry.example.com"
    ontology: "sales_ontology"
    posture: "impersonation-at-source"
    foundry_login:
      token_endpoint: "https://idp.example.com/realms/example/broker/foundry/token"
      client_id: "sutura"
      client_secret_file: "/run/sutura/idp-client-secret"
```

What is refused, and where:

- **At load.** `foundry_login` without `security.inbound.mode: direct`; `foundry_login` on a
  `shared-service-user` source or on any kind but `foundry`; `token_file` or
  `verification_identity` beside it; `workload_identity` on a `foundry` source; and an
  impersonating `foundry` source without `foundry_login`. The source holds no identity of its own,
  so a catalog anchor on it does not boot - an anchor cannot be re-run as anybody.
- **Per question.** A request with no verified caller is refused before any broker request. A caller
  who has not linked a Foundry account gets `403 identity_not_linked`. Any other broker failure, and
  a brokered token whose `accessTokenExpiration` has already passed, is `503 identity_unavailable`
  and sends nothing to Foundry. A Foundry `401` or `403` under the caller's token is
  `403 source_refused`, never an empty answer. No request falls back to a deployment token: an
  impersonating source has none.
- **Recorded.** `executed_as` names the leg `impersonation-at-source`. A federated answer that mixes it
  with a shared leg is answered, and names which leg ran as whom, as for every other source.

Limits:

- sutura sends the brokered token as it is, never logs it and never puts it in a refusal. It does
  not refresh it: refreshing is the identity provider's job. A token that expires during a question
  fails at Foundry.
- The brokered token is opaque to sutura. Whether it carries the scopes `executeOntology` needs is
  the Foundry application's registration, and is unverified. The broker answer must carry
  `accessTokenExpiration`; whether the identity provider's generic OIDC or OAuth 2 provider writes
  it for a Foundry login is also unverified, and an answer without it is refused.
- The `sutura` command (`query`, `mcp`) attaches no broker and refuses an impersonating Foundry
  source. Only `sutura serve` runs one.
- The [Foundry catalog](../catalogs/foundry.md) stays deployment-scoped: it reads metadata as its
  service user, and every caller sees the same models. A per-caller catalog visibility filter is
  out of scope and needs an ADR. A caller can therefore see a model whose objects Foundry will then
  refuse them; no rows reach them, but the model's name and columns do.
- An ADBC or Arrow Flight transport would take the same bearer. Whether Foundry's Flight endpoint
  accepts a third-party application's user token is unverified.
