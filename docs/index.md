---
title: Introduction
description: What sutura is, what it enforces, and which page to read next.
---

# sutura

sutura answers questions about data with metric definitions that somebody certified. It answers
as the person or agent that asks, where the data system can do this (BigQuery). When it cannot answer correctly, it refuses.

An agent with a plain database connection writes its own SQL and runs it with the credential of
the service. This causes two problems. Nobody certified the number, so "revenue" can mean
something different from what finance means. And the rows are the rows that the service may read,
not the rows that the caller may read, so row-level security has no effect.

## The four properties

| Property                        | Mechanism                                                                                                                                                                                                                                                     |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| You cannot ask it to run SQL    | The question type has no field for SQL, a table or a filter. An extra field is refused, and every value becomes a bind parameter                                                                                                                              |
| Definitions come from a catalog | Definitions arrive as a pinned, hashed snapshot. sutura does not edit them. The catalog load has no request context, and every anchor runs again before sutura serves                                                                                         |
| A refusal is an answer          | A refusal is a result with a reason, not an error to retry. Every outcome goes to the audit sink before sutura returns it. The sink writes to the log, and sutura keeps nothing. Behind the shared bearer token alone, the recorded subject is the deployment |
| Every query runs as the caller  | A credential for each data system and caller. A leg that cannot run as the caller is refused. BigQuery runs as the caller (secure-impersonation). Every other data system runs as one identity that the operator declares (shared-service-user)               |

[Architecture](architecture.md) shows how these properties shape the system.

## What it borrows

Two Apache-2.0 projects did parts of this first:

- **[Wren](https://github.com/Canner/WrenAI)** compiles a modelled question into SQL over
  [DataFusion](https://datafusion.apache.org/). sutura takes the idea of a plan compiled from a
  model from Wren.
- **[Spice](https://github.com/spiceai/spiceai)** federates queries across data systems, also on
  DataFusion.

sutura adds identity: what a question means, who asks it, and whether they may see the answer.
`sutura-sql` uses [polyglot](https://github.com/tobilg/polyglot) to write each SQL dialect.

## Where to start

| Tab              | Read it to                                                              |
| ---------------- | ----------------------------------------------------------------------- |
| **User Guide**   | Install sutura, ask a question, and serve it over HTTP or MCP           |
| **Integrations** | Configure a catalog, a data system or the inbound identity              |
| **Examples**     | Run a complete setup: one user, several users, or a local chat client   |
| **Reference**    | See every setting, verify a release, build sutura, or read the Rust API |
| **Changelog**    | See what changed in each release                                        |

Start with [Getting started](getting-started.md). [Questions and answers](qa.md) has short
answers to common questions.

## Feedback and reporting

For questions, ideas and bugs, open an issue in the
[issue tracker](https://github.com/telekom/sutura/issues). Report a suspected vulnerability
privately, not in a public issue. See
[SECURITY.md](https://github.com/telekom/sutura/blob/main/SECURITY.md).
