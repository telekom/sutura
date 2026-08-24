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

Each of those is held up by a type, a lint or a gate rather than by intent. [Invariants](invariants.md)
lists which mechanism carries which guarantee.

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
