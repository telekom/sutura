---
title: Questions and answers
description: Short answers to the first questions about the design.
---

# Questions and answers

## Why not give the agent a database connection and let it write SQL?

Two things go wrong. First, an agent that writes SQL also invents the definition, so nobody can say if its "revenue" is
the finance definition. Second, the rows come back as the service account can read them, not as the caller can read them,
so row-level security has no effect.

sutura prevents both. The agent can ask only for certified definitions, and every value binds as a parameter. With
secure-impersonation (BigQuery, ClickHouse), the data system runs the query as the caller. A `shared-service-user`
source runs it as the one identity that the operator declares.

## Why is a refusal not an error?

An error invites a retry. A refusal is a typed result with a reason, so a caller cannot mistake "you may not ask this" for
a network problem. sutura writes one audit record, with the caller, for every answer and every refusal.

## Why does the tool surface take no table name?

A refused field can be reworded and retried until it passes. A field that does not exist cannot. `Query` has no SQL, no
table, no filter expression and no row ids, so an uncertified question cannot be expressed. A filter names a dimension
and values that the catalog allows. A question with an unknown field, such as `sql:`, is an error that names the field.

## Why is there no result cache?

Under row-level security, two callers who ask the same question can see different rows. A cache keyed on the question
would send the first caller's rows to the second caller. For the same reason, sutura keeps no scheduled copy of the data.

## Can a question span two data systems?

Yes. sutura splits the question into a fact leg and a lookup leg, runs each leg at its source and joins the results. Each
leg runs in the identity mode of its source, and the answer records the mode of each leg.

## Why can I not edit definitions here?

A definition is certified where it is written, in the semantic layer. An edit here would separate the definition from the
number it certifies. Fix a wrong definition at its source.

## What happens if someone manipulates the agent?

sutura expects it. The agent can ask only certified questions over the same pinned definitions, so a manipulated agent can
ask only what its caller can already ask. With secure-impersonation, the data system also applies the caller's own grants.
With `security.inbound`, the caller's scopes decide which operations the caller can use.

## Why do the musl builds use a different allocator?

The musl allocator serialises all threads on one lock. In a published benchmark of one binary on 48 cores, a run takes
92 s with it and 4 s with mimalloc. sutura builds mimalloc in its secure mode (`MI_SECURE=4`: guard pages, random
placement, encoded free lists and double-free detection).

## How do I work on sutura?

Read [Contributing](contributing.md). To report a problem, [open an issue](open-an-issue.md). To report a vulnerability,
read [Report a security issue](report-a-security-issue.md).
