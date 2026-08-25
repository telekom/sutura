---
title: Architecture
description: The settled design, the mechanisms that hold it, and what is compiled today.
---

# Architecture

This page is the settled design and the reason the repository is laid out the way it is, not an
inventory of what compiles. [What exists today](#what-exists-today) is that inventory, and it is
short. `AGENTS.md` in the repository root holds the crate table and the invariant table; this
page says why they look the way they do.

## Serving is MCP

The primary interface is an MCP server, so an agent is a first-class client rather than an
afterthought wrapped around an API built for a dashboard.

That makes the tool surface the governance boundary, and it is deliberately narrow. No tool takes
SQL, a table name, a filter expression or a list of row ids. A question names a metric, some
dimensions and a time range, and there is no field into which anything else fits. An uncertified
question is unrepresentable rather than refused: a refusal can be retried until it succeeds, and an
absent field cannot.

The tool schemas are derived from the domain types rather than written by hand, so widening the
surface changes a generated schema and shows up in the diff of the review that widened it.

An HTTP surface sits beside the MCP one for callers that are not agents: a second transport over the
same service, derived from the same types, so it cannot accept a question the MCP surface would
reject.

## Metadata sources are behind a port

Metrics, dimensions, the glossary and lineage come from a semantic layer outside this repository.
`SemanticCatalog` is the port they arrive through: one trait, implemented once per catalogue. A
directory of YAML in git and a metadata catalogue with an HTTP API are two adapters behind it, and
swapping one for the other does not touch the query path.

Two properties of that trait carry the weight:

- `load` takes no request context. A catalogue cannot see who is asking, so it cannot return a
  different definition to different callers, and a tampered catalogue cannot select what executes.
- Definitions arrive as a pinned, hashed snapshot. Arguments are validated against the bundle rather
  than a live read, so a catalogue edit cannot change what a question means between two invocations.
  It changes the digest, and the digest travels with the answer.

Nothing here edits a definition. Editing one forks the definition from the number it certifies, which
was the only thing certifying it bought.

## Data systems are behind a second port

Execution sits behind its own trait. ClickHouse and Postgres are the near-term targets; DuckDB covers
local development and single-file work, where the data is a Parquet file and there is no server to
authenticate against. The trait is named `Warehouse`, which is the port's name and not a claim about
what sits behind it.

What differs per data system is dialect, connection and how an identity is presented. Neither the
plan nor what a metric means differs; [the semantic compiler](#the-semantic-compiler) decides both.

A plan resolves to exactly one data system. Federation across two is not a smaller version of the
same problem, it is a second identity to satisfy, and a plan that cannot run as one subject in both
places is refused rather than run partly as somebody else.

## Security is the reason for the shape

The three sections above are not features arranged around a core. They are what falls out of one
requirement: an agent may be handed a database only if the database can still tell who is asking.

**The caller's identity reaches the data system.** Not a service account holding the union of
everyone's access. A credential is minted per request, and a request that cannot run as the subject
is refused rather than downgraded to the service's own identity: that downgrade turns "you may not
see these rows" into "here are the rows". A row-level security policy that holds only for human
callers is decorative.

**Authorization stays in the data system.** sutura keeps no copy of who may see what, because a copy
can disagree with the original. Grants, row-level policies and masking already exist in ClickHouse
and Postgres, administered and audited by the people who own the data; a second implementation here
would produce a second answer and no way to tell which is right. One consequence: there is no result
cache, because under row-level security a cache keyed on the query text is a cross-user leak.

**A narrow tool surface bounds a compromised agent.** The input is natural language from wherever
the user found it, so a manipulated agent is the expected case and the defence is not detecting it.
The most an attacker can make the agent emit is a different certified question, asked as the same
caller, over the same pinned definitions, against the same authorization. The blast radius of a fully
manipulated agent is the set of questions its caller could already ask.

**Returned content is untrusted input.** Rows, column descriptions and glossary text are authored by
somebody else, and any of it can contain something shaped like an instruction. A delimiter cannot
separate instruction from data, because the content can contain the delimiter, and neither can a
preamble announcing that what follows is untrusted. So the boundary is a constraint on the wire
format: results leave as Arrow with provenance in the schema metadata, a typed field a caller reads
deliberately rather than a string concatenated into the channel that carries instructions.
Descriptive text from the catalogue travels the same way, and both envelopes share one encoder, so
neither transport can grow a text-blob shortcut on its own.

## The semantic compiler

`sutura-semantic` turns a modelled question into one statement for one data system. It does not
exist yet: the crate is named in the layout table in `AGENTS.md` and nothing compiles it. The
stages below are the design.

A question names a metric, some dimensions, a grain and a bounded time range. Dimension values are
arguments, checked against an allowlist in the pinned bundle. What comes out is a statement and
the values bound to it. Nothing in between is text a caller wrote.

**Resolve.** Every name in the question is looked up in the pinned snapshot: the metric has to exist,
each dimension has to be one that metric declares, the grain has to be one it supports, the range has
to be bounded. The lookup never goes to a live catalogue read, which is what makes the answer
independent of what the catalogue says at the moment of asking. The output is a set of definitions, or
a refusal naming the argument that failed.

**Plan.** The resolved question becomes a plan: which data system owns the metric, the projection, the
grouping keys, the date predicate and its bounds, and which values become bind parameters. Two things
are settled here and nowhere else. The plan names exactly one source, so a question that would need
two identities is refused before anything runs. And every value from the question becomes a parameter,
so no caller-supplied value reaches the next stage as text. The plan holds no SQL.

**Generate.** The plan becomes one statement in one dialect, which decides identifier quoting,
placeholder syntax, date arithmetic and how an aggregate is spelled. This is the only stage that emits
SQL, and what it emits is a wrapper: a projection, a `GROUP BY`, a bounded date predicate,
parameterized values, quoted identifiers. That is all of it.

### The splice

The certified statement is spliced into that wrapper as a derived table, byte for byte, without
being parsed. It is the decision that separates this from a SQL generator.

The statement arrives as text with a digest. sutura puts it in the `FROM ( ... )` position and
generates around it. It does not read it, rewrite it, transpile it, or push a predicate into it.

Parsing it would mean re-emitting it, and re-emitting substitutes our reading of the statement for the
author's. Every round trip through an AST is a chance to change the number quietly: a window frame
read slightly differently, a null ordering normalized, an implicit cast made explicit. The digest
covers the text we were handed rather than the SQL we produced, so it would not move, and the change
would be invisible in the one place this design exists to make visible.

Three costs follow:

- Nothing is optimized across the splice boundary. A predicate in the wrapper filters the
  statement's output, not its input.
- The statement's column types are unknown without asking the data system.
- The statement has to be valid in the dialect it will run in, which makes the data system part of
  what the definition means rather than a deployment choice.

SQL goldens are regenerated and reviewed as a diff rather than typed, and they assert the
passthrough byte for byte. An anchor test re-executes each pinned statement in CI and at startup,
and a failure there fails readiness. One gap is open and recorded in `AGENTS.md`: no lint yet bans
a transpile call on the query path, so review is what catches one.

## Where the parts come from

The same line runs through every system of this kind: **semantic layer, plan, federation, dialect,
execution**. The projects worth reading sit at different points on it. The last stage is the one
none of them covers.

```mermaid
flowchart TB
    Q["a modelled question"]
    SL["semantic layer<br>what a metric means<br>Wren"]
    PL["plan<br>one source, grouping,<br>bounds, parameters<br>DataFusion"]
    FD["federation<br>where a subplan runs<br>datafusion-federation, Spice"]
    DI["dialect<br>quoting, placeholders, dates<br>polyglot"]
    EX["execution<br>as the calling principal<br>nothing above does this"]
    ST(["the certified statement,<br>spliced in unparsed"])
    A["Arrow, with provenance"]

    Q --> SL --> PL --> FD --> DI --> EX --> A
    SL -.->|bytes| ST
    ST -.-> DI
```

**The semantic layer decides what a question means.** [Wren](https://github.com/Canner/WrenAI) is
the reference for that shape: a modelling language, an engine that plans against it, and MCP as the
way an agent asks. Wren derives a metric's SQL from a model of the tables; we take the statement as
given and refuse to look inside it, which is a narrower job and a different guarantee. Wren also
carries access rules in the model, which is the middle-tier policy copy
[this design declines to keep](#security-is-the-reason-for-the-shape).

**The plan is where a question stops being text.** [DataFusion](https://datafusion.apache.org/) is
what Wren and Spice both build on: a logical plan representation, an optimizer you extend with
rules, and execution over Arrow. We do not use it. `sutura-domain` names no framework - not tokio,
not arrow, not datafusion - so the first plan will be a small type in `sutura-semantic`. What would
earn DataFusion its place later is the optimizer and the federation rule below, not the SQL
frontend.

**Federation decides where a subplan runs, and here that is a security question.**
[datafusion-federation](https://github.com/datafusion-contrib/datafusion-federation) registers an
optimizer rule that finds the largest subplan a single remote source can execute and hands it there
to run. The property we want is not latency: a predicate pushed into ClickHouse or Postgres is
evaluated under the caller's own grants, row-level policies and column masking, so excluded rows
never enter this process and there is nothing here to re-authorize. Pull the rows up and filter them
locally and the filtering is ours, which is the second policy implementation this design declines to
keep. So federation arrives after per-leg identity, and
[a plan resolves to one data system](#data-systems-are-behind-a-second-port) today.

**The dialect stage is one plan and many adapters.**
[polyglot](https://github.com/tobilg/polyglot) is a Rust transpiler between more than thirty SQL
dialects, MIT-licensed, ClickHouse, Postgres and DuckDB among them. The wrapper is exactly its shape
of problem: a projection, a `GROUP BY` and a date predicate, rendered per data system. It must not
touch the splice, because a transpiler is a parse followed by a re-emit. Today that boundary is a
review rule, not a lint.

**Execution is where the caller's identity has to arrive.**
[Spice](https://github.com/spiceai/spiceai) comes closest to everything above it: Rust,
Apache-2.0, DataFusion-based, federating across thirty-odd connectors and accelerating them by
materializing into Arrow, DuckDB, SQLite or Postgres. The federation half is what we want, and
better exercised than anything we will write soon. The acceleration half we cannot take: a
materialized copy is read under whoever refreshed it, so under row-level security it is a cross-user
leak with a refresh schedule. Spice's front door is also SQL, where ours has no field for it.

| Stage | Decided there | Ours or theirs |
| --- | --- | --- |
| Semantic layer | what a metric means | Neither. Authored upstream, arriving pinned; Wren is the reference shape |
| Plan | source, projection, grouping, bounds, parameters | Build first, adopt later: a type in `sutura-semantic`, DataFusion when a type stops being enough |
| Federation | which subplan its owner runs | Adopt, once a credential exists per leg |
| Dialect | quoting, placeholders, date arithmetic | Adopt for the wrapper, never for the splice |
| Execution | the connection, and which principal the data system sees | Build. One adapter per data system, and the per-request credential is the part nothing above provides |

That last row is why this is a repository rather than a configuration file for one of the others.

## Hexagonal by construction

Ports live in the domain crate. Adapters live outside it. The domain crate depends on neither: it
names what it needs by trait, and the binary decides which implementation is passed in.

```text
        agent                        other callers
          |                                |
      MCP server                     HTTP / OpenAPI          transport, no logic
          +----------------+----------------+
                           |
                      sutura-app                             the service, generic over ports
                           |
                     sutura-domain                           domain types + port traits
          +----------------+----------------+
          |                |                |
   SemanticCatalog     Warehouse     CredentialBroker        ports, inside the hexagon
          |                |                |
    YAML in git,      ClickHouse,      the identity          adapters, outside it
    a metadata        Postgres,          provider
    catalogue         DuckDB
```

`cargo xtask check-boundaries` enforces the direction, so the diagram cannot quietly stop being true.
It fails if `sutura-domain` acquires a framework dependency anywhere in its transitive tree, which
catches a framework reached through an innocuous crate as well as one declared outright. It fails
again if a library crate's surface stops being a typed contract: a `pub` field on a `pub struct`, a
`Result` whose error type is `String`, or a declared dynamic-error crate such as `anyhow`.

Two consequences follow from the direction rather than from taste. The domain names no framework, so
its test suite compiles nothing heavy and runs in well under a second, which is what makes it the
inner loop. And adapters are feature-gated and default-off, which is why every lint and test entry
point passes `--all-features`; [Contributing](contributing.md) has the commands.

## What ships

Four artifacts, two libc flavours on two architectures, each a distroless image holding one
binary. `cargo xtask check-workflows` fails if a workflow names a build output that does not
exist, and the `one-binary` check fails if an image carries more than the binary.

The musl artifacts replace the system allocator. musl's mallocng serialises the whole process on one
lock word: `src/malloc/mallocng/glue.h` defines `rdlock` and `wrlock` as the same exclusive lock and
`upgradelock` as a no-op, and `struct malloc_context` is a single global with no arenas and no
per-thread cache.

Measured with one binary and only threading toggled, the single-threaded control is a wash across all
three. A 48-core run is not:

| Build | 48 threads |
| --- | --- |
| glibc | 4.45s |
| musl, mallocng | 92.16s |
| musl, mimalloc | 3.83s |

92.16s is slower than musl's own single-core run, which is what one global mutex predicts. Expect
parity single-threaded and a 4-20x gap for a threaded application.

mimalloc is built with `MI_SECURE=4`: guard pages, randomised placement, encoded free lists,
double-free detection. That costs 23-43% against plain mimalloc, far more than upstream's README
claims. mimalloc-secure still lands ahead of glibc and well ahead of mallocng, and the latency
budget here is a warehouse round trip.

## What exists today

Four packages: `sutura-domain` (domain types), `sutura-cli` (the binary), `xtask` (the repo gates,
listed by `cargo xtask --help`) and `sutura-dev` (a local development CLI). Everything else named
on this page is design. The query path is not built.

`sutura-domain` holds no port traits yet, deliberately. A port exists to invert a dependency on
something outside the hexagon, and no adapter exists yet to invert. A trait with no implementor and no
caller is a guess at a signature that only the first real adapter can settle, and in a library crate
`pub` hides it from `dead_code`. Each port arrives with the adapter beneath it, and each crate with
the milestone that needs it.

What does work is the environment, the gates and the release pipeline. Every claim on this page is
meant to be held up by a mechanism rather than by intent, and a mechanism is cheaper to build before
there is code to retrofit it onto.
