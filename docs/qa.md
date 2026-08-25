---
title: Questions and answers
description: Short answers to the questions this design provokes first.
---

# Questions and answers

Where another page says it better, this one links rather than restates.

## Why not just give the agent a database connection and let it write SQL?

Two things go wrong at once, and only one of them is obvious. An agent that invents SQL invents a
definition with it, so nobody can say whether "revenue" means what finance means by it. And the rows
come back according to what the *service* may read rather than what the *caller* may read, which is
how a row-level security policy becomes decorative. sutura fixes both by construction: definitions
arrive certified and pinned, and every query runs as the caller.

## Why is a refusal not an error?

An error invites a retry. A refusal is a variant of the result type with a typed reason, so a
caller cannot mistake "you may not ask this" for a transport hiccup and loop until something
answers. Every refusal is recorded with the whole principal chain, which makes a refused call as
attributable as an answered one.

## Why does the tool surface take no table name?

Because a refusal can be retried, reworded and eventually satisfied, and an absent field cannot.
`Query` carries no SQL, no table, no filter expression and no row ids, so an uncertified question
does not compile. Widening that surface changes a generated schema, which puts the widening in
the diff of the review that did it.

## Why is there no result cache?

Under row-level security, two callers asking the same question are entitled to different rows. A
cache keyed on the query text serves the first caller's rows to the second: a cross-user leak
with a hit rate. The same reasoning rules out a materialised copy refreshed on a schedule, which
is read under whoever refreshed it.

No mechanism can prove an absence, so this one is written down as a decision. Adding any cache of
rows is an architecture change, keyed on subject first or not at all.

## Why can a question not span two data systems?

A second data system is a second identity to satisfy, not a bigger version of the same query. Today
a plan resolves to exactly one source and a test asserts it.

Federation is wanted, in this order: per-leg identity first, then federation. A predicate pushed into
ClickHouse or Postgres is evaluated there, under the caller's own grants, so excluded rows never
enter this process.
[Where the parts come from](architecture.md#where-the-parts-come-from) names the projects that
already do that part well.

## Why are definitions not editable here?

Editing a certified statement forks the definition from the number it certifies, which was the
only thing certifying it bought. Definitions are authored in the semantic layer that renders
them and arrive pinned and hashed. A wrong definition is wrong upstream.

## What happens if the agent asking is manipulated?

For a system whose input is natural language from wherever the user found it, a manipulated agent
is the expected case rather than the disaster case, and the defence is not detecting it. The most
an attacker can make the agent emit is a different certified question, asked as the same caller,
over the same pinned definitions, against the same authorization. The blast radius of a fully
manipulated agent is the set of questions its caller could already ask.

## Why do the musl builds swap the allocator?

musl's `mallocng` serialises the whole process on one lock word. With one binary and only
threading toggled, a 48-core run takes 4.45s on glibc and 92.16s on musl, slower than musl's own
single-core run; linking mimalloc brings it to 3.83s.
[What ships](architecture.md#what-ships) has the details, including the cost of `MI_SECURE=4`.

## Can I use it today?

No. Four packages exist: the domain types, the binary, the repository gates and a local development
CLI. There is no query path, no MCP server and no adapter for any data system.
[What exists today](architecture.md#what-exists-today) is the inventory.

The environment, the gates and the release pipeline do work, because a mechanism is cheaper to build
before there is code to retrofit it onto.

## How do I work on it?

[Contributing](contributing.md) covers the three routes to an environment, the two toolchains and
which gates run when. On a network with no direct egress, read
[Building without direct egress](enterprise-mirrors.md) first.
