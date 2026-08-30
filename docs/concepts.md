---
title: Concepts
description: The words on the tool surface, and what each one commits to.
---

# Concepts

The words on the tool surface, and what each one commits to. The design is settled and the code is
not, so this page is vocabulary rather than an API you can call.
[Architecture](architecture.md) says how the pieces fit.

!!! warning "Two kinds of claim on this page, marked at each one"

    **Enforced today** means a type, a lint, a gate or a test holds the property in the code that
    is here now, and the mechanism is named beside the claim. **Design target** means it is written
    down and not built: no port, no adapter, no test, and nothing that would notice if the property
    were false.

    **The identity claims are the ones to read carefully, and they are no longer all design targets.**
    A deployment can now establish who is asking, from a signature, and no question can execute
    without a credential a broker minted for the source it reads. What is still absent is a data
    system that evaluates the asking subject: no adapter in this build can carry a per-subject
    credential, so every question reads as one identity - and a deployment behind only the bearer
    token has no per-caller identity at all, because that token authenticates the **deployment**.
    So the perimeter is real, the subject is known where leg 1 is configured, and per-caller ACCESS
    is not here. No sentence should be read as a control you can rely on unless it says *enforced
    today*.

## A question, and what it is made of

A question names a **metric**, some **dimensions**, a **grain** and a bounded **time range**.
That is the whole vocabulary. There is no field for SQL, a table, a filter expression or a list
of row ids, so a question outside this shape is unrepresentable rather than refused.

**Metric.** A named measure somebody certified: revenue, active subscribers, churn. The name
comes from a semantic layer outside this repository, and it arrives with what the metric means.

**Dimension.** An attribute a metric declares it can be broken down by: region, product, tariff.
A dimension the metric does not declare is not a narrower question, it is a name that does not
resolve, and it comes back as a refusal naming the argument that failed. Dimension *values* are
arguments checked against an allowlist in the pinned bundle, never text pasted into a query.

**Grain.** The time resolution the answer is aggregated to, and one the metric supports. Daily
revenue and monthly revenue are the same metric at two grains, not two metrics.

**Time range.** Bounded, always, and bounded in two senses. It must have both ends - an unbounded
range is a table scan with a plausible name - and it must be short enough to be worth answering,
because two real dates ten thousand years apart are a full scan that parses cleanly.

**Enforced today.** `Query` carries no field for SQL, a table, a predicate or a row id, so an
uncertified question is unrepresentable rather than refused - and `deny_unknown_fields` makes a
question carrying `sql:` an error naming the field rather than one silently dropped. `TimeRange`
has no unbounded form, so an unbounded range fails to deserialize at all. Both are asserted by
tests in `sutura-catalog-local`, which has a real format parser to provoke them with. The span cap
is separate and sits one layer in, at resolution, where the range is demonstrably a caller's rather
than an author's - the table under [What you cannot ask for](#what-you-cannot-ask-for)
says what it does and does not bound.

**Telling an agent all of this is a separate job from enforcing it.** An agent that has not been told
what this surface is will look for a field to put SQL in, and then read a refusal as an outage and
retry. The types stop the damage and cannot stop the loop, so the vocabulary above is also rendered
as a system prompt from the pinned bundle and the exposed operations -
[The agent prompt](agent-prompt.md) is what it says, what it deliberately leaves out, and what an
operator can layer on top.

## A certified definition

A **definition** is what a metric means: the measure it computes, the model it reads, the grains
it supports, the dimensions it may be broken down by, and the filters that are part of its meaning.

**Enforced today.** That meaning is a closed vocabulary rather than an expression language, so
there is no field a catalogue could write an arbitrary expression into. `Measure` is an enum of two
shapes over a `Term` enum of two terms, `RequiredFilter` an enum of four operators, and
`deny_unknown_fields` applies at every depth. sutura generates the whole statement from that, in
`sutura-semantic`, so neither caller-authored nor catalogue-authored SQL is on the path at all.

**Partly built, and named so it cannot be quiet.** A metric may instead carry `authored_sql:` - a SQL
expression a catalogue author wrote, for what the closed vocabulary cannot say: a window function, a
percentile, an expression over two columns. It is a *sibling* of `measure:` rather than a field on it,
exactly one of the two may be present, and a fragment is parsed at load, checked against a list of
refused constructs, checked against the model's declared columns and rendered for every dialect
before anything serves. A caller still has no field for SQL, and the agent prompt still never sees
any. [A named escape hatch for authored SQL](adr/0004-a-named-escape-hatch-for-authored-sql.md) is
the record. The types and the compile exist; no catalogue document can write the key yet.

**Design target, not built.** A definition may instead arrive as **statement** text authored
upstream, spliced into a generated wrapper byte for byte, because re-emitting it would substitute
our reading for the author's and the number would change quietly. Nothing implements that splice:
`Metric` has no statement field today.
[What exists today](architecture.md#what-exists-today) records the gap.

An **anchor** is a known result for a metric. **Enforced today:** `sutura_app::verify_and_validate`
re-executes every metric that declares one, and returns the bundle as `Validated` only if every
anchor it declares was checked and matched. A bundle whose anchors were never checked cannot reach
the query path, because that function is the only thing anywhere that produces a `Validated`, and it
takes the data system as an argument - so the type cannot be obtained without one having been asked.
A `compile_fail` doctest holds it, because a review found the earlier version forgeable.

## The pinned snapshot

Definitions do not arrive live. They arrive as a **pinned snapshot**: a bundle of definitions
with a version and a **digest** over the text. Two consequences follow:

- A catalogue edit cannot change what a question means between two invocations. It changes the
  digest, and the digest travels with the answer.
- The catalogue cannot see who is asking. The load path takes no request context, so it cannot
  return one definition to one caller and a different one to another.

**Enforced today.** `SemanticCatalog::load` takes no argument at all, so there is no request
context to pass it and none to leave out; dimension validation reads the pinned bundle rather than a
live view. The digest is taken over the canonical form of the parsed definitions, so reformatting a
document does not move it and changing what a metric means does - asserted by a golden either way.

A result carries the version and digest of the definitions that produced it, so a number traces
back to the text that defined it. That is what makes "certified" checkable rather than asserted.

## Refusal

A **refusal** is an answer, not an error. It is a variant of the result type, with a reason, so
a caller cannot mistake it for a transport hiccup and retry until something works.

Reasons are typed rather than prose, because the variant is the contract and the message is not.
A dimension that does not resolve, a value outside the allowlist, a plan that would need two data
systems, a data system this process did not open: each is its own `RefusalReason` variant.

**Enforced today.** A refusal is `ToolOutcome::Refusal { reason }`, a variant of the result rather
than an `Err`, and the golden suite provokes every variant a question can reach. A rejected value is
never echoed back either: `DimensionValueNotAllowed` names the dimension and stops there, so caller
text cannot be reflected into a log, a UI or an agent's context.

**Built, and the limit is the deployment's rather than ours.** Every outcome - a refusal as much as an
answer - is written to an `AuditSink` before it is returned, and the record carries the principal
chain. `sutura-runtime` ships the structured writer a deployment that attaches nothing else gets. Two
things that are not the same as attribution: sutura **retains nothing**, so what a record is worth is
what the deployment's sink is worth; and the subject in that chain is only as strong as what
established it - a deployment behind the shared bearer token alone records the *deployment*, because
that is who asked as far as anything can tell. A deployment that declares `security.inbound` records
the caller, from a signature.

**Still a design target.** A refusal recorded against a subject whose *access* decided the answer.
The record can now say who asked and which identity each leg ran under; it cannot say the two were
the same, because no adapter in this build can carry a per-subject credential.

## Principal, subject, and running as the caller

The **principal** is who is asking. When an agent asks on somebody's behalf there is a chain of
them, and the **subject** is the identity the data system must see: the person, not the service.

**Half built, and the halves are worth telling apart.** The target is that every query runs as the
subject: a credential is minted per request, and a leg that cannot run as the subject is refused
rather than falling back to the service's own identity - because that fallback turns "you may not see
these rows" into "here are the rows".

*Built:* answering takes a request context and a credential broker, the broker mints once for every
source the plan reads, and the execution port takes what it produced with **no signature that omits
it**. A subject with no credential at a source is refused as `credential_unavailable`, and each
adapter refuses credential material it has nowhere to put rather than quietly ignoring it. So the
fallback is not forbidden by a rule - it is absent from every signature.

*Not built:* an adapter that can carry a per-subject credential. Both in this build declare that they
have nowhere for one to arrive, and the broker that ships mints from configuration and performs no
token exchange. So what a credential says today is *the deployment's own identity for this source,
acknowledged by an operator* - which is honest, and is not impersonation.

**Enforced today, and narrower than it sounds.** Nothing in the query path can *choose* an identity.
`SemanticCatalog::load` takes no request context, so a catalogue cannot return one definition to one
caller and another to the next; a plan resolves to exactly one named data system; and `Secret`
implements no `PartialEq` and has a hand-written `Debug`, so credential material cannot be compared
or printed by accident. In single-player that is the whole of it: the data is a file, a file has no
login, and there is only ever one subject. It is a true statement about a laptop and not about a
warehouse.

**There is no downgrade path because there is no second identity to downgrade to** - not because
something forbids one. `RefusalReason::SourceUnavailable` does exist, but `sutura-app` raises it
when a plan names a data system this process did not open, which is a source-name mismatch and not
an identity failure. Nothing here would catch a future adapter that fell back to a service account.

**sutura holds no copy of who may see what.** Grants, row-level policies and masking live in the
data system, administered and audited by the people who own the data. A second copy here could
disagree with the original, with no way to tell which one is right.

## Catalogue and data system

Two ports, deliberately separate.

A **catalogue** supplies definitions: metrics, dimensions, the glossary, lineage. A directory of
documents in git and a metadata catalogue with an HTTP API are two adapters behind one trait. The
first exists; the second is a design target.

A **data system** executes. ClickHouse and Postgres are the near-term targets. The port is named
`Warehouse`, which says nothing about what sits behind it. Two adapters exist and they are different
kinds of thing: `sutura-exec-datafusion` is **the engine** - it reads the CSV and Parquet files
itself, executes the plan over Arrow and generates no SQL, and it is what the shipped binary links;
`sutura-exec-duckdb` is a **data source** - it renders the plan into `DuckDB` SQL and pushes the
statement down, and it is a development dependency, there to prove the rendered SQL runs somewhere.
Postgres and ClickHouse are rendered for and parse-checked without either being connected to.

A plan resolves to exactly **one** data system. Spanning two is not a bigger version of the same
problem, it is a second identity to satisfy, and a plan whose legs cannot all run as one subject
is refused rather than run partly as somebody else.

**Enforced today.** The plan stage collects the source of the metric's own model and of every model
reached through a join into a set, and refuses `PlanSpansTwoSources` unless exactly one name is in
it. The count is computed from the plan rather than asserted about it afterwards, and a golden
builds a two-source catalogue to provoke the refusal. **The identity half of that reasoning is a
design target:** there is no per-leg credential, so nothing tests that two subjects get different
rows, and nothing can until one exists.

## Provenance, and why results are meant to be Arrow

Rows, column descriptions and glossary text are authored by somebody else, and any of it can contain
something shaped like an instruction. A delimiter cannot separate instruction from data, because the
content can contain the delimiter.

**Design target, not built.** Results leave as Arrow with **provenance** in the schema metadata: a
typed field a caller reads deliberately, never a string concatenated into the channel that carries
instructions, with both wire envelopes sharing one encoder so neither can grow a text-blob shortcut
on its own. None of that is here. There is no Arrow IPC or Flight envelope, no encoder, and no
transport to carry one. The engine does execute over Arrow *inside* the process, which is a
different claim: nothing leaves as Arrow.

**Enforced today.** The `Warehouse` port returns a `RowSet` - typed columns and typed `Value` cells
with an enforced row width, not a text blob - and an answer carries `Provenance` beside it as its own
field rather than as text mixed into the rows. The definition version and digest travel there, so a
result is still not separable from what defined it. The envelope is a row type rather than an Arrow
schema, and `RowSet` is honest about being one.

## What you cannot ask for

Absences by design. The last column says what holds each one, because an absence written down and
an absence enforced are not the same thing:

| Not available | Why | What holds it |
| --- | --- | --- |
| SQL, a table name, a filter expression, row ids | The tool surface has no field for any of them. An uncertified question is unrepresentable, not merely refused | **Enforced.** `Query` declares no such field, and `deny_unknown_fields` makes an attempt an error naming it |
| A cached result | Under row-level security a query-keyed cache is a cross-user leak. There is no cache to key | **Enforced by absence.** No mechanism can prove one: adding any cache of rows is an architecture decision, keyed on subject first or not at all |
| An edit to a definition | Editing one forks the definition from the number it certifies. Definitions are authored upstream | **Enforced.** Nothing on the query path writes to the catalogue, and the bundle is hashed, so an edit moves the digest that travels with the answer |
| A query as the service identity | A leg that cannot run as the subject should be refused rather than downgraded | **Enforced, and narrower than it reads.** `Warehouse::execute` takes a credential a broker minted for that source and has no signature that omits one, so there is no fallback to downgrade THROUGH; a subject with no credential is refused as `credential_unavailable`, and an adapter handed material it cannot use returns an error rather than answering. What is NOT enforced is the sentence people hear in it: no adapter in this build can carry a per-subject credential, so a leg still runs as this process - by declaration and with the answer recording that it did |
| An unbounded time range | A range has to be bounded to resolve at all | **Enforced.** `TimeRange` has no unbounded form, so an absent bound fails to deserialize and the refusal is unprovokable |
| A range too long to be worth answering | A bounded range still permits a full scan: two real dates can be ten thousand years apart | **Enforced, on the caller's path only.** Resolution refuses a span over ten years as `TimeRangeTooLong`, carrying two derived integers and nothing of the caller's text. It is checked there rather than in the type because the same type carries a metric's anchor range, which an author writes and no caller can reach. **What it does not bound:** the work inside a permitted span, or a caller asking three permitted questions in a row - a per-caller budget needs a clock and a subject, which is the same absent port as the identity row above |
| A definitional filter removed or renamed | A metric's required filters are part of what it means | **Enforced.** They are compiled into every plan for the metric and marked as definitional; a caller has no field that could name, select or remove one |

## Status

**A governed single-player semantic compiler and executor over local files.** That is what is here:
`sutura compile` renders the statement for a question and `sutura query` answers it, over a
catalogue of documents in git and the CSV or Parquet files in a directory you name, with every
certified number re-executed before the bundle may be served.

Everything marked *design target* above is unbuilt. The identity path is no longer one of them and is
not finished either: there IS a request context, a credential broker port with a static-credential
implementor, an audit sink and an MCP surface, and a deployment that declares `security.inbound`
verifies a caller's own token. What there is NOT is an adapter that can carry a per-subject
credential, so per-caller ACCESS is still absent - and a deployment behind only the bearer token has
no per-caller identity at all, because that token authenticates the deployment. No Arrow envelope. [What exists today](architecture.md#what-exists-today) is the inventory, and `AGENTS.md` in the
repository lists each invariant beside the type, lint or gate that holds it - including the rows
that say outright that nothing holds them yet.

The mechanisms came first on purpose: every claim on this page is meant to be held up by a type,
a lint, a hook or a gate, and those are cheaper to build before there is code to retrofit them
onto.
