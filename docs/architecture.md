---
title: Architecture
description: The settled design, the mechanisms that hold it, and what is compiled today.
---

# Architecture

This page is the settled design and the reason the repository is laid out the way it is, not an
inventory of what compiles. [What exists today](#what-exists-today) is that inventory, and it is
short. `AGENTS.md` in the repository root holds the crate table and the invariant table; this page
says why they look the way they do.

!!! warning "Read every section here as a design, not as a control"

    Most of this page describes a system that is not built. What exists is a governed single-player
    semantic compiler and executor over local files, served over HTTP - behind a token that
    authenticates the DEPLOYMENT, unless the deployment declares `security.inbound` and verifies a
    caller's own token. A request context, a credential broker, an audit sink and an MCP surface all
    exist now; what does not is an adapter that can carry a per-subject credential, so every question
    still reads as one identity. No Arrow result envelope. Sections that describe something enforced
    today say so inside the section, and [What exists today](#what-exists-today) is the inventory.
    **Do not deep-link a section of this page as evidence that a control is in place.**

## Serving is MCP

**Both transports are built, and one property this section describes is not.** `sutura-http` serves
a versioned `v1` tree, a liveness probe, a generated interface description, rate limiting and a
bearer gate; `sutura-mcp` serves the tool surface over a process's own standard input and output,
and `just mcp-e2e` drives that one end to end. What is absent is a caller identity on the agent
surface: a pipe has no header a token could arrive in, so it answers as the deployment and offers
every capability, and a network-reachable agent surface needs the identity leg
[how a caller proves who it is](adr/0014-how-a-caller-proves-who-it-is.md) designs.

The primary interface is an MCP server, so an agent is a first-class client rather than an
afterthought wrapped around an API built for a dashboard.

That makes the tool surface the governance boundary, and it is deliberately narrow. No tool takes
SQL, a table name, a filter expression or a list of row ids. A question names a metric, some
dimensions and a time range, and there is no field into which anything else fits. An uncertified
question is unrepresentable rather than refused: a refusal can be retried until it succeeds, and an
absent field cannot.

That last property is the part that **is** enforced today, transport or no transport: `Query` is the
tool surface, it declares no such field, and `deny_unknown_fields` makes a question carrying one an
error naming it. A transport can only narrow what the type already refuses.

The tool schemas are derived from the types rather than written by hand, so widening the surface
changes a generated schema and shows up in the diff of the review that widened it. That dump is
written: `sutura-mcp` generates each tool's input schema with `schemars` over the wire type the
deserializer uses, and snapshots it, so a new or widened field fails the byte-compare until
somebody re-accepts the snapshot. **What review still owns** is whether a widened field should
exist - a snapshot can be re-accepted without thought, which is why one test asserts the question
tool's fields by name instead of by bytes.

An HTTP surface sits beside the MCP one for callers that are not agents: a second transport over the
same service, derived from the same types, so it cannot accept a question the MCP surface would
reject.

## Metadata sources are behind a port

Metrics, dimensions and the glossary come from a catalogue, and `SemanticCatalog` is the port they
arrive through: one trait, implemented once per catalogue. A directory of documents in git and a
metadata catalogue with an HTTP API are two adapters behind it, and swapping one for the other does
not touch the query path.

**And an adapter behind it says what it can supply, because the absence is the part that matters.**
`SemanticCatalog::capabilities` is required with no default: an adapter that omitted it would not
compile. It names which of nine definition kinds - structure, descriptions, relationships, a join's
cardinality, metrics, definitional filters, grains, value allowlists, anchors - and which of the four
knowledge kinds this source can carry at all. An empty collection is two different facts: a reviewed
catalogue that has not certified a metric yet, and a source that holds a measure this runtime will
not execute. `sutura-catalog-local` declares every kind, a statement about the format rather than
about the directory it read; a source that supplies part of the model declares the part, and its
conformance test is that what it declared is exactly what it produced.
[What DataHub can carry](adr/0016-what-datahub-can-carry.md) is the measurement this came from.

**This sentence used to say lineage arrives through that port too, and it does not.** There is no
lineage type anywhere in the workspace and none is planned: a plan reads at most two data systems, a
measure reads columns a model declares, and where a column came from upstream changes neither. It is
real metadata that real catalogues carry - [what DataHub can carry](adr/0016-what-datahub-can-carry.md)
reads three lineage aspects out of one of them - and this port has no shape to put it in.

The catalogue may be outside this repository or in it, and
[the first-party models decision](adr/0001-first-party-semantic-models.md) is why both are allowed.
What the port guarantees is the same either way.

Two properties of that trait carry the weight:

- `load` takes no request context. A catalogue cannot see who is asking, so it cannot return a
  different definition to different callers, and a tampered catalogue cannot select what executes.
- Definitions arrive as a pinned, hashed snapshot. Arguments are validated against the bundle rather
  than a live read, so a catalogue edit cannot change what a question means between two invocations.
  It changes the digest, and the digest travels with the answer.

Nothing here edits a definition that arrived rendered. Editing one forks it from the number it
certifies, which was the only thing certifying it bought. A first-party model is a different case: it
is authored here, so reviewing a change to it is reviewing what will execute, and that review belongs
in the same pull request as any other change to behaviour.

## The engine, and the data systems behind a port

Two different things, and conflating them is the mistake this section exists to prevent.

**The engine is one thing: DataFusion, with `polyglot-sql` rendering SQL when a query is pushed
down.** Not one engine per data system, and not a second engine kept in step with a first. It
executes what it must locally over Arrow, and it is where a plan becomes rows.

**A data system is a place data already lives** - DuckDB, Postgres, ClickHouse, BigQuery - that
somebody wants queried. Those sit behind the `Warehouse` port. What differs per data system is
dialect, connection and how an identity is presented; neither the plan nor what a metric means
differs, because [the semantic compiler](#the-semantic-compiler) decides both.

**The port carries a plan, not a rendered statement**, and that is load-bearing rather than tidy.
Taking a statement asserted that every data system speaks SQL. The engine does not: it executes a
logical plan over Arrow and never sees a string. So the plan is the contract and rendering is the
adapter's private business, which is what makes an adapter possible that cannot have a dialect bug,
because it emits no dialect. [DataFusion for local execution](adr/0003-datafusion-for-local-execution.md)
is the record.

### Where the engine sits, and where it is going

Today the engine is *behind* the port, as one adapter among the others, executing local files. Call
that a stepping stone: an engine belongs **above** the port, deciding which subplan each data system
runs and executing the remainder itself. That is what federation means here, and it is the shape the
surveyed projects converge on. Today's arrangement is the engine with zero remote sources.

### Connectors: Arrow Flight, not a driver per data system

The intended transport to a data system is **Arrow Flight SQL**, uniformly - DuckDB, Postgres and
BigQuery alike - rather than a linked native driver each.

Three reasons, and the third is the one that shows up as code today. Arrow is already the result
format, so a Flight response needs no row-by-row conversion. A Flight endpoint is reachable without
compiling a C library, which is what makes a statically linked artifact possible at all. And a driver
per data system means a *type mapping* per data system: the two adapters that exist already had to
decide independently what a boolean and an exact decimal become, and they agreed only because one was
written while reading the other. One wire format decides that once.

Taken seriously, that reaches back into the port. Results leave sutura as Arrow with provenance in
the schema metadata, and a Flight response arrives as Arrow, so a row-oriented type in the middle is
a conversion in and a conversion out for no benefit. The `RowSet` and `Value` the port returns today
are exactly that middle, and the cost is already visible: two adapters had to decide independently
what a boolean and an exact decimal become.

The obstacle is the one the layout exists to enforce - the domain may not name a framework, and
`arrow` is one. The way through is that **Arrow IPC is a serialization format, not a library type**: a
port can return encoded bytes plus the provenance that must travel with them, which is lossless,
names no framework, and makes Arrow the format end to end. `sutura-arrow` then owns encode and decode,
and anything wanting typed values - the table a CLI prints - becomes a consumer of it rather than of a
bespoke row type.

Not built. `RowSet` is what exists, and it is honest about being a row type. Recorded here because the
direction decides whether it grows a `Boolean` variant and an exact decimal, or is replaced.

Until then a data system's driver is a **development dependency** - present to prove that the SQL we
render actually runs, which is a real job and the reason a data-system adapter exists at all today.
It is not in the shipped binary, and the shipped binary is not poorer for it: the engine reads CSV and
Parquet itself.

A plan resolves to exactly one data system. Federation across two is not a smaller version of the
same problem, it is a second identity to satisfy, and a plan that cannot run as one subject in both
places is refused rather than run partly as somebody else. **The one-source rule is enforced
today**, by the plan stage and a golden that builds a two-source catalogue to provoke the refusal;
the identity reasoning behind it is a design target, because there is no per-leg credential to test
against.

### What can be plugged in today, and what the shipped binary actually uses

Three adapters exist. **They are chosen at compile time, in the composition root - there is no
configuration that names one.** If you are looking for a setting to point sutura at a different
catalogue or a different data system, there is not one yet.

| Port | Adapter | What it is | In the shipped binary? |
| --- | --- | --- | --- |
| `SemanticCatalog` | `sutura-catalog-local` | A directory of markdown documents with YAML frontmatter, read off disk | **Yes.** The only catalogue adapter there is |
| `Warehouse` | `sutura-exec-datafusion` | THE ENGINE. Reads the CSV and Parquet files itself and executes the plan over Arrow. Generates no SQL | **Yes**, and it is what `sutura query` runs |
| `Warehouse` | `sutura-exec-duckdb` | A DATA SOURCE. Renders the plan into `DuckDB` SQL and pushes the statement down | **No.** A development dependency of `sutura-app` |

So the combination a PUBLISHED binary supports is **local markdown with YAML frontmatter for the
metadata, and the in-process engine over the CSV or Parquet files in a directory**. `sutura query
<catalog-dir> <question.yaml> [data-dir]` is the whole of it there, and `sutura doctor` says the same
thing in one line: `data systems : none - this build reads files, and pushes down to nothing`. A
source build carrying `--features bigquery` supports one more, and the paragraph below says what that
is and what has never been run.

**The directory is optional because that command reads the same `sources:` tree the server does.** A
catalog whose models name `warehouse` is answered when `sources.warehouse` declares a `files`
directory beside it, so the name a catalog uses is no longer required to be `local`; a caller with no
configuration at all passes the directory instead, and that is the built-in declaration - a `files`
source called `local`, over the directory given, read as whoever ran the command. Both, for one
source, is refused as two answers to one question.

**And `kind: bigquery` is openable from that binary too, behind a default-off `bigquery` feature.**
`sutura query` then renders the plan into `GoogleSQL` and submits it to the dataset - **which no
automated test in this repository has ever watched answer.** The furthest any of them reaches is
reading the credential file, because the transport's host is a compile-time constant with no loopback
to point at; what a real dataset HAS accepted is the corpus, on the adapter's own suite, through
`just bigquery-acceptance`. So this is a composition that is tested and a path that is not - which is
what makes "pushes down to nothing" above a statement about the DEFAULT build rather than about the
code. No published artefact carries the feature: `nix/shipped.nix` builds both binaries with cargo's
default features, because `--features bigquery` compiles `ring` from C and assembly and two of the
four release triples are musl. On a build without it, a `kind: bigquery` source is a refusal naming
the feature; `sutura doctor` prints which of the two builds you are holding.

**Two things are easy to read as more than they are, and both are worth being exact about.**

*`DuckDB` is a test dependency, not the runtime data source.* An earlier example did run through it,
and the description outlived the code. The `DuckDB` adapter is still compiled and still executes on
every run of the test suite - it is what proves the SQL we render actually runs somewhere, and the
differential test runs one plan both ways and compares the rows - but the binary does not link it and
an operator needs no `libduckdb` to run `sutura query`. That is also what keeps the musl artifacts
building: nixpkgs has no musl `libduckdb`, and the binary never asks for one.

*Four dialects are four rendering targets, not four data systems.* `sutura compile` will render a
statement for `DuckDB`, Postgres, `ClickHouse` or `BigQuery`, and the goldens parse-check each one.
Rendering `ClickHouse` SQL is not a claim that a `ClickHouse` exists anywhere, and there is no
`ClickHouse` adapter: the port takes a plan, and rendering is one adapter's private business.

**`BigQuery` is the one where that distinction has a nearer edge, so it is worth stating - and the
edge moved once, without the distinction moving with it.** A `BigQuery` adapter *does* exist,
`sutura-exec-bigquery`, and since [`adr/0018`](adr/0018-what-the-bigquery-wire-is-built-from.md) it
also has a transport that speaks to the endpoint: `jobs.query` over a blocking HTTP client, behind a
default-off `wire` feature, with a second narrow port for the credential.

**A statement generated here has now been accepted by a real dataset**, on 2026-08-30, under a
service-account key -
[`adr/0017`](adr/0017-what-a-bigquery-test-runs-against.md)'s amendment records it and puts the repeat
in CI. **Whether a build can REACH it is now a build's question rather than the repository's**, and
that is the fact this paragraph used to state the other way round: both composition roots register the
adapter behind a default-off `bigquery` feature, so a build that carries it opens `kind: bigquery`
and a build without it - every published artefact - refuses that entry by name, naming the feature.
The `data_systems:` axis of the golden matrix still gains no entry: one live statement, and a
composition no automated test has watched answer, is not a registered data system.

**And that leg is a SMOKE test rather than the acceptance leg 0017 specifies**, which is worth knowing
before reading its green as closing the gap: one hand-built `SUM` over a two-column table, exercising
none of the constructs the parse check was measured to be blind about. The wider leg is #78's importer
shape pointed at a dataset, and **it is built** - `crates/sutura-exec-bigquery/tests/corpus.rs`, run
green against a real dataset, which is where those constructs are covered and where the one divergence
it found is recorded. **The limit next to that: every test in it is `#[ignore]`d and it is outside
`just validate`**, because a nix check has no network - it runs by binary selection in the acceptance
tier, on a push that touches this data source, so a green `just validate` says nothing about it. This
sentence said *it is not built* after that landed;
[`adr/0017`](adr/0017-what-a-bigquery-test-runs-against.md)'s third amendment is the record and says
which of its four bullets the run answered and which it did not.

**One thing that leg now does prove, and it is the reason it grew:** the same table read by its
**fully qualified** `project.dataset.table` name answers the same numbers as the unqualified read, and
a qualified path naming a dataset that is not there is refused - which is the control that makes the
first half a measurement rather than an inference. So *qualification resolves*, not merely renders.
[`adr/0019`](adr/0019-a-table-outside-the-connections-dataset.md) is that record and says what the run
does not cover: no second dataset and no second project, because the acceptance credential's IAM
refuses `datasets.create`.

0017 also records how much narrower parse-checking is than acceptance - measured, not assumed: within
one target the parse check cannot see a function's argument order.

**A catalogue may name its data system anything, and an UNDECLARED name is still refused - both
halves matter, and this paragraph said the opposite of each until #121.** A declared source is opened
under its own name and its own posture; a source no `sources:` entry names falls back to the
command-line tool's built-in `files` declaration, which answers to `local` only.

Why the undeclared name is still checked: `sutura_app::answer` and `verify_anchors` both do
`warehouses.get(plan.source())` - a LOOKUP keyed on the name the catalogue declared, not a
comparison. (An earlier version of this paragraph named a `plan.source() != warehouse.source()`
guard, which exists nowhere in the tree; `sutura-app`'s own source says *"The plan SELECTS its
warehouse - it is not compared against one."*) So an engine registered under whatever the catalogue
said makes that lookup succeed by construction, and a catalogue naming a production warehouse would
get its certified metric answered out of the caller's files under that bundle's digest. An engine
registered under a fixed name misses instead, and a miss is a refusal - `SourceUnavailable` from a
question, `SourceNotConfigured` from an anchor.

**The limit a declaration does NOT close**, stated next to the claim: an entry says where a data
system is, never that the files there hold what the bundle certifies. The only thing that checks
content is an ANCHOR, and `verify_anchors` walks the metrics that declare one - so a bundle of
unanchored metrics is answered under its real digest out of whatever directory the entry points at.
That is true of `sutura-serve` too and always was; #121 is the change that makes it the documented
command-line workflow.

**What it costs to add a fourth adapter.** A `Warehouse` or `SemanticCatalog` implementation, one line
in the workspace manifest, one line in the composition root - and, in the test suite, one `impl` of
`adapters::CatalogUnderTest` or `adapters::DataSystemUnderTest` plus one line in
`adapters::registered`. For a catalogue there is one more thing and it is a line rather than a body:
the capability declaration, which the port requires with no default. No test body changes: the golden
corpus, the refusal corpus, the anchor check and the engine comparison are all expanded once per
registration, so a new adapter arrives with all of them already pointed at it - and for a catalogue
the declaration is what selects between agreeing with the hand-written oracle, which is the reference
adapters' contract, and being held to its own declaration, which is what a source supplying part of
the model gets. That is the property the two ports exist for, and it is the one worth checking has not
quietly stopped being true.

## Security is the reason for the shape

The three sections above are not features arranged around a core. They are what falls out of one
requirement: an agent may be handed a database only if the database can still tell who is asking.

Of the four properties below, two are enforced today, one is half built and one is not built. Each
says which, because a reader who lands on this section from a search result gets no other warning.

**The caller's identity reaches the data system. Half built, and the missing half is the point.** Not
a service account holding the union of everyone's access. A credential is minted per request, and a
request that cannot run as the subject is refused rather than downgraded to the service's own
identity: that downgrade turns "you may not see these rows" into "here are the rows". A row-level
security policy that holds only for human callers is decorative.

*Built:* the mechanism that removes the downgrade. A request context reaches the query path, a
credential broker mints once per answer for every source the plan reads, and `Warehouse::execute` has
no signature that omits the result - so **no question executes as this process**, and a subject with
no credential at a source is refused as `credential_unavailable`. Each adapter matches exhaustively on
what it was handed, refuses credential material it cannot use as an error rather than ignoring it, and
compares the leg against the posture it was opened with - so a shared leg carrying somebody else's
operator acknowledgement is refused too, rather than executed and then recorded under the adapter's
own declaration.

*The one path that runs with no credential, said here because the sentence above is only true with
it:* the boot path re-executes every anchor before a listener is bound, and there is no caller then, so
`Warehouse::verify_anchor` takes no credential at all.

**What bounds it is a lint, and its input type is a self-check rather than a barrier - a correction a
second review forced on this page.** This paragraph used to say the method "cannot be handed a
question" because it takes an `AnchorPlan`; a reviewer disproved that in one function by fabricating
the tuple the constructor took. The constructor is public, every value it reads is publicly
constructible, and Rust has no cross-crate friend visibility, so no arrangement of guards there can
be an authority. Two mechanisms now, stated apart:

- `clippy.toml` bans `Warehouse::verify_anchor`. `sutura_app::verify_anchors` holds the single
  `#[expect]`, so a second call site anywhere in the workspace is a build error until somebody writes
  a second expectation a reviewer sees in the diff. **That is what makes the path boot-only.** Its
  limit: a lint is not a type - it reaches this workspace, and an `#[allow]` walks past it.
- `AnchorPlan::of` checks that the boot path compiled the question it meant to, reading everything it
  compares off the pinned bundle rather than taking it as an argument: the metric has to be defined
  and anchored, the range has to be the one that bundle certifies, the grain has to be the coarsest
  that metric declares, and there may be no group-by key and no predicate a question asked for. The
  grain check closed a real gap - a `Day`-grain plan over the anchor's range used to pass and come
  back as a series rather than the one certified number.

*Not built:* an adapter that can carry a per-subject credential. Both in this build declare that they
have nowhere for one to arrive, and the broker that ships mints from configuration. So the identity a
leg presents is *the one this deployment holds for that source*, acknowledged by an operator and
recorded on the answer - which is honest and is not impersonation. Against a local file the property
is trivially satisfied and buys nothing, since a file has no login. What the port bought is that the
day a source with grants arrives, there is no path for it to be read as this process through.

What is enforced beside that, and worth stating as such: nothing on the query path can **choose** an
identity, because `SemanticCatalog::load` takes no request context and a plan resolves to exactly one
named source.

**Authorization stays in the data system. Enforced by absence, today.** sutura keeps no copy of who
may see what, because a copy can disagree with the original. Grants, row-level policies and masking
already exist in ClickHouse and Postgres, administered and audited by the people who own the data; a
second implementation here would produce a second answer and no way to tell which is right. One
consequence: there is no result cache, because under row-level security a cache keyed on the query
text is a cross-user leak. No mechanism can prove an absence, so adding either a policy store or a
cache of rows is an architecture decision rather than a feature.

**A narrow tool surface bounds a compromised agent. Enforced today.** The input is natural language
from wherever the user found it, so a manipulated agent is the expected case and the defence is not
detecting it. The most an attacker can make the agent emit is a different certified question over
the same pinned definitions: `Query` has no field for SQL, a table, a predicate or a row id, every
value it carries binds as a parameter, and a golden asserts over the whole corpus that no literal a
question carries appears in the statement generated for it.

The clause "asked as the same caller, against the same authorization" is the part that is **not**
enforced, and it is the same gap as the first property. Today the bound is what an attacker can make
sutura *ask*, and nothing constrains whose rows come back.

**Returned content is untrusted input. Partly enforced today.** Rows, column descriptions and
glossary text are authored by somebody else, and any of it can contain something shaped like an
instruction. A delimiter cannot separate instruction from data, because the content can contain the
delimiter, and neither can a preamble announcing that what follows is untrusted.

*Design target:* the boundary is a constraint on the wire format - results leave as Arrow with
provenance in the schema metadata, a typed field a caller reads deliberately rather than a string
concatenated into the channel that carries instructions, with descriptive text travelling the same
way and both envelopes sharing one encoder so neither transport can grow a text-blob shortcut on its
own. There is no Arrow envelope, no encoder and no transport to carry one.

*Enforced today:* the `Warehouse` port returns a `RowSet` of typed columns and typed `Value` cells
rather than a text blob, and provenance rides beside the rows as its own typed field rather than as
text mixed into them. The shape of the guarantee is right; the envelope is a row type rather than an
Arrow schema. The engine executing over Arrow *in process* is a different claim from results leaving
as Arrow, and only the first is true.

## The semantic compiler

A modelled question becomes one statement for one data system in three stages, and they live in
**two** crates rather than one. `sutura-semantic` is the first two, named after them; `sutura-sql` is
the third. The split is the dependency and not tidiness - see **Generate** below.

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
are settled here and nowhere else. The plan names one source per leg and at most two legs, so a
question that would read from a third data system is refused before anything runs - and whether those
two legs would decide identity the same way is not a plan-stage fact at all, because a posture belongs
to an opened adapter rather than to a plan; that is refused where the legs are assembled, above the
credential mint. And every value from the question becomes a parameter,
so no caller-supplied value reaches the next stage as text. The plan also records where each predicate
came from, definitional or requested, because a predicate that is part of what a metric means is not
one a caller chose and must not be removable. The plan holds no SQL, its type is a domain type because
[the execution port carries it](#the-engine-and-the-data-systems-behind-a-port), and its serialized
form is what a golden snapshot pins.

**Generate.** The plan becomes one statement in one dialect, which decides identifier quoting,
placeholder syntax, date arithmetic and how an aggregate is spelled. This is the only stage that emits
SQL, and an adapter that executes a plan directly never reaches it.

**And it is its own crate, `sutura-sql`, for exactly that last reason.** Rendering was already off the
compile path - `compile` returns a plan, because that is what the execution port carries - but it was
still a public module of the compiler, so the SQL generator sat in the dependency closure of every
consumer of `sutura-semantic`. That included the HTTP binary, which executes plans on the engine,
renders nothing, and can reach none of that code. Sharing the quoting and placeholder decisions
between SQL adapters is a good reason for the code to be shared and not a reason for it to sit in the
core; a separate crate gives the same sharing with the generator out of the core's closure. The
direction is a gate rather than a comment: `cargo xtask check-boundaries` fails if `sutura-semantic`
can reach `sutura-sql` or the dialect layer, over the whole transitive tree.

### Two ways a definition arrives

The stages above are the same either way. What differs is who wrote the statement.

**A first-party model.** The catalog declares models, relationships and metrics, and the generator
produces the whole statement from them. This is the path that is built, and
[The first-party models decision](adr/0001-first-party-semantic-models.md) is the record: it exists
because the spliced path below has a precondition - something upstream must already have rendered
dialect-correct SQL - and on a laptop, or over a single file, there is no upstream to have done it.

Its load-bearing constraint is that **a model may not contain a free-text SQL expression.** A measure
is one of two closed shapes over a `Term` of two terms - a term is one aggregate over a named column
or a conditional count, and a measure is either a single term or a ratio of two of them, which is what
lets a conditional count be half of a ratio rather than only a whole measure
([decision 0002](adr/0002-a-closed-vocabulary-for-measures.md) records why that was factored one level
lower than it first was); a relationship is a pair of columns and a join type; a
dimension is a column, optionally one declared relationship away. A metric may also carry required
filters, predicates from a closed set of four operators that are **part of what the metric means
rather than something a caller asks for**: they are applied to every question about it, and a caller
cannot see, choose or remove one.
[A closed vocabulary for measures](adr/0002-a-closed-vocabulary-for-measures.md) is the record of why
the vocabulary is a closed set of *shapes* rather than a single aggregate. The property being defended
was always no free-text SQL, and one aggregate over one column was a narrow means to it that could
express two of a real semantic layer's seven metrics.

The cost is unchanged for the closed vocabulary: an expression over two columns cannot be said in a
`Measure`, and neither can a window function.

**There is now a second, explicitly-named way to say them, and it is a separate shape rather than a
widening of the one above.** `Computation` has two variants - `measure:` for the closed vocabulary and
`authored_sql:` for a fragment somebody wrote - so which metrics are governed by a closed set and
which are text is a word in the document rather than a reading of it.
[A named escape hatch for authored SQL](adr/0004-a-named-escape-hatch-for-authored-sql.md) is the
record: what the hatch is, what stays closed, which constructs are refused at load and why each one
is on the list.

**Partly built.** The domain types and the compile - parse, refuse, qualify against the model's
columns, render per dialect, all at catalog-compile time - exist in `sutura_domain::expression` and
`sutura_sql::expression`. What does not exist yet is the wiring: no catalog document can write
`authored_sql:` and no plan can carry a compiled one, so no metric uses the hatch today.

**A pinned statement**, rendered upstream and taken as given, spliced into a generated wrapper. Not
built. The rest of this section is its design.

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

It remains implementable exactly as described: the dialect layer has a verbatim passthrough node
whose generator appends the text unchanged, which no dialect pass rewrites and which has no children
for a transform to descend into. What is missing is a reason to build it, which arrives with the first
upstream renderer.

### What holds the generated statement up

Goldens per dialect, regenerated and reviewed as a diff rather than typed, over a corpus of questions
that includes one per refusal. Beside them, four checks that are assertions rather than snapshots:

- **No literal from a question appears in the statement.** Every value is a bind parameter, and the
  parameter list is asserted to be exactly the set of literals the question carried - so a generator
  that dropped the predicate fails it too.
- **Every identifier and alias is quoted**, so a column called `order` is not a syntax error.
- **Every statement parses under the dialect it was generated for**, which catches a malformed
  statement without needing one of each data system in CI. Parse only, never re-emit: that is what
  makes it safe. **It is not the same as "the data system accepts it", and the difference matters.**
  The dialect layer's parser takes a dialect but is not gated on it for every construct, and its
  generator writes `x IS TRUE` without ever consulting its own flag for whether a dialect allows
  that - so a construct like that parses under all three targets whatever a real instance would say.
  Acceptance is vouched for by execution instead: by the anchors, and by one plan run both ways over
  a real DuckDB and the real engine, rows compared. For Postgres and ClickHouse we render and
  parse-check, and nothing more.
- **Every declared anchor re-executes and reproduces its number**, and a bundle whose anchors were not
  all checked cannot be served, because there is no constructor that produces one.

The gap this page used to record - that no lint banned a transpile call on the query path - is closed
differently and better: the dialect layer's `transpile` feature is not compiled, so a call to it does
not build.

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

Three of those nodes are design targets rather than descriptions of this repository. Federation is
**built, and a published build answers a two-source question end to end** - the splitter, two
executions and the combiner, with the engine declaring `Warehouse::EXECUTES_LEGS` and a differential
over both. That constant still defaults to `false`, which is what refuses an adapter with no leg
venue rather than half-answering. Execution
*as the calling principal* is not built, and no longer because nothing carries a principal: a
verified subject reaches the request path, and what is missing is an adapter with anywhere for a
per-subject credential to arrive. The spliced statement is not built, because a metric has no
statement field; and the Arrow envelope is not built, because the port returns a row type. What runs
today is the question, the semantic layer, the plan, the dialect, and execution against a local file
as whoever started the process.

**The semantic layer decides what a question means.** [Wren](https://github.com/Canner/WrenAI) is
the reference for that shape: a modelling language, an engine that plans against it, and MCP as the
way an agent asks. Wren derives a metric's SQL from a model of the tables; we take the statement as
given and refuse to look inside it, which is a narrower job and a different guarantee. Wren also
carries access rules in the model, which is the middle-tier policy copy
[this design declines to keep](#security-is-the-reason-for-the-shape).

**The plan is where a question stops being text.** [DataFusion](https://datafusion.apache.org/) is
what Wren and Spice both build on: a logical plan representation, an optimizer you extend with
rules, and execution over Arrow. The plan is still our own type, and it now lives in
`sutura-domain`, which names no framework - not tokio, not arrow, not datafusion - so it cannot be
DataFusion's. What earned DataFusion its place is the execution stage rather than this one, and not
the SQL frontend either way: over a local file it executes a plan and emits no SQL, which is a whole
class of dialect bug that cannot occur there.
[DataFusion for local execution](adr/0003-datafusion-for-local-execution.md) is the record.

**Push-down already happens, and completely - federation is not what buys it.** Worth stating plainly,
because "federation pushes the query down" invites the assumption that without it we pull rows up and
filter locally. We do not. A plan is rendered as one statement carrying the join, the bounded
predicate, the grouping, the ordering and the row cap, and the data system returns the finished
aggregate. Nothing comes back that was not asked for, and there is nothing left here to re-authorize -
which is the security property, not a performance one: the predicate is evaluated under the caller's
own grants, row-level policies and column masking, by the system that owns them.

**What federation adds is a SECOND source, and that is a security question rather than a capability
one.** [datafusion-federation](https://github.com/datafusion-contrib/datafusion-federation) registers
an optimizer rule that finds the largest subplan a single remote source can execute, hands it there,
and combines the results. That is exactly what a plan spanning two data systems needs - and a plan
spanning two data systems is refused today
([one data system per plan](#the-engine-and-the-data-systems-behind-a-port)) because it is a second
identity to satisfy, not because we cannot compute it. Pull the rows up and join them here and the
filtering becomes ours, which is the second policy implementation this design declines to keep. So
federation arrives after a credential exists per leg. The blocker is identity, not the optimizer.

Two practical notes for when it does arrive. Its push-down renders the remote SQL with DataFusion's
own unparser, which is the path
[the splice section](#what-holds-the-generated-statement-up) deliberately avoids - adopting it means
either accepting that renderer for the pushed-down half or supplying one that uses `polyglot-sql`.
And there is real version skew between the crate and current DataFusion, which is a second and much
smaller reason it is later rather than now.

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
| Semantic layer | what a metric means | Both, by two routes. A first-party model is authored here and compiled; a rendered statement is authored upstream and taken as given. Wren is the reference shape for the modelling half, and the difference is that a model here may hold no SQL expression |
| Plan | source, projection, grouping, bounds, parameters | Ours, and the prediction this row used to make came true from the other side. The type is still ours and it moved into `sutura-domain`, because the execution port carries a plan rather than a statement; DataFusion arrived for execution rather than for representation. [DataFusion for local execution](adr/0003-datafusion-for-local-execution.md) |
| Federation | which subplan its owner runs | Adopt for a SECOND source, once a credential exists per leg. Not needed for the first: a single-source plan is already pushed down whole |
| Dialect | quoting, placeholders, date arithmetic | Adopt for what we generate, never for the splice. Two things it does not decide: placeholder style, which it renders identically for every target, and quoting, which it applies only when asked. Both are ours |
| Execution | the connection, and which principal the data system sees | Build, and adopt for the local leg: one adapter per data system, DataFusion where the data is a file on the same machine, and the per-request credential is the part nothing above provides |

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
    catalogue         DuckDB,
                      DataFusion
```

`cargo xtask check-boundaries` enforces the direction, so the diagram cannot quietly stop being true.
It fails if `sutura-domain` acquires a framework dependency anywhere in its transitive tree, which
catches a framework reached through an innocuous crate as well as one declared outright. It fails if
a named crate can reach a named crate it may not: today that is `sutura-semantic` reaching the SQL
generator, directly or through `sutura-sql`, and each entry prints the reason and the fix rather than
only saying no. It fails again if a library crate's surface stops being a typed contract: a `pub`
field on a `pub struct`, a `Result` whose error type is `String`, or a declared dynamic-error crate
such as `anyhow`.

One thing the check cannot read is which crate a trait is *declared* in, and that gap has cost
something once: the `Surface` driving port was declared inside `sutura-http`, which would have made a
second transport depend on the first. It is `sutura-app`'s now. What the check does still hold up is
the half that matters most about the move - `sutura-app`'s dependency tree is `sutura-domain`,
`sutura-semantic` and `thiserror`, so hosting a transport's interface could not have brought a
framework with it.

Two consequences follow from the direction rather than from taste. The domain names no framework, so
its test suite compiles nothing heavy and runs in well under a second, which is what makes it the
inner loop. And every lint and test entry point passes `--all-features`, so an adapter placed behind
a feature is inspected from the day it lands rather than from the day somebody remembers the flag.
That habit was adopted while no crate declared a `[features]` table at all, which is when it is
cheapest to adopt; it is load-bearing now, because `sutura-config`, `sutura-http` and `sutura-serve`
each declare `tls` - see [Serving over HTTP](serving.md#tls) - and code behind a flag nothing passed
would be linted and tested by nothing. [Contributing](contributing.md) has the commands.

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

The query path is built: one shape of catalog, an engine that executes it, and one data system it
knows how to push down to but does not ship.
[What can be plugged in today](#what-can-be-plugged-in-today-and-what-the-shipped-binary-actually-uses)
is the table, and it is the section to read before assuming which of the three is on the runtime path.

`sutura-domain` holds the domain types, the query plan and two port traits, `SemanticCatalog` and
`Warehouse`; `sutura-catalog-local` reads a directory of markdown documents with YAML frontmatter;
`sutura-semantic` resolves and plans, and renders nothing; `sutura-sql` renders a plan into one
dialect's SQL for whoever asks, and is the only crate that names the dialect layer;
`sutura-exec-datafusion` is the engine, executing a plan over Arrow and rendering no SQL at all;
`sutura-exec-duckdb` is a data source, rendering the plan into DuckDB SQL through `sutura-sql` and
pushing it down; `sutura-app` is the service, generic over both ports, and holds the `Surface`
driving port a transport consumes; `sutura-cli` composes them, and links the engine only. `xtask`
holds the repo gates and `sutura-dev` the local development CLI.

What that adds up to: a question naming a metric, a grain, a bounded range, up to four dimensions and
a filter compiles to a plan; the plan renders as one statement in `DuckDB`, Postgres or `ClickHouse`
dialect when somebody asks for SQL, and it **executes through the engine, over the CSV or Parquet
files in the directory the caller named**. A measure is a single term or a ratio of two, where a term
is an aggregate over a column or a conditional count - so a conditional count can be either a whole
measure or half of a ratio - and a metric may carry required filters that every question about
it is answered under. Every metric that declares a certified number re-executes and reproduces it
before the bundle can be served, and a bundle whose anchors were not checked cannot reach the query
path because there is no constructor that produces one.

**`CredentialBroker` is here now, and what it changed is narrower than the name suggests.** The port
mints once per answer for every source a plan reads, `Warehouse::execute` takes the result and has no
signature that omits it, and a subject with no credential at a source is refused rather than answered
under this process's identity. So the *fallback* is gone: not forbidden by a rule, absent from every
signature.

**What has not arrived is a data system with grants to run under.** A CSV is a file with no login, so
"every query runs as the calling principal" is still satisfied here by there being nobody else to be -
and both adapters in this build declare that they have nowhere for a subject's own credential to
arrive. The broker that ships mints from configuration and performs no token exchange. That is a true
statement about a laptop and not about a warehouse, so this remains a compiler with a governed front
door: what the port bought is that the day a real source arrives, there is no code path for it to be
read as the process through.

**The HTTP transport is here now**, and this sentence used to say it was not: an axum surface with a
versioned `v1` tree, a liveness probe, a generated interface description, rate limiting, a bearer
gate and optional in-process TLS. What it does **not** carry is a per-caller identity - the token
authenticates the deployment - so none of the identity claims above are made true by its arrival.

Still absent: Arrow results with provenance in the schema metadata, and a per-caller budget beyond
the row cap and the ten-year span. The spliced-statement path is designed, documented above, and
unimplemented. **This bullet used to list the MCP transport, federation, a second catalog adapter
and the audit sink**, all four of which arrived - `sutura-mcp`, the splitter and combiner,
`sutura-catalog-datahub` and `sutura_runtime::audit` - while the same page said so twenty lines
above its own diagram. **A fifth, *execution of a federated leg by a shipped adapter*, was written
into this bullet on this branch and spent before it merged**: the engine declares
`Warehouse::EXECUTES_LEGS` now. The list is the shape this page goes stale in, which is why the
history is kept beside it.

And one thing that was absent here and is now half present, because the two ports are what the layout
is *for*: **runtime selection of a data system.** `sutura-serve` reads a `sources:` tree, opens one
adapter per source the catalog names, and hands each the posture its entry declared - so a `SourceName`
now *selects* a warehouse out of a registry rather than being compared for equality against the one
adapter that was linked, and a source with no entry is a startup refusal naming it. `sutura-cli` reads
the same tree now: a declared source is opened under its own name and its own posture, an undeclared
one falls back to that binary's own built-in `files` declaration, and a declared kind it linked no
adapter for is refused by name. What it still will not do is answer a question spanning two data
systems - it answers one question against one, and federation is the HTTP surface's.

**Half, and the honest half is the one that is missing:** which *kind* of data system a source may be
is still decided at compile time, because `files` is the only kind an adapter ships for. So a
deployment chooses how many sources it has, where each one is and which identity reaches it, and cannot
choose to point one at a database. `feat/bigquery-adapter` is the step that changes that, and it also
decides the shape a heterogeneous set needs - `Warehouses<W>` is generic in one adapter type today.
Which *catalogue* is read is still a compile-time decision with no configuration at all.

The mechanisms came first on purpose, and that has not changed: every claim on this page is meant to
be held up by a type, a lint, a hook or a gate rather than by intent, and a mechanism is cheaper to
build before there is code to retrofit it onto.
