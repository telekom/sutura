---
title: Concepts
description: The words on the tool surface, and what each one commits to.
---

# Concepts

The words on the tool surface, and what each one commits to. The design is settled and the code
is not, so this page is vocabulary rather than an API you can call.
[Architecture](architecture.md) says how the pieces fit.

## A question, and what it is made of

A question names a **metric**, some **dimensions**, a **grain** and a bounded **time range**.
That is the whole vocabulary. There is no field for SQL, a table, a filter expression or a list
of row ids, so a question outside this shape is unrepresentable rather than refused.

**Metric.** A named measure somebody certified: revenue, active subscribers, churn. The name
comes from a semantic layer outside this repository, and it arrives with the statement that
computes it.

**Dimension.** An attribute a metric declares it can be broken down by: region, product, tariff.
A dimension the metric does not declare is not a narrower question, it is a name that does not
resolve, and it comes back as a refusal naming the argument that failed. Dimension *values* are
arguments checked against an allowlist in the pinned bundle, never text pasted into a query.

**Grain.** The time resolution the answer is aggregated to, and one the metric supports. Daily
revenue and monthly revenue are the same metric at two grains, not two metrics.

**Time range.** Bounded, always. An unbounded range is a table scan with a plausible name.

## A certified definition

A **definition** is the metric's meaning plus the **statement** that computes it: SQL text authored
upstream in the semantic layer. sutura does not write, edit, parse or rewrite that statement. It is
spliced into a generated wrapper byte for byte, because re-emitting it would substitute our reading
for the author's and the number would change quietly.

An **anchor** is a known result for a statement. The anchor test re-executes each pinned statement
and compares, in CI and again at startup, so a definition that has stopped meaning what it claimed
fails readiness rather than answering.

## The pinned snapshot

Definitions do not arrive live. They arrive as a **pinned snapshot**: a bundle of definitions
with a version and a **digest** over the text. Two consequences follow:

- A catalogue edit cannot change what a question means between two invocations. It changes the
  digest, and the digest travels with the answer.
- The catalogue cannot see who is asking. The load path takes no request context, so it cannot
  return one definition to one caller and a different one to another.

A result carries the version and digest of the definitions that produced it, so a number traces
back to the text that defined it. That is what makes "certified" checkable rather than asserted.

## Refusal

A **refusal** is an answer, not an error. It is a variant of the result type, with a reason, so
a caller cannot mistake it for a transport hiccup and retry until something works.

Reasons are typed rather than prose, because the variant is the contract and the message is not.
A dimension that does not resolve, a range that is not bounded, a plan that would need two
identities, a source unreachable as the calling subject: each is its own reason, recorded with
the whole principal chain before it is returned.

## Principal, subject, and running as the caller

The **principal** is who is asking. When an agent asks on somebody's behalf there is a chain of
them, and the **subject** is the identity the data system must see: the person, not the service.

Every query runs as the subject. A credential is minted per request, and a leg that cannot run
as the subject is refused rather than falling back to the service's own identity. That fallback
turns "you may not see these rows" into "here are the rows".

**sutura holds no copy of who may see what.** Grants, row-level policies and masking live in the
data system, administered and audited by the people who own the data. A second copy here could
disagree with the original, with no way to tell which one is right.

## Catalogue and data system

Two ports, deliberately separate.

A **catalogue** supplies definitions: metrics, dimensions, the glossary, lineage. A directory of
YAML in git and a metadata catalogue with an HTTP API are two adapters behind one trait.

A **data system** executes. ClickHouse and Postgres are the near-term targets, with DuckDB for
local and single-file work. The port is named `Warehouse`, which says nothing about what sits
behind it.

A plan resolves to exactly **one** data system. Spanning two is not a bigger version of the same
problem, it is a second identity to satisfy, and a plan whose legs cannot all run as one subject
is refused rather than run partly as somebody else.

## Provenance, and why results are Arrow

Rows, column descriptions and glossary text are authored by somebody else, and any of it can contain
something shaped like an instruction. A delimiter cannot separate instruction from data, because the
content can contain the delimiter.

So results leave as Arrow with **provenance** in the schema metadata: a typed field a caller reads
deliberately, never a string concatenated into the channel that carries instructions. The definition
version and digest ride in the same envelope, so a result cannot be separated from what defined it.

## What you cannot ask for

Absences by design:

| Not available | Why |
| --- | --- |
| SQL, a table name, a filter expression, row ids | The tool surface has no field for any of them. An uncertified question is unrepresentable, not merely refused |
| A cached result | Under row-level security a query-keyed cache is a cross-user leak. There is no cache to key |
| An edit to a definition | Editing one forks the definition from the number it certifies. Definitions are authored upstream |
| A query as the service identity | A leg that cannot run as the subject is refused. There is no downgrade path |
| An unbounded time range | A range has to be bounded to resolve at all |

## Status

Most of this is callable now, over a catalogue of documents in git and a `DuckDB` file:
`sutura compile` renders the statement for a question and `sutura query` answers it.
[What exists today](architecture.md#what-exists-today) is the inventory, and it is honest about the
one absence that matters - there is no credential broker, so "every query runs as the calling
principal" holds here only because a file has nobody else to be.

The mechanisms came first on purpose: every claim on this page is meant to be held up by a type,
a lint, a hook or a gate, and those are cheaper to build before there is code to retrofit them
onto.
