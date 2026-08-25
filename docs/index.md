<p class="sutura-mark" style="text-align: center; margin: 0 0 1.5rem;">
  <img src="assets/sutura.svg" alt="" width="78" height="90">
</p>

# sutura

sutura answers questions about data **as the person or agent asking**, using metric
definitions somebody certified, and refuses when it cannot do either.

Give an agent a database connection and it will answer with SQL it invented, run under
whatever credential the service happens to hold. Both halves are wrong. The number is
uncertified, so nobody can say whether "revenue" means what finance means by it. And the
rows come back according to what the *service* may read, not what the *caller* may read,
which is how a row-level security policy becomes decorative.

## The four properties

| Property | The mechanism it rests on |
| --- | --- |
| Every query runs as the caller | A credential minted per request for the calling principal. A leg that cannot run as the subject is refused, never downgraded to a service identity |
| A refusal is an answer | Refusal is a variant of the result type rather than an error return, so a caller cannot mistake it for a hiccup and retry until something works |
| You cannot ask it to run SQL | The tool surface has no field for a query, a table or a filter. An uncertified question is unrepresentable, not merely refused |
| Definitions come from elsewhere | They are authored in a semantic layer and arrive pinned and hashed. Nothing here edits one, because that would fork the definition from the number it certifies |

Those mechanisms are the design. [Architecture](architecture.md) says how the four force the
shape of the system and which parts are compiled today; `AGENTS.md` in the repository lists
every invariant beside the type, lint or gate that enforces it.

## What it borrows

sutura is not a new idea about semantic layers. Two projects got there first, both Apache-2.0
and both worth reading:

- **[Wren](https://github.com/Canner/WrenAI)** compiles a modelled question into SQL over
  [DataFusion](https://datafusion.apache.org/). The compile-a-plan-from-a-model shape comes
  from there.
- **[Spice](https://github.com/spiceai/spiceai)**, also DataFusion-based, federates and
  accelerates queries across sources.

Between them they cover compiling a question and federating it. What sutura adds is identity:
not only what a question means, but who is asking and whether they may see the answer. Every
query executes as the caller rather than as the service, which is the requirement that makes
chatting with data safe to expose to an agent at all.

A third project sits one port down. Every data system adapter has to emit the same plan as
valid SQL in its own dialect, and **[polyglot](https://github.com/tobilg/polyglot)** is a
Rust transpiler between more than thirty of them, ClickHouse, Postgres and DuckDB included.

[Where the parts come from](architecture.md#where-the-parts-come-from) sets each of them on the
line from a modelled question to executed SQL, and says which parts we mean to build.

## Status

The design is settled and the code is a walking skeleton. What works today is the environment,
the release pipeline, and the gates that keep the properties above from quietly becoming
aspirations. The query path is not built yet.

## Where to start

| You want to | Read |
| --- | --- |
| Know what the words on the tool surface mean | [Concepts](concepts.md) |
| Ask the short questions first | [Questions and answers](qa.md) |
| Understand the shape of the system | [Architecture](architecture.md) |
| Set a machine up and run the checks | [Contributing](contributing.md) |
| Build behind a proxy or with no direct egress | [Building without direct egress](enterprise-mirrors.md) |
| Know how the versioned site is published | [Publishing the docs](publishing.md) |
| See what changed | [Changelog](changelog.md) |
| Know what is guaranteed, and by what | `AGENTS.md` in the repository |
