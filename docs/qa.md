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
how a row-level security policy becomes decorative.

sutura fixes the first by construction, today: definitions arrive certified and pinned, the tool
surface has no field an invented query could arrive in, and every value a question carries binds as
a parameter rather than reaching the statement as text.

The second is **half built, and the missing half is the one that matters.** "Every query runs as the
caller" needs a credential minted per request, and that now exists: a request context reaches the
query path, a credential broker mints once per answer for every source a plan reads, and the execution
port has no signature that runs without the result - so a subject with no credential at a source is
refused rather than answered as this process. What is absent is a data system that evaluates the
asking subject: no adapter in this build can carry a per-subject credential. Against a local file the
property is trivially true and worth nothing, because a file has no login. Against a warehouse it is
still a target - what changed is that no QUESTION has a code path for a warehouse to be read as this
process through. The boot path does, by design: it re-executes every anchor before a listener is
bound, there is no caller then, and `Warehouse::verify_anchor` takes no credential.

**What bounds that path is placement made checkable, and not its input type - which is a correction a
second review forced.** `verify_anchor` takes an `AnchorPlan`, which parses a plan as one the pinned
bundle itself agrees is a declared anchor's own: the metric has to be defined and anchored, the range
has to be the one the bundle certifies, the grain has to be the coarsest that metric declares, and
there may be no group-by key and no predicate a question asked for. Every one of those facts is read
off the bundle, so the check catches a boot path that compiled the wrong question. It does **not** stop
code that wants to reach the method: the constructor is public, every value it reads is publicly
constructible, and Rust has no cross-crate friend visibility. So the mechanism that makes the
credential-free path boot-only is a lint - `clippy.toml` bans the method and the boot path holds the
single expectation, so a second call site is a build error until somebody writes a second one a
reviewer sees. A lint reaches this workspace and not a crate outside it; that is the limit, and it is
stated on the type as well.

## Why is a refusal not an error?

An error invites a retry. A refusal is a variant of the result type with a typed reason, so a
caller cannot mistake "you may not ask this" for a transport hiccup and loop until something
answers. That part is enforced today, and the golden suite provokes every variant a question can
reach.

Recording each refusal with the whole principal chain is **built**: an `AuditSink` takes every
outcome, refusal and answer alike, before it is returned, and `sutura-runtime` ships the structured
writer a deployment that attaches nothing else gets. Two limits, both deliberate. sutura **retains
nothing** - what a record is worth is what the deployment's sink is worth. And the subject in the chain
is only as strong as what established it: behind the shared bearer token alone the record names the
*deployment*, because that is who asked as far as anything can tell; a deployment that declares
`security.inbound` names the caller, from a signature. What is still a design target is a refusal
attributable to a subject whose own **access** decided it - the record can say which identity each leg
ran under, and on this build that is never the asking subject.

## Why does the tool surface take no table name?

Because a refusal can be retried, reworded and eventually satisfied, and an absent field cannot.
`Query` carries no SQL, no table, no filter expression and no row ids, so an uncertified question
does not compile. That is enforced today, and `deny_unknown_fields` means a question carrying `sql:`
is an error naming the field rather than one silently dropped.

The intended backstop - widening the surface changes a dumped schema, so the widening lands in the
diff of the review that did it - is a **design target**. The schema dump is not written yet, so
today the only thing catching a widened surface is review.

## Why is there no result cache?

Under row-level security, two callers asking the same question are entitled to different rows. A
cache keyed on the query text serves the first caller's rows to the second: a cross-user leak
with a hit rate. The same reasoning rules out a materialised copy refreshed on a schedule, which
is read under whoever refreshed it.

No mechanism can prove an absence, so this one is written down as a decision. Adding any cache of
rows is an architecture change, keyed on subject first or not at all.

## Why can a question not span two data systems?

A second data system is a second identity to satisfy, not a bigger version of the same query. The
one-source rule is enforced today: the plan stage collects every source the plan reaches into a set
and refuses unless exactly one name is in it, and a golden builds a two-source catalogue to provoke
the refusal.

The *identity* half of that reasoning is a design target. There is no per-leg credential, so nothing
asserts that two subjects get different rows, and nothing can until one exists.

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
an attacker can make the agent emit is a different certified question over the same pinned
definitions - that much is enforced today by the shape of `Query`. The blast radius of a fully
manipulated agent is the set of questions its caller could already ask.

The clause "asked as the same caller, against the same authorization" is **half built.** The credential
broker exists and `Warehouse::execute` has no signature that runs without what it minted, so there is
no code path a question reaches a data system through as an unnamed identity. What is still absent is
an adapter that can carry a per-subject credential, so nothing today makes a statement about **whose
rows** come back: the bound on a manipulated agent is the tool surface plus, where a deployment
declares `security.inbound`, the scopes that caller was granted - which decide which operations it may
invoke and not which rows an answer contains.

## Why do the musl builds swap the allocator?

musl's `mallocng` serialises the whole process on one lock word. With one binary and only
threading toggled, a 48-core run takes 4.45s on glibc and 92.16s on musl, slower than musl's own
single-core run; linking mimalloc brings it to 3.83s.
[What ships](architecture.md#what-ships) has the details, including the cost of `MI_SECURE=4`.

## Can I use it today?

For single-player work over local files, yes. `sutura compile` renders the statement for a question
and `sutura query` answers it, over a catalogue of documents in git and the CSV or Parquet files in
a directory you name. That is the honest description: **a governed single-player semantic compiler
and executor over local files**, served either from the command line or
[over HTTP](serving.md).

Not yet as the identity-aware runtime this site describes, and the gap is narrower and more specific
than it used to be. There **is** a request context, a credential broker port with a static-credential
implementor, an audit sink, an MCP surface, and - where a deployment declares `security.inbound` - a
verified caller identity from a signature, with OAuth scopes deciding which operations that caller may
invoke. What there is **not** is leg 2: no adapter in this build has anywhere for a per-subject
credential to arrive, both declare so, and the broker that ships mints what an operator configured. So
a deployment can know exactly who is asking, record it, refuse a subject it holds no credential for -
and still read every row as one identity. There is no Arrow result envelope. The one data system the
shipped binary opens is the in-process engine over those files - `sutura-exec-duckdb` renders and
pushes down, and is a development dependency rather than something the binary links. So the governance
that decides **which rows** is still the narrow tool surface and the pinned bundle, not identity.
[What exists today](architecture.md#what-exists-today) is the inventory.

The environment, the gates and the release pipeline do work, because a mechanism is cheaper to build
before there is code to retrofit it onto.

## How do I work on it?

[Contributing](contributing.md) covers the three routes to an environment, the two toolchains and
which gates run when. On a network with no direct egress, read
[Building without direct egress](enterprise-mirrors.md) first.
