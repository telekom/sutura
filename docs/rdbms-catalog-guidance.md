---
title: An RDBMS dictionary as a metadata source
description: What a database dictionary can contribute as metadata, what it cannot certify, and the identity boundary on what the reader can see.
---

# An RDBMS dictionary as a metadata source

This implements the dictionary half of issue #151 and
`docs/adr/0011-pluggable-by-declaration.md`'s RDBMS row: a metadata source that reads a database's
own dictionary - `information_schema` plus whatever table comments a human wrote - and declares
**structure and prose** and nothing else. It is the narrowest
declaration this repository makes, and it exists because a database with DDL and comments and no
semantic layer is where every adoption starts. [A raw SQL tool](adr/0013-a-raw-sql-tool-off-by-default.md)
is the ramp that deployment's story continues; this page is the catalog half of it.

This page describes the adapter that exists. No runtime prompt consumes source-specific guidance
from it yet.

## What this source is, and what it is not

A dictionary is mainly DDL and comments: the tables, the columns, their constraints, and prose written
against them. The connector that reads it provides exactly that, and the declaration says so
explicitly:

- **Structure** - the tables and their columns.
- **Descriptions** - table comments.
- **Relationships** - the join a foreign key records, in the safe direction: a primary or unique
  constraint on the referenced column is required evidence before the join maps to `ManyToOne`.

And nothing else. There is **no measure, no grain, no definitional filter, no value allowlist, no
anchor and no cardinality** - a human declares those in a semantic layer. A bundle from this source
therefore loads, pins and validates with **zero metrics**, and answers no certified question on its
own. That is not a limitation the deployment is missing; it is the honest statement about what a
physical schema, by itself, knows.

The dictionary is filtered by the identity used to read it: database dictionaries expose only the
objects visible to that identity. `load()` has no request identity, and this adapter does not build
impersonation at the source.

A foreign key is the one apparent exception and it is worth saying exactly why it is not one. A
relationship arrives with its endpoints, but **no declared cardinality** - the dictionary vouches for
the direction a constraint can prove and no fan-out it cannot - so the relationship does **not** by
itself license a dimension. That is `a_foreign_key_licenses_no_dimension_without_a_declared_cardinality`.

## Promoting a number into one

This is the ramp, and it is the reason the "answers no certified question" sentence is not a dead end.
A deployment that runs a physical schema today can make a number certified without leaving this quiet
starting point:

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

## The two sentences that may not be written

`docs/adr/0016-what-datahub-can-carry.md` decision 7 established the rule for a narrow connector, and
this page follows the same writing boundary. Two sentences are prohibited:

- **"Configure your database like this."** It is not ours to say. A database's DDL, constraints and
  comments are whatever they already are, and nothing here turns a particular way of configuring them
  into a precondition. Every choice named on this page is an option with a payoff.
- **"This source is not usable without X."** It is false. The source is usable with zero metrics, zero
  comments, and no raw tool - it loads a bundle whose declaration is exactly the empty-of-metric
  truth.

Options name their payoff without becoming preconditions.
