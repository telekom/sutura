# sutura

sutura answers questions about data **as the person or agent asking**, using metric
definitions somebody certified - and refuses when it cannot do either.

Give an agent a database connection and it will answer with SQL it invented, run under
whatever credential the service happens to hold. Both halves are wrong. The number is
uncertified, so nobody can say whether "revenue" means what finance means by it. And the
rows come back according to what the *service* may read, not what the *caller* may read,
which is how a row-level security policy becomes decorative.

## What is different about it

| Property | What it means |
| --- | --- |
| Every query runs as the caller | No service account holding the union of everyone's access. A leg that cannot run as the subject is refused, never downgraded |
| A refusal is an answer | It comes back as a result with a reason, so a caller cannot mistake it for a hiccup and retry until something works |
| You cannot ask it to run SQL | There is no field for a query, a table or a filter. An uncertified question is unsayable, not merely refused |
| Definitions come from elsewhere | Authored in a semantic layer, pinned and hashed. Nothing here edits one, because that would separate the definition from the number it certifies |

Each of those is held up by a type, a lint or a gate rather than by intent. The full list,
with the mechanism that carries each one, is in `AGENTS.md` at the root of the repository.

## What it borrows

sutura is not a new idea about semantic layers. Two projects got there first and are worth
reading:

- **[Wren](https://github.com/Canner/WrenAI)** - a semantic engine over
  [DataFusion](https://datafusion.apache.org/) that compiles a modelled question into SQL. The
  compile-a-plan-from-a-model shape comes from here.
- **[Spice](https://github.com/spiceai/spiceai)** - also DataFusion-based, for federating and
  accelerating queries across sources.

Both are Apache-2.0 and both solve the semantic half well. What sutura adds is the other half:
**identity**. Wren and Spice answer "what does this question mean"; sutura also has to answer
"who is asking, and may they see the answer" - and refuse when it cannot. Every query executes
as the caller rather than as the service, which is the requirement that makes chatting with
data safe to expose to an agent at all.

The other deliberate choice is **pluggability on both sides**: the semantic layer and the data
system are ports, not assumptions. Definitions can come from one catalogue and rows from
another warehouse, and swapping either is an adapter rather than a rewrite.

## Status

Early, and worth being precise about: the design is settled and the code is a walking
skeleton. What works today is the environment, the release pipeline, and the checks that
keep the guarantees above from quietly becoming aspirations. The query path is not built
yet - see [Architecture](architecture.md) for what exists on disk versus what is planned.

## Where to start

| You want to | Read |
| --- | --- |
| Set up a machine and run the checks | [Getting started](getting-started.md) |
| Build behind a proxy or with no direct egress | [Enterprise mirrors](enterprise-mirrors.md) |
| Understand the crate layout | [Architecture](architecture.md) |
| Know what is guaranteed, and by what | [Invariants](invariants.md) |
| Know what CI will reject | [The gates](gates.md) |
