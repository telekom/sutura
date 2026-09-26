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

    Design targets and built mechanisms appear together here. The compiler, file engine, HTTP and
    MCP surfaces, request context, credential broker and audit sink are built. A deployment can use
    a shared token or declare `security.inbound` to verify a caller. The shipped binary links an
    adapter that can carry a per-subject credential -
    `sutura-exec-bigquery`, enabled in the shipped binary - but its served source-acceptance
    venue has no observed run (`docs/where-identity-is-proven.md`). Other sources use declared
    shared identities. No Arrow result envelope. Sections that describe something enforced
    today say so inside the section, and [What exists today](#what-exists-today) is the inventory.
    **Do not deep-link a section of this page as evidence that a control is in place.**

## Serving is MCP

**Both transports are built, and one property this section describes is not.** `sutura-http` serves
a versioned `v1` tree, a liveness probe, direct-mode protected-resource metadata, a generated
interface description, rate limiting and a bearer gate; `sutura-mcp` serves the tool surface over a
process's own standard input and output, and `just mcp-e2e` drives that one end to end. What is absent
is a caller identity on the agent surface: a pipe has no header a token could arrive in, so it answers
as the deployment and offers every capability, and a network-reachable agent surface needs the identity leg
`docs/adr/0014-how-a-caller-proves-who-it-is.md` designs.

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
compile. It names which of eleven definition kinds - structure, descriptions, relationships, a
join's cardinality, metrics, definitional filters, grains, value allowlists, anchors, a column's
type, a column's description - and which of the four knowledge kinds this source can carry at all. An empty collection is two different facts: a reviewed
catalogue that has not certified a metric yet, and a source that holds a measure this runtime will
not execute. `sutura-catalog-local` declares every kind, a statement about the format rather than
about the directory it read; a source that supplies part of the model declares the part, and its
conformance test is that what it declared is exactly what it produced.
`docs/adr/0016-what-datahub-can-carry.md` is the measurement this came from.

**Lineage does not arrive through that port.** There is no
lineage type anywhere in the workspace and none is planned: a plan reads at most two data systems, a
measure reads columns a model declares, and where a column came from upstream changes neither. It is
real metadata that real catalogues carry - `docs/adr/0016-what-datahub-can-carry.md`
reads three lineage aspects out of one of them - and this port has no shape to put it in.

The catalogue may be outside this repository or in it, and
`docs/adr/0001-first-party-semantic-models.md` is why both are allowed.
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
because it emits no dialect. `docs/adr/0003-datafusion-for-local-execution.md`
is the record.

### Where the engine sits today

DataFusion is an adapter behind `Warehouse` for local files and can execute a leg of a federated
answer. The splitter and combiner are built; each selected adapter declares whether it can execute
a leg, and an unsupported leg is refused. A federated answer may mix source identity postures and
reports each one. That report is not a uniform authorization guarantee. [How a federated join key is compared](how-a-join-key-is-compared.md) records what the combiner refuses before any row is read,
and what it does not cover.

### Connectors: Arrow Flight, not a driver per data system

That heading records an earlier direction; the implemented boundary is the plan passed to
`Warehouse`, leaving each adapter to choose its own transport and rendering. BigQuery uses ADBC;
Postgres and ClickHouse use their own drivers. The domain returns
`ResultBatches` (Arrow) rather than a `RowSet`, decoded to rows once at the
presentation edge. DuckDB is a development dependency used to prove
rendered SQL, while the shipped BigQuery and Postgres adapters are runtime dependencies.

### What can be plugged in today, and what the shipped binary actually uses

The composition root chooses adapters at build time, then `sources.<alias>.kind` selects among
those linked by the binary. `nix/shipped.nix` is the release feature list: it enables `bigquery`,
`postgres`, `datahub`, `agent` and `tls` for the shipped binary. A local Cargo build without those
features has a narrower set; `sutura doctor` reports the adapters it links. ClickHouse and Oracle
remain default-off and absent from the shipped binary. [Integrations](integrations.md) records each
adapter's capability and identity posture.

`sutura query` can answer a local file question or open a configured source whose adapter is linked.
It does not establish a remote caller's identity: its request context is the command-line caller's
local context. `sutura serve` accepts HTTP and MCP calls, can verify a caller's token, and composes
sources according to each entry's declared posture. The engine can execute a leg of a federated
question; an adapter without leg support refuses one. A mixed answer reports the posture of each
leg, which is disclosure rather than a uniform authorization guarantee.

BigQuery uses the ADBC driver, not the former HTTP `wire` transport. Its adapter declares
`PerSubjectCredential`: the served path can carry the verified caller's assertion through the
source's declared per-subject account map to the driver. An undeclared subject is refused. This is
built, but the real served identity venue is `wired` with no observed run, so source acceptance of
that identity is not proven. The real-dataset corpus exercises statement acceptance under its own
credential; it does not prove the served per-subject hop. [Where identity is proven](where-identity-is-proven.md)
keeps those venues separate. Postgres, DataFusion and the other shared adapters execute under a
source identity declared by the deployment.

DuckDB remains a test dependency rather than a shipped runtime adapter. Rendering a dialect is
also separate from executing it: `sutura compile` can produce SQL for a system whose adapter the
binary cannot open. The real-server and conformance limits for each adapter belong in
[Integrations](integrations.md), not in a claim about every shipped source.

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
That is true of `sutura serve` too and always was; #121 is the change that makes it the documented
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

The identity mechanism below is built for BigQuery and unproven at its real served venue. The
other properties name their own mechanisms and limits.

**The caller's identity can reach BigQuery; source acceptance is unproven.** A credential is minted
per request. An undeclared BigQuery subject is refused rather than downgraded to the deployment's
identity; shared-identity sources intentionally execute with their declared credential.

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

**What bounds it is a lint, and its input type is a self-check rather than a barrier.** The method
can be handed a fabricated `AnchorPlan`, not just a caller's own question: the constructor is public,
every value it reads is publicly constructible, and Rust has no cross-crate friend visibility, so no
arrangement of guards there can be an authority. Two mechanisms now, stated apart:

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

*Built, behind a feature - not yet proven at a served binary.* `sutura-exec-bigquery` is a published
build's own adapter (its default-off `bigquery` feature ships in `nix/shipped.nix`) and its
`IMPERSONATION` is `PerSubjectCredential`, so it is no longer true that no published build has one.
Every OTHER adapter a published binary links - `sutura-exec-datafusion` for `files`,
`sutura-exec-postgres` - still declares `NoPlaceForASubject`, and the broker that ships mints from
configuration for those: a `files` or `postgres` leg's identity is *the one this deployment holds
for that source*, acknowledged by an operator and recorded on the answer - which is honest and is
not impersonation. Against a local file the property is trivially satisfied and buys nothing, since
a file has no login. `bigquery`'s own mechanism resolves per source
(`docs/where-identity-is-proven.md`), but no served binary has executed a leg as the calling
subject yet - so what the port buys for every OTHER source stands unchanged: the day one arrives
with grants, there is no path for it to be read as this process through.

What is enforced beside that: `SemanticCatalog::load` takes no request context, so a catalog cannot
choose an identity. A federated plan can contain more than one source, each with its own credential
and reported posture.

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

*Enforced today:* the `Warehouse` port returns `ResultBatches` (Arrow) rather than a text blob,
and provenance rides beside the rows as its own typed field rather than as text mixed into them.
The shape of the guarantee is right: the envelope is Arrow, decoded to typed `Value` cells once
at the presentation edge (`ResultBatches::to_rows`). The engine executing over Arrow *in
process* is a different claim from results leaving as Arrow, and only the first is true.

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
`docs/adr/0001-first-party-semantic-models.md` is the record: it exists
because the spliced path below has a precondition - something upstream must already have rendered
dialect-correct SQL - and on a laptop, or over a single file, there is no upstream to have done it.

Its load-bearing constraint is that **a model may not contain a free-text SQL expression.** A measure
is one of two closed shapes over a `Term` of two terms - a term is one aggregate over a named column
or a conditional count, and a measure is either a single term or a ratio of two of them, which is what
lets a conditional count be half of a ratio rather than only a whole measure
(`docs/adr/0002-a-closed-vocabulary-for-measures.md` records why that was factored one level
lower than it first was); a relationship is a join type and either one column pair or an ordered
list of join keys - the single pair stays the shape every existing document writes, and a compound
join declares a `keys:` list instead, one column pair never both; a
dimension is a column, optionally one declared relationship away. A metric may also carry required
filters, predicates from a closed set of four operators that are **part of what the metric means
rather than something a caller asks for**: they are applied to every question about it, and a caller
cannot see, choose or remove one.
`docs/adr/0002-a-closed-vocabulary-for-measures.md` is the record of why
the vocabulary is a closed set of *shapes* rather than a single aggregate. The property being defended
was always no free-text SQL, and one aggregate over one column was a narrow means to it that could
express two of a real semantic layer's seven metrics.

The cost is unchanged for the closed vocabulary: an expression over two columns cannot be said in a
`Measure`, and neither can a window function.

**There is now a second, explicitly-named way to say them, and it is a separate shape rather than a
widening of the one above.** `Computation` has two variants - `measure:` for the closed vocabulary and
`authored_sql:` for a fragment somebody wrote - so which metrics are governed by a closed set and
which are text is a word in the document rather than a reading of it.
`docs/adr/0004-a-named-escape-hatch-for-authored-sql.md` is the
record: what the hatch is, what stays closed, which constructs are refused at load and why each one
is on the list.

**Loaded and refused at boot; not compiled, not executed.** A local metric document writes
`authored_sql:` beside nothing else, the fragment is admitted as text - present, bounded, one
fragment rather than a script, no control or invisible characters - and pinned under the definition
digest exactly as written. Nothing published compiles it: the compile in `sutura_sql::expression`
has no production caller, and a catalog adapter may not reach that crate
(`cargo xtask check-boundaries`), because the shipped binary links the local catalog and a SQL
generator in its tree is one in the network binary's. Every adapter this workspace ships leaves
`Warehouse::EXECUTES_AUTHORED_SQL` at `false`, so `verify_and_validate` refuses a bundle carrying an
authored metric before serving, naming it. The compile belongs to the first adapter that executes
the fragment. Until then an authored fragment is stored, not checked.

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
  a real DuckDB and the real engine, rows compared - and for Postgres and ClickHouse the same, against
  the server each one's nix tier starts. For Oracle we render and parse-check, and nothing more.
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

The question, semantic layer, plan, dialects and execution are built. Federation can answer a
two-source question when both selected adapters execute legs. BigQuery has a per-subject credential
path, but its served source-acceptance venue has no observed run. The spliced statement and Arrow
result envelope remain design targets.

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
`docs/adr/0003-datafusion-for-local-execution.md` is the record.

**Push-down applies to a source's executable share of a plan.** The adapter renders its share for
the system that owns the data, where that system applies the grants of the credential presented for
the leg. On BigQuery that credential can come from the declared per-subject map; on other sources it
is shared. The served BigQuery identity hop remains unproven at a real source.

**Federation adds a second source and a second identity decision.** The built splitter and combiner
can answer a question spanning two sources when both adapters execute legs. Each leg obtains a
credential for its own source. A mixed-posture result names each leg's execution posture; that
disclosure does not make the combined rows subject to one uniform grant.

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

| Stage          | Decided there                                            | Ours or theirs                                                                                                                                                                                                                                                                                                                |
| -------------- | -------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Semantic layer | what a metric means                                      | Both, by two routes. A first-party model is authored here and compiled; a rendered statement is authored upstream and taken as given. Wren is the reference shape for the modelling half, and the difference is that a model here may hold no SQL expression                                                                  |
| Plan           | source, projection, grouping, bounds, parameters         | Ours, and the prediction this row used to make came true from the other side. The type is still ours and it moved into `sutura-domain`, because the execution port carries a plan rather than a statement; DataFusion arrived for execution rather than for representation. `docs/adr/0003-datafusion-for-local-execution.md` |
| Federation     | which subplan its owner runs                             | Built for adapters that execute legs; each leg obtains a source credential, and a mixed-posture answer reports both                                                                                                                                                                                                           |
| Dialect        | quoting, placeholders, date arithmetic                   | Adopt for what we generate, never for the splice. Two things it does not decide: placeholder style, which it renders identically for every target, and quoting, which it applies only when asked. Both are ours                                                                                                               |
| Execution      | the connection, and which principal the data system sees | Build, and adopt for the local leg: one adapter per data system, DataFusion where the data is a file on the same machine, and the per-request credential is the part nothing above provides                                                                                                                                   |

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
cheapest to adopt; it is load-bearing now, because `sutura-config`, `sutura-http` and `sutura-cli`
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

| Build          | 48 threads |
| -------------- | ---------- |
| glibc          | 4.45s      |
| musl, mallocng | 92.16s     |
| musl, mimalloc | 3.83s      |

92.16s is slower than musl's own single-core run, which is what one global mutex predicts. Expect
parity single-threaded and a 4-20x gap for a threaded application.

mimalloc is built with `MI_SECURE=4`: guard pages, randomised placement, encoded free lists,
double-free detection. That costs 23-43% against plain mimalloc, far more than upstream's README
claims. mimalloc-secure still lands ahead of glibc and well ahead of mallocng, and the latency
budget here is a warehouse round trip.

## What exists today

The shipped binary serves HTTP and MCP, compiles governed questions, executes local files through
DataFusion, and links the BigQuery and Postgres data-source adapters. It can load local and DataHub
metadata. `nix/shipped.nix` holds the exact release feature set; [Integrations](integrations.md)
records what each adapter executes and what identity posture it declares.

`security.inbound` can verify a caller, and every executed question obtains a source credential. The
BigQuery path can use that verified caller's assertion through a declared per-subject account map.
Its real served identity venue has no observed run, so the source's acceptance of that identity is
unproven. Shared-identity sources use the credential declared for the source. The HTTP and MCP
surfaces report the posture of each leg of a federated answer; the record does not itself authorize
access. [Where identity is proven](where-identity-is-proven.md) states which tests can support each
claim.

A deployment selects linked adapters through its `sources:` registry. `sutura query` has a local
caller context and can open a configured source; `sutura serve` establishes callers through its
inbound mode and can answer a federated question where each selected adapter supports leg execution.
An unsupported source kind or leg is refused. Arrow results with provenance in schema metadata and
the spliced-statement path described above remain design targets.
