---
title: Foundry
description: Experimental. Execute semantic plans over a Foundry OSv2 ontology with a deployment token.
---

# Foundry

!!! warning "Experimental"

    No real Foundry tenant has exercised this adapter. SQL goldens and transport fakes do not
    establish tenant compatibility. The feature is default-off and absent from release builds.
    Build `sutura-cli` with `--features foundry` for evaluation.

The `foundry` data system renders semantic plans as Spark SQL and submits them to Foundry's preview
[Ontology SQL API](https://www.palantir.com/docs/foundry/api/sql-queries-v2-resources/sql-queries/execute-ontology-sql-query).
The crate is `sutura-exec-foundry`. It queries ontology objects on Object Storage v2, not dataset SQL.
The tenant must enable this beta API. The deployment token needs `api:sql-queries-execute` and
`api:ontologies-read`, plus permission to read the selected objects.

The [Foundry catalog](../catalogs/foundry.md) reads metadata independently. A source can also execute
plans from a markdown catalog whose models name this source and its object/property API names.
Ontology metadata supplies no certified metrics, required filters or anchors; those must come from
the catalog that defines the questions.

## Settings

The entry reads the [settings that every data system has](../../integrations.md#data-system-settings)
and these keys:

| Key          | Type          | Default  | Meaning                                                                                                             |
| ------------ | ------------- | -------- | ------------------------------------------------------------------------------------------------------------------- |
| `endpoint`   | string        | required | `scheme://host[:port]`. HTTPS for remote hosts; HTTP only for a loopback IP address. No path, query or credentials. |
| `ontology`   | string        | required | The ontology API name or RID.                                                                                       |
| `token_file` | absolute path | required | A deployment bearer token, read once at startup. Restart to replace it.                                             |

The source's token setting is separate from the catalog's authentication settings. Catalog OAuth
client credentials do not provide a query credential. The source uses `security.outbound` for its
TLS trust and optional client certificate.

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

Only `shared-service-user` is supported. Every query presents the deployment token, so every caller
receives the data that token may read. An impersonating source is refused. No caller-token exchange
or per-caller Foundry identity is implemented.
