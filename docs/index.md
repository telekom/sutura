---
title: Introduction
description: What sutura is, what it guarantees, and which page to read next.
---

# sutura

sutura answers questions about data **as the person or agent asking**, using metric
definitions somebody certified, and refuses when it cannot do either.

Give an agent a database connection and it answers with SQL it invented, run under whatever
credential the service holds. Two failures, not one. The number is uncertified, so nobody can
say whether "revenue" means what finance means by it. And the rows are the ones the *service*
may read rather than the ones the *caller* may read, which is how a row-level security policy
becomes decorative.

## The four properties

| Property | The mechanism it rests on |
| --- | --- |
| Every query runs as the caller | A credential minted per request for the calling principal. A leg that cannot run as the subject is refused, never downgraded to a service identity |
| A refusal is an answer | Refusal is a variant of the result type rather than an error return, so a caller cannot mistake it for a hiccup and retry until something works |
| You cannot ask it to run SQL | The tool surface has no field for a query, a table or a filter. An uncertified question is unrepresentable, not merely refused |
| Definitions come from elsewhere | They are authored in a semantic layer and arrive pinned and hashed. Nothing here edits one, because that would fork the definition from the number it certifies |

Those mechanisms are the design. [Architecture](architecture.md) says how the four force the
shape of the system and which parts are compiled today. `AGENTS.md` in the repository lists
every invariant beside the type, lint or gate that enforces it.

## What it borrows

Two Apache-2.0 projects got there first.

- **[Wren](https://github.com/Canner/WrenAI)** compiles a modelled question into SQL over
  [DataFusion](https://datafusion.apache.org/). The compile-a-plan-from-a-model shape comes from
  there.
- **[Spice](https://github.com/spiceai/spiceai)**, also DataFusion-based, federates and accelerates
  queries across sources.

Between them they cover compiling a question and federating it. What sutura adds is identity: not
only what a question means, but who is asking and whether they may see the answer.

[Where the parts come from](architecture.md#where-the-parts-come-from) sets those two and
[polyglot](https://github.com/tobilg/polyglot) on the line from a modelled question to executed SQL,
and says which parts we mean to build.

## Status

The design is settled; the code is a walking skeleton. The environment, the release pipeline
and the gates that hold the properties above exist. The query path does not.

## Where to start

| You want to | Read |
| --- | --- |
| Know what the words on the tool surface mean | [Concepts](concepts.md) |
| Ask the short questions first | [Questions and answers](qa.md) |
| Understand the shape of the system | [Architecture](architecture.md) |
| Read the Rust API | [API reference](api/index.md) |
| Install it and ask a question | [Getting started](getting-started.md), once the query path exists |

To work *on* sutura, start at [Contributing](contributing.md) under **Development**.
