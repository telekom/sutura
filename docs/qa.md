---
title: Questions and answers
description: Short answers to the questions this design provokes first.
---

# Questions and answers

Where another page says it better, this one links rather than restates.

## Why not just give the agent a database connection and let it write SQL?

Two things go wrong at once, and only one of them is obvious. An agent that invents SQL invents a
definition with it, so nobody can say whether "revenue" means what finance means by it. The rows
come back according to what the *service* may read rather than what the *caller* may read, which
is how a row-level security policy becomes decorative.

sutura fixes the first by construction today: definitions arrive certified and pinned, the tool
surface has no field an invented query could arrive in, and every value a question carries binds
as a parameter rather than reaching the statement as text.

The second is **secure-impersonation** (BigQuery). "Every query runs as the caller" needs a
credential minted per request. A request context reaches the query path, a credential broker mints
once per answer for every source a plan reads, and the execution port has no signature that runs
without the result - so a subject with no credential at a source is refused rather than answered
as this process. The BigQuery adapter carries the verified caller's assertion through a declared
per-subject map. A **shared-service-user** source (Postgres, ClickHouse, Oracle, files) uses its declared
identity. The boot path re-executes every anchor before a listener is bound, there is no caller then, and `Warehouse::verify_anchor`
takes no credential.

**What bounds that path is placement made checkable, not its input type - a correction a second
review forced.** `verify_anchor` takes an `AnchorPlan`, a plan the pinned bundle itself agrees is
a declared anchor's own: the metric must be defined and anchored, the range the one the bundle
certifies, the grain the coarsest that metric declares, and there may be no group-by key and no
predicate a question asked for. Every one of those facts is read off the bundle, so the check
catches a boot path that compiled the wrong question. It does **not** stop code that wants to
reach the method: the constructor is public, every value it reads is publicly constructible, and
Rust has no cross-crate friend visibility. So the mechanism that makes the credential-free path
boot-only is a lint - `clippy.toml` bans the method and the boot path holds the single
expectation, so a second call site is a build error until somebody writes a second one a reviewer
sees. A lint reaches this workspace and not a crate outside it; that is the limit, and it is
stated on the type as well.

## Why is a refusal not an error?

An error invites a retry. A refusal is a variant of the result type with a typed reason, so a
caller cannot mistake "you may not ask this" for a transport hiccup and loop until something
answers. That part is enforced today, and the golden suite provokes every variant a question can
reach.

Recording each refusal with the whole principal chain is **built**: an `AuditSink` takes every
outcome, refusal and answer alike, before it is returned, and `sutura-runtime` ships the
structured writer a deployment that attaches nothing else gets. Two limits, both deliberate.
sutura **retains nothing** - what a record is worth is what the deployment's sink is worth. And
the subject in the chain is only as strong as what established it: behind the shared bearer token
alone the record names the *deployment*, because that is who asked as far as anything can tell; a
deployment that declares `security.inbound` names the caller, from a signature.

## Why does the tool surface take no table name?

Because a refusal can be retried, reworded and eventually satisfied, and an absent field cannot.
`Query` carries no SQL, no table, no filter expression and no row ids, so an uncertified question
does not compile. That is enforced today, and `deny_unknown_fields` means a question carrying
`sql:` is an error naming the field rather than one silently dropped.

The MCP wire type generates a committed schema snapshot, so widening its input changes a
byte-compare a reviewer must accept. There is no equivalent OpenAPI dump for HTTP, and a snapshot
cannot decide whether a new field should exist; review still owns that decision.

## Why is there no result cache?

Under row-level security, two callers asking the same question are entitled to different rows. A
cache keyed on the query text serves the first caller's rows to the second: a cross-user leak with
a hit rate. The same reasoning rules out a materialised copy refreshed on a schedule, which is
read under whoever refreshed it.

No mechanism can prove an absence, so this one is written down as a decision. Adding any cache of
rows is an architecture change, keyed on subject first or not at all.

## Can a question span two data systems?

Yes. A supported federated question splits into fact and lookup legs, executes each at its source,
then joins the results. Each leg is presented either the asking subject's credential or that
source's acknowledged shared identity. The answer records the posture of each leg, even when they
differ. That record reaches the caller with the rows; it is a
disclosure, not an authorization check.

BigQuery supports secure-impersonation ([BigQuery identity](integrations/data-systems/bigquery.md#identity)).
The other adapters run as a shared service user.

## Why are definitions not editable here?

Editing a certified statement forks the definition from the number it certifies, which was the
only thing certifying it bought. Definitions are authored in the semantic layer that renders them
and arrive pinned and hashed. A wrong definition is wrong upstream.

## What happens if the agent asking is manipulated?

For a system whose input is natural language from wherever the user found it, a manipulated agent
is the expected case rather than the disaster case, and the defence is not detecting it. The most
an attacker can make the agent emit is a different certified question over the same pinned
definitions - that much is enforced today by the shape of `Query`. The blast radius of a fully
manipulated agent is the set of questions its caller could already ask.

The clause "asked as the same caller, against the same authorization" holds for a
secure-impersonation source (BigQuery) and not for a shared-service-user source. The credential
broker exists and `Warehouse::execute` has no signature that runs without what it minted, so there
is no code path a question reaches a data system through as an unnamed identity. The BigQuery
adapter carries the verified caller's assertion to a declared account for that subject. Other
adapters execute under their source's shared identity. Where a deployment declares
`security.inbound`, the scopes that caller was granted decide which operations the caller may
invoke, not which rows an answer contains
([BigQuery identity](integrations/data-systems/bigquery.md#identity)).

## Why do the musl builds swap the allocator?

musl's `mallocng` serialises the whole process on one lock word. With one binary and only
threading toggled, a 48-core run takes 4.45s on glibc and 92.16s on musl, slower than musl's own
single-core run; linking mimalloc brings it to 3.83s.
mimalloc is built with `MI_SECURE=4` (guard pages, random placement, encoded free lists and
double-free detection). That costs 23-43% against plain mimalloc and is still faster than glibc.

## How do I work on it?

[Contributing](contributing.md) covers the three routes to an environment, the two toolchains and
which gates run when. To report a problem, [open an issue](open-an-issue.md). To report a
vulnerability, read [Report a security issue](report-a-security-issue.md). On a network with no direct egress, read
[Building without direct egress](enterprise-mirrors.md) first.
