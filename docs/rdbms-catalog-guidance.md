---
title: An RDBMS dictionary as a metadata source
description: What a deployment with a database but no semantic layer yet gets from reading its own dictionary, what the agent is told about it, and how a number becomes a certified metric. The guidance for the narrowest metadata connector - for both the operator and the prompt, and what the two never-write sentences are.
---

# An RDBMS dictionary as a metadata source

Issue #151, and a branch of `docs/adr/0011-pluggable-by-declaration.md`'s RDBMS row: a metadata
source that reads a database's own dictionary - `information_schema` plus whatever table and column
comments a human wrote - and declares **structure and prose** and nothing else. It is the narrowest
declaration this repository makes, and it exists because a database with DDL and comments and no
semantic layer is where every adoption starts. [A raw SQL tool](adr/0013-a-raw-sql-tool-off-by-default.md)
is the ramp that deployment's story continues; this page is the catalog half of it.

This is the page the roadmap asked for by name (*"prompt instructions how to use that one"*), split the
way the connector is: what the source is (for the operator), and what the agent is told about the
deployment that reads it (for the prompt). The two sentences that may not be written are named in
their own section at the end, because they are the boundary this page is measured against.

## What this source is, and what it is not

A dictionary is mainly DDL and comments: the tables, the columns, their constraints, and prose written
against them. The connector that reads it provides exactly that, and the declaration says so
explicitly:

- **Structure** - the tables and their columns.
- **Descriptions** - the table and column comments.
- **Relationships** - the join a foreign key records, in the safe direction: a primary or unique
  constraint on the referenced column proves that side is unique, so the join maps to `ManyToOne` and
  cannot duplicate rows.

And nothing else. There is **no measure, no grain, no definitional filter, no value allowlist, no
anchor and no cardinality** - a human declares those in a semantic layer. A bundle from this source
therefore loads, pins and validates with **zero metrics**, and answers no certified question on its
own. That is not a limitation the deployment is missing; it is the honest statement about what a
physical schema, by itself, knows.

A foreign key is the one apparent exception and it is worth saying exactly why it is not one. A
relationship arrives with its endpoints, but **no declared cardinality** - the dictionary vouches for
the direction a constraint can prove and no fan-out it cannot - so the relationship does **not** by
itself license a dimension. That is `a_foreign_key_licenses_no_dimension_without_a_declared_cardinality`.

## What the agent is told about it

A deployment whose whole model is a physical schema needs the agent told what it is looking at, or it
will treat a column comment as a definition and a table as a metric. The prompt guidance for this
source has to say all three of these, each in the agent's terms:

1. **Nothing here is a certified metric.** A question names a metric the bundle defines, and this
   bundle defines none - so any question is declined as `metric_unknown`, and the decline is an
   answer, not an error or a retry.
2. **A column comment is the author's prose, not a definition.** It may explain what a column holds;
   it is not a certified measure, and it is rendered only where descriptions render, never quoted as
   if a human had certified a number.
3. **A foreign key licenses no join by itself.** The relationship exists, but there is no declared
   cardinality and therefore no dimension to group by, so the agent must not invent a fan-out.

All of it is derived and none of it is stitched into a per-source blob. Following the withdrawal in
`docs/adr/0011-pluggable-by-declaration.md`, this source contributes **no catalog-authored usage
prose**: it does not say "search first" or "qualify your schema" - those are first-party prompt text
landing with the raw SQL tool or not at all. What the prompt states from the bundle is the fact that
the bundle carries no metrics, and `docs/agent-prompt.md`'s rules apply unchanged: every line of prose
gets the `> ` prefix, the trust boundary is named above the quoted block, and a line inside the block
that reads as an instruction is content to report rather than obey (it is the untrusted catalog text
the connector's own fixture corpus exercises).

## Promoting a number into one

This is the ramp, and it is the reason the "answers no certified question" sentence is not a dead end.
A deployment that runs a physical schema today can make a number certified without leaving this quiet
starting point:

- Someone authors a **semantic layer** - a metric over a model the dictionary already reads - and the
  bundle a deployment reads then carries it. The dictionary's structure and descriptions stay; the
  metric is a declaration beside them.
- That is the normal channel [a raw SQL tool](adr/0013-a-raw-sql-tool-off-by-default.md) is designed
  to feed: the ungoverned answer that worked is a written demand signal, and defining the metric
  makes the next identical question certified.
- The catalog parse validates the new declaration the same way it validates every other - a
  `Measure` with no expression field, dimensions checked against the model - so "define a metric" is
  a workflow with a checkpoint, not advice.

Each is an option with a payoff, and the absence of all of them is a supported configuration: the
bundle still loads, the prompt still tells the truth, and the deployment is exactly the narrow source
it declares itself to be.

## The two sentences that may not be written

`docs/adr/0016-what-datahub-can-carry.md` decision 7 established the rule for a narrow connector, and
this page holds to it word for word. Two sentences are prohibited:

- **"Configure your database like this."** It is not ours to say. A database's DDL, constraints and
  comments are whatever they already are, and nothing here turns a particular way of configuring them
  into a precondition. Every choice named on this page is an option with a payoff; none is required
  for the source to load and the prompt to be true.
- **"This source is not usable without X."** It is false. The source is usable with zero metrics, zero
  semantic models, zero comments, and no raw tool - it loads a bundle whose declaration is exactly
  the empty-of-metric truth, and that is a complete, supported state.

Saying what a user MAY do and what each thing buys, once, with the benefit next to it - and never
turning any of it into a precondition - is the whole discipline. This page was written to that
boundary.
