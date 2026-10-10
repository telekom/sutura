---
title: Concepts
description: The words on the tool surface, and what each one commits to.
---

# Concepts

The words on the tool surface, and what each one commits to.
[Architecture](architecture.md) says how the pieces fit.

## A question, and what it is made of

A question names a **metric**, some **dimensions**, a **grain** and a bounded **time range**.
That is the whole vocabulary. There is no field for SQL, a table, a filter expression or a list
of row ids, so a question outside this shape is unrepresentable, not refused.

**Metric.** A named measure somebody certified: revenue, active subscribers, churn. The name
comes from a semantic layer outside this repository, and it arrives with what the metric means.

**Dimension.** An attribute a metric declares it can be broken down by: region, product, tariff.
A dimension the metric does not declare is not a narrower question. It is a name that does not
resolve, and it comes back as a refusal naming the argument that failed. Dimension *values* are
arguments checked against an allowlist in the pinned bundle. No value is ever pasted into a query
as text.

**Grain.** The time resolution the answer is aggregated to, and one the metric supports. Daily
revenue and monthly revenue are the same metric at two grains, not two metrics.

**Time range.** Bounded, always, in two senses. It must have both ends: an unbounded range is a
table scan with a plausible name. It must also be short enough to be worth answering: two real
dates ten thousand years apart are a full scan that parses cleanly.

`Query` carries no field for SQL, a table, a predicate or a row id, so an uncertified question is
unrepresentable, not refused. `deny_unknown_fields` makes a question carrying `sql:` an error
naming the field, not one silently dropped. `TimeRange` has no unbounded form, so an unbounded
range fails to deserialize at all. The span cap is separate and sits one layer in, at resolution,
where the range is demonstrably a caller's, not an author's.

**Telling an agent all of this is a separate job from enforcing it.** An agent that has not been
told what this surface is looks for a field to put SQL in, and then reads a refusal as an outage
and retries. The types stop the damage but cannot stop the loop. So the vocabulary above is also
rendered as a system prompt from the pinned bundle and the exposed operations.
[The agent prompt](agent-prompt.md) describes what it says, what it deliberately leaves out, and
what an operator can layer on top.

## A certified definition

A **definition** is what a metric means: the measure it computes, the model it reads, the grains
it supports, the dimensions it may be broken down by, and the filters that are part of its
meaning.

That meaning is a closed vocabulary, not an expression language, so no field
exists where a catalogue could write an arbitrary expression. `Measure` is an enum of two shapes
over a `Term` enum of two terms. `RequiredFilter` is an enum of four operators.
`deny_unknown_fields` applies at every depth. sutura generates the whole statement from that, in
`sutura-semantic`, so neither caller-authored nor catalogue-authored SQL is on the path at all.

**Authored SQL.** A metric may instead carry
`authored_sql:` - a SQL expression a catalogue author wrote, for what the closed vocabulary cannot
say: a window function, a percentile, an expression over two columns. It is a *sibling* of
`measure:`, not a field on it, and exactly one of the two may be present. The fragment is admitted
as text: present, bounded, one fragment rather than a script, free of control and invisible
characters. It is pinned under the definition digest exactly as written. sutura does not compile
it. No adapter this workspace ships executes it, so a bundle that carries one is refused at
startup, naming the metric. A caller still has no field for SQL, and the agent prompt never sees
any.

An **anchor** is a known result for a metric. `sutura_app::verify_and_validate` re-executes every
metric that declares one. It returns the bundle as `Validated` only if every anchor it declares was
checked and matched. A bundle whose anchors were never checked cannot reach the query path. That
function is the only thing anywhere that produces a `Validated`, and it takes the data system as an
argument, so the type cannot be obtained unless a data system was asked.

## The pinned snapshot

Definitions do not arrive live. They arrive as a **pinned snapshot**: a bundle of definitions
with a version and a **digest** over the text. Two consequences follow:

- A catalogue edit cannot change what a question means between two invocations. It changes the
  digest, and the digest travels with the answer.
- The catalogue cannot see who is asking. The load path takes no request context, so it cannot
  return one definition to one caller and a different one to another.

`SemanticCatalog::load` takes no argument at all, so there is no request
context to pass it and none to leave out. Dimension validation reads the pinned bundle, not a live
view. The digest is taken over the canonical form of the parsed definitions. Reformatting a
document does not move it, and changing what a metric means does move it.

A result carries the version and digest of the definitions that produced it, so a number traces
back to the text that defined it. That makes "certified" checkable, not asserted.

## Refusal

A **refusal** is an answer, not an error. It is a variant of the result type, with a reason. A
caller cannot mistake it for a transport hiccup and retry until something works.

Reasons are typed, not prose, because the variant is the contract and the message is not. A
dimension that does not resolve, a value outside the allowlist, a plan that would need two data
systems, a data system this process did not open: each is its own `RefusalReason` variant.

A refusal is `ToolOutcome::Refusal { reason }`, a variant of the result rather
than an `Err`. A rejected value
is never echoed back. `DimensionValueNotAllowed` names the dimension and stops there, so caller
text cannot be reflected into a log, a UI or an agent's context.

**Audit.** Every outcome - a refusal as much as an answer - is written to an `AuditSink` before
it is returned, and the record carries the principal chain. `sutura-runtime` provides the
structured writer that a deployment gets when it attaches nothing else. sutura **retains
nothing**: the sink the deployment attaches keeps the record. A deployment behind the shared bearer
token alone records the *deployment* as the subject. A deployment that declares `security.inbound`
records the caller, from a signature.

The record names who asked and which identity each leg ran under.

## Principal, subject, and running as the caller

The **principal** is who is asking. When an agent asks for a person, the principals form a chain.
The **subject** is the identity that the data system sees: the person, not the service.

A leg runs in one of two ways:

- **secure-impersonation** (BigQuery, ClickHouse). The leg runs as the subject. On BigQuery, the
  caller's verified token is exchanged for the service account that the operator declares for that
  subject. On ClickHouse, sutura signs in as one declared user and runs each statement as the
  ClickHouse user that the operator declares for that subject (`EXECUTE AS`).
- **shared-service-user** (files, Postgres). The leg runs as the deployment's own identity for
  that source. An operator declares this in the configuration.

Where the deployment declares `security.inbound`, who is asking comes from the caller's own token.
A deployment behind only the bearer token has no per-caller identity, because that token
authenticates the deployment.

sutura never falls back to the service identity on its own. A fallback would turn "you may not see
these rows" into "here are the rows". Answering takes a request context and a credential broker.
The broker mints once for every source the plan reads. The execution port takes what the broker
produced, and no signature omits it. A subject with no credential at a source is refused as
`credential_unavailable`. An adapter refuses credential material that it has nowhere to put.

Nothing in the query path chooses an identity. `SemanticCatalog::load` takes no request context, so
a catalogue cannot return one definition to one caller and another to the next. A plan resolves to
exactly one named data system. `Secret` implements no `PartialEq` and has a hand-written `Debug`,
so nobody can compare or print credential material by accident. `RefusalReason::SourceUnavailable`
means that the plan names a data system this process did not open. It is not an identity failure.

**sutura holds no copy of who may see what.** Grants, row-level policies and masking live in the
data system. The people who own the data administer and audit them. A second copy here could
disagree with the original, with no way to tell which one is right.

## Catalogue and data system

Two ports, deliberately separate.

A **catalogue** supplies definitions: metrics, dimensions, the glossary, lineage. A directory of
documents in git and a metadata catalogue with an HTTP API are two adapters behind one trait.
Both exist, and more beside them. `sutura-catalog-local` reads a directory.
`sutura-catalog-datahub` and `sutura-catalog-openmetadata` read a metadata catalogue over HTTP.
The other `sutura-catalog-*` crates read further declaration sources.

A **data system** executes. The port is named `Warehouse`, and the name says nothing about what
sits behind it. Its adapters are different kinds of thing. `sutura-exec-datafusion` is **the
engine**: it reads CSV, Parquet and NDJSON files itself, executes the plan over Arrow and
generates no SQL. Every build links it. `sutura-exec-duckdb` is a **data source**: it renders the
plan into `DuckDB` SQL and pushes the statement down. Postgres, ClickHouse, Oracle and BigQuery
are data sources too, each behind a default-off feature. The release binary enables DuckDB,
BigQuery, Postgres and ClickHouse. A musl release links the DuckDB driver, and any other build
mounts the `libduckdb` that `SUTURA_DUCKDB_ADBC_DRIVER` names. The golden suite executes the
example corpus against Postgres and ClickHouse on a server their nix tier starts beside the suite.
BigQuery's golden cells run against a real dataset in the `bigquery-conformance` CI job and skip
elsewhere under a named exemption. Oracle's skip under one because no gate provisions an Oracle.

A plan resolves to **one** data system per leg. Spanning two is not a bigger version of the same
problem: it is a second identity to satisfy. A plan whose legs cannot all run as one subject is
refused, not run partly as somebody else.

**Answered today, for two sources.** The plan stage collects the source of the metric's own model
and of every model reached through a join into a set. Three or more sources refuse as
`PlanSpansTooManySources`. Exactly two are split into a fact leg and a lookup leg. Each leg
executes against its own data system, and the legs are joined and re-aggregated above them. The
count is computed from the plan, not asserted about it afterwards. A golden builds a two-source
catalogue to provoke the split.

**Two sources are not two identities, and that is the limit to read this with.** Every adapter a
released binary links declares that it has nowhere for a subject to arrive. So both legs of a
two-source answer run under the one identity the process itself has, and the answer records that
same shared identity twice, not two different ones. This is *single-player* federation: one
asker's question reaching two data systems, not two data systems each applying that asker's own
grants. Nothing here tests that two subjects get different rows, and nothing can until a source
can execute as the asker.

**A combined answer MAY be made of two different kinds of identity, and it says which leg was
which.** Where one leg runs under a source's acknowledged shared identity and the other as the
asking subject, the answer carries one `executed_as` entry per source naming that leg's own
posture. Only one adapter can carry a per-subject credential, so every federation across two
*different* data systems is cross-posture by construction.

**What that does and does not answer for.** A total made of rows one identity was permitted to see
plus rows another identity was permitted to see is a number neither of them is entitled to. That
arithmetic is unchanged, and the disclosure is not what answers for it: an answer's execution
record and its rows arrive in one body on both transports, so a caller who is told already has the
rows. What answers for it is that an operator declared each source's posture in writing on its own
entry before the process started. A deployment with a shared source and no acknowledgement does
not boot at all. What is *not* decidable either way is whether two legs under the same shared
posture are the same identity. Nothing here names which shared identity a source is read as.

**A deployment still holds one KIND of data system.** Two `files` sources are a two-source
deployment. A `files` source beside a warehouse is refused at startup, naming both entries. A
build whose only linked adapter cannot run a leg - which is any build without the in-process
engine - refuses a two-source question as `FederationNotExecutable` rather than answering half of
it.

## Provenance

Rows, column descriptions and glossary text are authored by somebody else, and any of it can
contain something shaped like an instruction. A delimiter cannot separate instruction from data,
because the content can contain the delimiter.

The `Warehouse` port returns a `RowSet`: typed columns and typed `Value` cells
with an enforced row width, not a text blob. An answer carries `Provenance` beside it as its own
field, not as text mixed into the rows. The definition version and digest travel there, so a
result is still not separable from what defined it.

## What you cannot ask for

Absences by design:

| Not available                                   | Why                                                                                                                                                                      |
| ----------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| SQL, a table name, a filter expression, row ids | The tool surface has no field for any of them. An uncertified question is unrepresentable, not merely refused                                                            |
| A cached result                                 | Under row-level security a query-keyed cache is a cross-user leak. There is no cache to key                                                                              |
| An edit to a definition                         | Editing one forks the definition from the number it certifies. Definitions are authored upstream                                                                         |
| A query as the service identity                 | A leg that cannot run as the subject should be refused rather than downgraded                                                                                            |
| An unbounded time range                         | A range has to be bounded to resolve at all                                                                                                                              |
| A range too long to be worth answering          | A bounded range still permits a full scan: two real dates can be ten thousand years apart. Resolution refuses a span over ten years as `TimeRangeTooLong`                |
| A definitional filter removed or renamed        | A metric's required filters are part of what it means. They are compiled into every plan for the metric, and a caller has no field that could name, select or remove one |
