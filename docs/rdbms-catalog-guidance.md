---
title: An RDBMS dictionary as a metadata source
description: What a database dictionary can contribute as metadata, what it cannot certify, and the identity boundary on what the reader can see.
---

# An RDBMS dictionary as a metadata source

> **Implementation status:** the crate currently converts dictionary records and is tested only
> through the fake `FixtureReader`. There is no production dictionary reader or database client,
> and no composition root links or serves this catalog. Operators cannot configure or use it in a
> shipped binary yet.

The crate implements the conversion half of issue #151 and
`docs/adr/0011-pluggable-by-declaration.md`'s RDBMS row. It is intended for a metadata source that
reads a database's own dictionary - `information_schema` plus whatever table comments a human wrote
- and declares **structure and prose** and nothing else. It is the narrowest
declaration this repository makes, and it exists because a database with DDL and comments and no
semantic layer is where every adoption starts. [A raw SQL tool](adr/0013-a-raw-sql-tool-off-by-default.md)
is the ramp that deployment's story continues; this page is the catalog half of it.

This page describes the converter's contract and the intended boundary of a future production
reader. The runtime prompt does not consume connector-specific text from this page. It derives the
same guidance from a pinned bundle: non-empty physical structure, a `Structure` declaration, and
zero metrics.

## What this source is, and what it is not

A dictionary is mainly DDL and comments: the tables, the columns, their constraints, and prose written
against them. The converter accepts exactly that, and the declaration says so
explicitly:

- **Structure** - the tables and their columns.
- **Descriptions** - table comments.
- **Relationships** - the join a foreign key records, in the safe direction: the referenced column
  must be the sole column of a primary or unique constraint before the join maps to `ManyToOne`.
  Membership in a composite constraint does not qualify.

And nothing else. There is **no measure, no grain, no definitional filter, no value allowlist, no
anchor and no cardinality** - a human declares those in a semantic layer. A bundle from this source
therefore loads, pins and validates with **zero metrics**, and answers no certified question on its
own. That is not a limitation the deployment is missing; it is the honest statement about what a
physical schema, by itself, knows.

A future production reader will see only the objects the database exposes to its configured
identity. `load()` has no request identity, and this converter does not build impersonation at the
source.

A foreign key is the one apparent exception and it is worth saying exactly why it is not one. A
relationship arrives with its endpoints, but **no declared cardinality** - the dictionary vouches for
the direction a constraint can prove and no fan-out it cannot - so the relationship does **not** by
itself license a dimension. That is `a_foreign_key_licenses_no_dimension_without_a_declared_cardinality`.

## Promoting a number into one

This is the intended ramp, and it is the reason the "answers no certified question" sentence is not
a dead end. Once a production reader and composition exist, a deployment can make a number certified
from this quiet starting point:

- Someone authors a **semantic layer** - a metric over a model the dictionary already reads. Combining
  that declaration with this adapter is outside the current implementation.
- That is the normal channel [a raw SQL tool](adr/0013-a-raw-sql-tool-off-by-default.md) is designed
  to feed: the ungoverned answer that worked is a written demand signal, and defining the metric
  makes the next identical question certified.
- The catalog parse validates the new declaration the same way it validates every other - a
  `Measure` with no expression field, dimensions checked against the model - so "define a metric" is
  a workflow with a checkpoint, not advice.

Each is an option with a payoff. This adapter itself remains a dictionary-only catalog with zero
metrics.

## What the runtime prompt says

A bundle with physical structure and zero metrics gets a dedicated prompt section. It tells the
agent three things the ordinary empty metric list cannot say on its own:

- no table or column in the bundle is a certified metric;
- descriptions, including database comments, are authored and untrusted descriptive prose, not a
  definition or an instruction;
- promoting a number means having a person author semantic metadata for it and loading that new
  definition before the number becomes askable.

The trigger is the pinned content and its digest-covered capability manifest, not an adapter name or
source label. That keeps the instruction true for any declaring source with the same physical-only
shape and keeps database names out of the prompt.

## What this page deliberately does not claim

`docs/adr/0016-what-datahub-can-carry.md` decision 7 established the rule for a narrow connector, and
this page follows the same boundary by keeping two claims out of the adapter contract:

- It does not prescribe a database configuration. A database's DDL, constraints and comments are
  whatever they already are, and nothing here turns a particular configuration into a precondition.
  Every choice named on this page is an option with a payoff.
- It does not make metrics, comments or a raw tool prerequisites for conversion. A dictionary
  without them converts to a bundle whose declaration is exactly the empty-of-metric truth.

Options name their payoff without becoming preconditions.
