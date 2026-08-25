# Questions and answers

Questions with answers that already exist. Where a page says it better, this one links rather than
restates.

## Why not just give the agent a database connection and let it write SQL?

Because two things go wrong at once, and only one of them is obvious.

The number is uncertified. An agent that invents SQL invents a definition with it, so nobody can
say whether "revenue" means what finance means by it. And the rows come back according to what the
*service* may read rather than what the *caller* may read, which is how a row-level security
policy becomes decorative. sutura fixes both by construction: definitions arrive certified and
pinned, and every query runs as the caller.

## Why is a refusal not an error?

An error invites a retry. A refusal is a variant of the result type with a typed reason, so a
caller cannot mistake "you may not ask this" for a transport hiccup and loop until something
answers. Every refusal is recorded with the whole principal chain, which makes a refused call as
attributable as an answered one.

## Why does the tool surface take no table name?

Because refusing a question is weaker than not being able to express it. A refusal can be retried,
reworded and eventually satisfied; an absent field cannot. `Query` carries no SQL, no table, no
filter expression and no row ids, so an uncertified question does not compile rather than being
rejected at runtime. Widening that surface changes a generated schema, which puts the widening in
the diff of the review that did it.

## Why is there no result cache?

Under row-level security, two callers asking the same question are entitled to different rows. A
cache keyed on the query text serves the first caller's rows to the second, which is a cross-user
leak with a hit rate. The same reasoning rules out a materialised copy refreshed on a schedule: it
is read under whoever refreshed it.

No mechanism can prove an absence, so this one is written down as a decision. Adding any cache of
rows is an architecture change, keyed on subject first or not at all.

## Why can a question not span two data systems?

A second data system is a second identity to satisfy, not a bigger version of the same query. A
plan whose legs cannot all run as one subject is refused rather than run partly as somebody else.
Today a plan resolves to exactly one source and a test asserts it.

Federation is wanted, and the order matters: per-leg identity first, then federation. Pushing a
predicate into ClickHouse or Postgres means that system evaluates it under the caller's own grants,
so rows excluded there never enter this process.
[Where the parts come from](architecture.md#where-the-parts-come-from) sets out which projects
already do that part well.

## Why are definitions not editable here?

Editing a certified statement forks the definition from the number it certifies, which is the only
thing certifying it was worth. Definitions are authored in the semantic layer that renders them and
arrive pinned and hashed. If a definition is wrong, it is wrong upstream.

## What happens if the agent asking is manipulated?

For a system whose input is natural language from wherever the user found it, a manipulated agent
is the expected case rather than the disaster case. The defence is not detecting it. The most an
attacker can make the agent emit is a different certified question, asked as the same caller, over
the same pinned definitions, against the same authorization. The blast radius of a fully
manipulated agent is the set of questions its caller could already ask.

## Why do the musl builds swap the allocator?

Because musl's `mallocng` serialises the whole process on one lock word, and that is measurable
rather than theoretical: with one binary and only threading toggled, a 48-core run goes from 4.45s
on glibc to 92.16s on musl, which is slower than musl's own single-core run. Linking mimalloc
brings the same run to 3.83s. [What ships](architecture.md#what-ships) has the details, including
why mimalloc is built with `MI_SECURE=4` despite the cost.

## Can I use it today?

No. Four packages exist: the domain types, the binary, the repository gates and a local development
CLI. The query path is not built, there is no MCP server and there is no adapter for any data
system. [What exists today](architecture.md#what-exists-today) is the honest inventory, and
[Getting started](getting-started.md) is a placeholder until there is something to start.

What does work is the environment, the gates and the release pipeline, which is deliberate: a
mechanism is cheaper to build before there is code to retrofit it onto.

## How do I work on it?

[Contributing](contributing.md) covers the three routes to an environment, the two toolchains and
which gates run when. On a network with no direct egress, read
[Building without direct internet egress](enterprise-mirrors.md) first.
