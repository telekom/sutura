# Architecture

The directory layout *is* the architecture. `sutura-domain` holds the types and the port
traits; everything else is an adapter that depends on it, and nothing depends on an adapter.
That is not a convention - `cargo xtask check-boundaries` fails the build if the domain
crate acquires a framework dependency.

`AGENTS.md` in the repository root is the authoritative layout table. This page is the
reader's version of it, plus the part a table cannot say: what is on disk today.

## Ports and adapters

The domain owns the traits, the adapters implement them, and the composition happens once
in the binary.

```
                    sutura-mcp / sutura-http        (transport, no business logic)
                                |
                             sutura-app             (the service, generic over ports)
                                |
                          sutura-domain             (types + port traits)
                          /       |       \
        SemanticCatalog   |   Warehouse   |   CredentialBroker   (ports)
                |                 |
      catalog-local / -datahub    exec-duckdb / -bigquery        (adapters)
```

Consequences worth stating, because each is load-bearing rather than tidy:

- The domain names no framework. No tokio, axum, rmcp, datafusion or arrow appears in its
  manifest, so `cargo nextest run -p sutura-domain` compiles no heavy dependency and its suite runs
  in well under a second.
- Adapters are feature-gated and default-off. That is why every lint and test entry point
  passes `--all-features`: a bare `cargo clippy --workspace` would inspect almost nothing
  and still report success.
- Transport crates hold no business logic, so a governance decision cannot be made in an
  HTTP handler where no test would look for it.

## The crates

| Crate | Role | On disk |
| --- | --- | --- |
| `sutura-domain` | Types and port traits. No framework dependencies | yes |
| `sutura-cli` | The binary; composes adapters | yes |
| `xtask` | The repo gates - see [The gates](gates.md) | yes |
| `sutura-semantic` | `Query` to plan to `GeneratedQuery` | planned |
| `sutura-app` | The service, generic over ports, holding no framework types | planned |
| `sutura-catalog-local` / `sutura-catalog-datahub` | `SemanticCatalog` adapters: git YAML, or a metadata catalog | planned |
| `sutura-exec-duckdb` / `sutura-exec-bigquery` | `Warehouse` adapters | planned |
| `sutura-arrow` | `RecordBatch` to Arrow IPC and Flight SQL | planned |
| `sutura-mcp` / `sutura-http` | Transport only | planned |

A crate arrives with the milestone that needs it, not as a placeholder: an empty crate is a
compile target and a maintenance surface for no value. The planned rows are the settled
design, not a wish list - but nothing in them is running yet.

## What executes a question, once it does

The pieces above compose into one path, and the constraints on it are the point of the
project rather than a detail of it:

1. A question arrives on the tool surface. It carries no SQL, no table, no filter and no
   row-id list, because `Query` has no field for one.
2. `sutura-semantic` compiles a plan against **pinned** definitions - a hashed snapshot
   authored upstream in a semantic layer, never edited here.
3. The plan resolves to exactly one source. `PlanSources` is asserted to have length one.
4. A credential is minted for the calling principal for that leg. A leg that cannot run as
   the subject is refused, not downgraded to a service identity.
5. Rows come back as Arrow, with provenance in the schema metadata, so a result cannot be
   separated from the definition that produced it.

Every step in that list has a mechanism behind it, listed in [Invariants](invariants.md).
