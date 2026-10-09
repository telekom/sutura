---
title: Architecture
description: How sutura is built - the crates, the ports between them, and the path of one question.
---

# Architecture

sutura is one binary built from a set of Rust crates. The crates follow a ports-and-adapters
(hexagonal) design. The domain crate defines the ports. Each adapter implements one port for one
metadata source or one data system.

<iframe
  src="../assets/architecture.html"
  title="sutura crates and ports"
  loading="lazy"
  style="width: 100%; height: 820px; border: 0;"
></iframe>

[Open the diagram on its own page](assets/architecture.html). The diagram works without
JavaScript; the script only highlights the connections of the crate you point at.

## The crates

The crate name prefix tells you the role of the crate.

| Crate              | Role                                                                                                  |
| ------------------ | ----------------------------------------------------------------------------------------------------- |
| `sutura-domain`    | The interior. Domain types and the three ports `SemanticCatalog`, `Warehouse` and `CredentialBroker`. |
| `sutura-semantic`  | Compiles a question into a plan.                                                                      |
| `sutura-sql`       | Renders a plan as one SQL statement in one dialect.                                                   |
| `sutura-app`       | The service. It answers a question through the ports, and defines the `Surface` driving port.         |
| `sutura-http`      | The HTTP transport: the `/v1` API, the OpenAPI description, and the agent surface at `/mcp`.          |
| `sutura-mcp`       | The MCP tool surface, over standard input and output or over HTTP.                                    |
| `sutura-catalog-*` | Metadata adapters. Each one reads definitions from one kind of catalog.                               |
| `sutura-exec-*`    | Data-system adapters. Each one runs a plan on one kind of data system.                                |
| `sutura-config`    | Reads the settings, and refuses to start on an unsafe or incomplete configuration.                    |
| `sutura-runtime`   | Process-wide parts: admission control, the blocking pool, metrics.                                    |
| `sutura-cli`       | The composition root. It links the adapters and connects them to the ports.                           |

The dependencies point inward. `sutura-domain` depends on no other sutura crate and on no
framework. Only `sutura-cli` links the catalog and data-system adapters, and no adapter depends on
another adapter of its kind. `cargo xtask check-boundaries` fails when one of these rules breaks.
[Integrations](integrations/index.md) has one page for each adapter.

## The path of a question

1. A caller sends a question to `sutura-http` or `sutura-mcp`. A question names metrics,
   dimensions, a time grain and a bounded time range.
2. The transport passes the question to `sutura-app` through the `Surface` port.
3. `sutura-semantic` resolves every name against the pinned catalog snapshot and makes a plan. The
   plan names the data system, the columns, the grouping, the date bounds and the bind parameters.
4. `sutura-app` gets one credential for each data system that the plan reads, from the
   `CredentialBroker` port.
5. The `Warehouse` adapter for that data system runs the plan. A SQL adapter renders the plan
   through `sutura-sql`. The DataFusion adapter runs the plan directly and renders no SQL.
6. The rows return as Arrow record batches. The transport converts them to rows. The answer
   includes the digest of the catalog snapshot. A federated answer also reports the identity
   posture of each leg.

A plan reads one or two data systems. A plan that reads two is a federated plan: each data system
runs its part (a leg), and DataFusion joins the legs. sutura refuses a leg on a data system whose
adapter cannot run one.

## The question surface is closed

No tool and no endpoint takes SQL, a table name, a filter expression or a list of row IDs. The
question type has no field for them, and `deny_unknown_fields` refuses a question that carries
one. Every value in a question becomes a bind parameter. A golden test checks that no value from
a question appears as text in the statement that sutura generates for it.

So an agent that is manipulated can only ask a different certified question over the same
definitions. It cannot make sutura run arbitrary SQL. The raw SQL tool is an exception that is
off by default; [Serving over HTTP](serving.md#the-raw-sql-tool-over-a-duckdb-source) describes
it.

The tool schemas come from the Rust types. A snapshot test fails when a field is added or changed,
so a change to the surface shows in review.

## Catalogs are pinned snapshots

A metadata adapter implements `SemanticCatalog`. It loads the definitions once, as a pinned and
hashed snapshot. Two rules follow:

- `load` takes no request context. A catalog cannot see who asks, so it cannot give different
  definitions to different callers.
- sutura checks a question against the snapshot, not against a live read. A catalog edit changes
  the digest, and the digest goes with every answer.

Each adapter declares which kinds of definition it can supply: structure, descriptions,
relationships, cardinality, metrics, filters, grains, value allowlists, anchors, column types and
column descriptions. The declaration is required. A catalog that supplies part of the model
declares the rest as absent. The conformance test checks that an adapter produces exactly what it
declares.

## Identity

sutura separates two claims about identity:

- **Leg 1: sutura knows who asks.** This is built. With `security.inbound`, sutura verifies the
  caller's token before it answers. [Inbound identity](integrations/identity.md) has the settings.
- **Leg 2: a data system runs the query as the caller.** This is built for BigQuery
  (secure-impersonation). The BigQuery adapter sends the caller's verified assertion through the
  account that the source maps for that caller. sutura refuses a caller that the map does not
  declare.

Every other data system runs as one identity that the deployment declares for that source. An
operator acknowledges that shared identity in the configuration, and the answer reports it.

sutura keeps no copy of who may see which rows. Grants, row policies and masking stay in the data
system. For this reason there is no result cache: under row-level security, a cache keyed on the
question would leak rows between callers.

## What ships

Each release has four artifacts: musl and glibc builds for x86_64 and aarch64. Each artifact is
a distroless image that holds only the binary. The release binary links the features that
`nix/shipped.nix` lists, and `sutura doctor` shows which adapters a binary links.
`just build-release` builds the release binary, and Nix compiles and links the ADBC drivers into it.

In the development shell, a `cargo` build can select one feature:

```bash
cargo build --release -p sutura-cli --features bigquery
```

That build links no driver. It loads the driver from the file that `SUTURA_BIGQUERY_ADBC_DRIVER`
names.

The musl artifacts use mimalloc instead of the musl allocator. The musl allocator puts the whole
process behind one lock, which makes a threaded program slow. Measured with one binary on 48
threads:

| Build          | 48 threads |
| -------------- | ---------- |
| glibc          | 4.45s      |
| musl, mallocng | 92.16s     |
| musl, mimalloc | 3.83s      |

mimalloc is built with `MI_SECURE=4`: guard pages, random placement, encoded free lists and
double-free detection. This costs 23-43% against plain mimalloc, and it is still faster than glibc.

## What exists today

- The shipped binary serves HTTP and MCP, compiles questions and runs local files through
  DataFusion. It links the adapters that `nix/shipped.nix` lists, and
  [Integrations](integrations/index.md) describes each adapter.
- `security.inbound` verifies a caller (leg 1). On BigQuery, a query can run as the caller's mapped
  account (leg 2). Every other source runs as its declared identity.
- Results leave the process as rows, not as Arrow. Arrow is used inside the process only.
- A metric is a computation over a model. sutura cannot take a SQL statement from a catalog and
  splice it into its query: the metric type has no field for a statement.
- A source entry says where the data is. It does not prove that the data matches what the catalog
  certifies. Only an anchor checks this, and a metric without an anchor gets no check.
