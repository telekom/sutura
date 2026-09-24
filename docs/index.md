---
title: Introduction
description: What sutura is, what it enforces today, what it does not yet, and which page to read next.
---

# sutura

sutura is being built to answer questions about data **as the person or agent asking**, using
metric definitions somebody certified, and to refuse when it cannot do either.

Give an agent a database connection and it answers with SQL it invented, run under whatever
credential the service holds. Two failures, not one. The number is uncertified, so nobody can
say whether "revenue" means what finance means by it. The rows are the ones the *service* may
read rather than the ones the *caller* may read, which is how a row-level security policy
becomes decorative.

!!! warning "What is built, and what is a design target"

    sutura serves governed questions over local files and declared data sources. The path that
    executes as a verified caller is **built and unproven**. A request context, a credential broker
    port, an audit sink and
    an MCP surface all exist, and a deployment that declares `security.inbound` verifies a caller's
    own token - so who is asking can be known, no question can execute without a credential minted
    for the source it reads, and every outcome is recorded. A published build links BigQuery's
    per-subject credential path, but no served run has proven source execution as the asker. Shared
    sources still read under their declared identity. No Arrow result envelope either. Every claim on this
    site is marked *enforced today* or *design target* at the point it is made.

## The four properties

Each row names the property, the mechanism it is meant to rest on, and whether that mechanism
holds it in the code that is here now.

| Property                        | The mechanism it rests on                                                                                                                                       | Status                                                                                                                                                                                                                                                                                                                                                                        |
| ------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Every query runs as the caller  | A credential minted per request for the calling principal. A leg that cannot run as the subject is refused, never downgraded to a service identity              | **Design target, not built.** No caller identity reaches the query path as the subject, and in single-player the property is trivially true and worth nothing: a file has no login, so there is nobody else to be                                                                                                                                                             |
| A refusal is an answer          | Refusal is a variant of the result type rather than an error return, so a caller cannot mistake it for a hiccup and retry until something works                 | **Enforced today.** `ToolOutcome::Refusal` is the public surface and the golden suite provokes every reachable variant. Recording it against the principal chain is enforced too - every outcome goes through an `AuditSink` before it is returned - with two limits: sutura retains nothing, and behind the shared bearer token alone the subject recorded is the deployment |
| You cannot ask it to run SQL    | The tool surface has no field for a query, a table or a filter. An uncertified question is unrepresentable, not merely refused                                  | **Enforced today.** `Query` declares no such field, `deny_unknown_fields` turns an attempt into an error naming it, and a golden asserts no value a question carries reaches the statement as text                                                                                                                                                                            |
| Definitions come from elsewhere | They are authored in a semantic layer and arrive pinned and hashed. Nothing here edits one, because that would fork the definition from the number it certifies | **Enforced today.** The load path takes no request context, the bundle is hashed, and every declared anchor re-executes before the bundle may be served                                                                                                                                                                                                                       |

Those mechanisms are the design. [Architecture](architecture.md) says how the four force the shape
of the system and which parts are compiled today. `AGENTS.md` in the repository lists every invariant
beside the type, lint or gate that enforces it - including the rows that say outright that nothing
enforces them yet.

## What it borrows

Two Apache-2.0 projects got there first.

- **[Wren](https://github.com/Canner/WrenAI)** compiles a modelled question into SQL over
  [DataFusion](https://datafusion.apache.org/). The compile-a-plan-from-a-model shape comes from
  there.
- **[Spice](https://github.com/spiceai/spiceai)**, also DataFusion-based, federates and accelerates
  queries across sources.

Between them they cover compiling a question and federating it. What sutura *means to* add is
identity: not only what a question means, but who is asking and whether they may see the answer.
That is the designed and unbuilt part, so what differs today is the narrow tool surface and the
pinned bundle, not the identity.

[Where the parts come from](architecture.md#where-the-parts-come-from) sets those two and
[polyglot](https://github.com/tobilg/polyglot) on the line from a modelled question to executed SQL,
and says which parts we mean to build.

## Where to start

| You want to                                  | Read                                  |
| -------------------------------------------- | ------------------------------------- |
| Install it and ask a question                | [Getting started](getting-started.md) |
| Know what the words on the tool surface mean | [Concepts](concepts.md)               |
| Ask the short questions first                | [Questions and answers](qa.md)        |
| Understand the shape of the system           | [Architecture](architecture.md)       |
| Read the Rust API                            | [API reference](api/index.md)         |

To work *on* sutura, start at [Contributing](contributing.md), under **Building it**.

## Status

The query path is built, and it supports exactly one combination: **metadata from a catalogue of
markdown documents with YAML frontmatter in git, executed by the in-process engine over the CSV or
Parquet files you point it at.** A question naming a metric, a grain, a bounded range and some
dimensions compiles to a plan and executes, and every metric that declares a certified number
reproduces it before the bundle can be served.

The plan can also be *rendered* as SQL for `DuckDB`, Postgres or `ClickHouse`, and there is a `DuckDB`
adapter that pushes a statement down - but that adapter is a **test dependency**, not the runtime data
source, and the shipped binary links neither it nor any driver. Which DATA SYSTEM is opened is a
declaration - `sources.<alias>.kind`, the same tree the service reads - and which KINDS a given build
can open is a compile-time decision: a published binary opens `files`, `bigquery` and `postgres`,
while `kind: clickhouse` and `kind: oracle` still need builds carrying their own default-off
features. A build without one refuses that kind by name rather than silently falling back.
[What can be plugged in today](architecture.md#what-can-be-plugged-in-today-and-what-the-shipped-binary-actually-uses)
is the table, and it is the section to read before assuming otherwise.

What remains unproven is whether a served BigQuery leg executes as the verified caller. There is a request context, a credential broker port
with a static-credential implementor, an audit sink, an MCP surface, and - where a deployment
declares `security.inbound` - a caller identity verified from a signature, with scopes deciding which
operations that caller may invoke. Leg 2 is built and unproven: the published BigQuery adapter can
carry a verified caller's assertion through its declared per-subject map, but no served run has
shown source execution as that caller. Shared sources still read under their declared identity.
Arrow results are also still ahead. Federation and [the HTTP surface](serving.md) are built. In
static access-token mode, the token authenticates the deployment rather than the caller.
[What exists today](architecture.md#what-exists-today) is the honest inventory.

## Feedback and reporting

For questions, feature ideas and bug reports, open an issue in the
[issue tracker](https://github.com/telekom/sutura/issues). For a suspected vulnerability, report it
privately rather than in a public issue - see
[SECURITY.md](https://github.com/telekom/sutura/blob/main/SECURITY.md).
