---
title: First-party semantic models
description: Why sutura generates a metric's SQL from a model it can read, instead of only splicing a statement authored elsewhere.
---

# First-party semantic models

Status: accepted. It supersedes nothing; it adds a second way for a definition to arrive.

**Amended.** The measure vocabulary described below - one aggregate over one declared column - was
widened by [a closed vocabulary for measures](0002-a-closed-vocabulary-for-measures.md), which adds
a conditional count, a ratio of two aggregates, and filters that belong to a metric's definition.
Nothing in the argument here changes, because the argument is about free-text SQL rather than about
arity: read the constraint in [what a model may not contain](#what-a-model-may-not-contain) as the
ban on expressions it is, and not as the one-aggregate limit it also happened to be.

## Context

[Architecture](../architecture.md) describes one path from a question to a number: a metric's SQL is
authored in a semantic layer outside this repository, arrives as text with a digest, and is spliced
into a generated wrapper byte for byte without being parsed. That path exists because re-emitting
somebody else's statement substitutes our reading of it for theirs, and the digest would not move.

The path has a precondition that is easy to miss: **something upstream has to have rendered the
statement.** Where a build-time semantic layer already emits dialect-correct SQL per metric, taking
it as given is strictly better than re-deriving it. Where there is no such upstream, on a laptop, over
a single Parquet file, on a first run against a new warehouse, the precondition is simply absent.
There is no statement to splice, and the design as written has nothing to say about that case.

That is not a small corner. It is the case a person is in when they first try this.

## Decision

**A catalog may declare a model, and sutura will generate the whole statement from it.**

A model names a physical table, its columns, and its relationships to other models. A metric names a
model, an aggregate over one of its columns, the grains it supports, the dimensions it may be broken
down by, and its time column. From those, `sutura-semantic` generates the entire statement:
projection, grouping, the bounded date predicate, the join for a single-hop dimension, and quoting
and placeholders per dialect. Nothing is spliced, because nothing arrived as text.

The invariant this repository actually cares about is unchanged, and gets easier to hold:

> We never re-parse SQL we did not generate.

When we generate all of it, there is no foreign SQL on the query path to re-parse. The rule holds by
construction rather than by discipline.

### What a model may not contain

**No free-text SQL expression, anywhere.** A measure is an aggregate from a closed set over a named
column, not the string `sum(amount)`. A relationship is a pair of columns and a join type, not the
string `orders.customer_id = customers.id`. A dimension is a column, optionally reached through one
declared relationship.

This is the load-bearing half of the decision, and it is where this design departs from the
modelling languages it otherwise resembles. Those carry SQL fragments as strings and parse them
downstream. A string field is an escape hatch, and an escape hatch on the query path is the thing
being defended against: it turns "the catalog describes the data" into "the catalog can say
anything", and every check downstream then has to reason about text somebody wrote.

The cost is real and worth stating. `sum(price * quantity)` cannot be expressed, and neither can a
window function or a three-table join. Those are the cases the spliced-statement path exists for:
where a definition is genuinely more than a model can say, it should be authored where the
expressiveness lives and arrive certified. **The two paths are for two different situations, and
neither is a degraded version of the other.**

Adding an expression field later is an architecture decision, not a feature. It would need its own
entry here, and the question it has to answer is not whether the expression is parsed but what
happens the first time one arrives that we cannot parse.

## What does not change

Every other guarantee is untouched, and each still has the same mechanism behind it:

| Guarantee | Still held by |
| --- | --- |
| Definitions are pinned and hashed, so a catalog edit cannot change what a question means between two invocations | The digest is taken over the canonical form of the parsed definitions. It moves on any semantic edit, and it travels with the answer |
| A catalog cannot see who is asking | `SemanticCatalog::load` takes no request context, so there is nothing to branch on |
| An unvalidated bundle is never served | The service accepts only `Validated<PinnedDefinitions>`, and the anchor check is its only constructor |
| A definition that has stopped computing its own number fails readiness | The anchor test re-executes each metric and compares against the number its author declared |
| Refusal is a result, not an error | `ToolOutcome::Refusal { reason }`, with a test per variant |
| No SQL, table, predicate or row id on the tool surface | `Query` has no field for one. This decision changes what a *catalog* may say, and nothing about what a *caller* may say |
| A mono answer resolves to one data system | The plan stage gathers the source of every model the statement would read - the metric's own, and each one reached through a join - into a set. One is a mono plan; exactly two are split into a fact leg and a lookup leg; three or more refuse as `PlanSpansTooManySources` |
| No result cache | Nothing added one |

One new guarantee arrives with the generator, and it is mechanical rather than argued: **no value
from a question reaches the statement as text.** Every one becomes a bind parameter, and a golden
test asserts that no literal from the question appears in the generated SQL. Before this decision the
claim was structural, because there was no generator. Now it is a check that can fail.

## Consequences

- The catalog on disk is now a source of definitions rather than only a carrier of them. Reviewing a
  catalog change is reviewing what will execute, which is a heavier kind of review than approving a
  text passthrough, and it belongs in the same pull request as any other change to behaviour.
- The generated SQL is a reviewable artifact. Goldens are regenerated and read as a diff, never
  typed, because a hand-written expectation is an assertion about what we wish the generator did.
- Dialect differences become ours. Placeholder syntax, identifier quoting and date truncation differ
  per data system, and the generator owns all three. It must own placeholder style *explicitly*: the
  dialect layer we build on renders a placeholder the same way regardless of target, so relying on it
  would silently emit the wrong syntax for anything but the first dialect we shipped.
- We do not translate between dialects. A transpile is a parse followed by a re-emit, and it does not
  reliably repair the differences that matter. A measured example is date truncation, where argument
  order differs between two dialects and translation left it unchanged, producing output that is not
  valid in the target. The generator renders into the target dialect from a plan, and the ban is a
  lint rather than a paragraph.
- The spliced-statement path stays documented and unbuilt. The dialect layer has a verbatim
  passthrough node whose generator appends the text unchanged and which no dialect pass rewrites, so
  the splice remains implementable exactly as [Architecture](../architecture.md) describes it.

## Alternatives considered

**Only splice, and require an upstream renderer.** Correct where one exists, and no help at all
otherwise. It makes the first thing a new user tries the hardest thing to set up, and it puts the
whole design behind a dependency a laptop does not have.

**Only generate, and delete the splice.** Tempting, because one path is simpler than two. Rejected:
where a definition has already been certified upstream, re-deriving it is exactly the substitution of
our reading for the author's that the splice exists to prevent. Deleting it would trade a real
guarantee for tidiness.

**Generate from free-text expressions, as the reference modelling languages do.** More expressive,
and it moves a parser onto the query path, where a parse failure is a runtime refusal instead of a
review comment. The closed set of aggregates covers the metrics a model can honestly describe, and
the ones it cannot belong on the other path.
