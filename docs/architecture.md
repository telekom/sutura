# Architecture

This page describes the shape sutura is heading toward. Read it as the settled design and as
the reason the repository is laid out the way it is, not as an inventory of what compiles.
[What exists today](#what-exists-today) at the end of the page is the inventory, and it is
short.

`AGENTS.md` in the repository root holds the crate table and the invariant table. This page
says why those tables look the way they do.

## Serving is MCP

The primary interface is an MCP server. An agent is a first-class client rather than an
afterthought wrapped around an API built for a dashboard, and the tool surface is the whole of
what an agent is allowed to say.

That makes the tool surface the governance boundary, and it is deliberately narrow. No tool
takes SQL. No tool takes a table name, a filter expression or a list of row ids. A question
names a metric, some dimensions and a time range, and there is no field into which anything
else fits. An uncertified question is unrepresentable rather than refused, and the difference
is not pedantry: a refusal can be retried until it succeeds, and an absent field cannot.

Narrow also has to stay narrow. The tool schemas are derived from the domain types rather than
written by hand, so widening the surface changes a generated schema and shows up as a diff in
the review that widened it.

An HTTP surface sits beside the MCP one for callers that are not agents. It is a second
transport over the same service, derived from the same types, so it cannot accept a question
the MCP surface would reject.

## Metadata sources are behind a port

Metrics, dimensions, the glossary and lineage come from a semantic layer that is not this
repository. `SemanticCatalog` is the port they arrive through: one trait, implemented once per
catalogue. A directory of YAML in git and a metadata catalogue with an HTTP API are two
adapters behind the same trait, and swapping one for the other does not touch the query path.

Two properties of that trait carry weight:

- `load` takes no request context. A catalogue cannot see who is asking, so it cannot return a
  different definition to different callers, and a catalogue somebody has tampered with cannot
  select what executes.
- Definitions arrive as a pinned, hashed snapshot. Arguments are validated against the pinned
  bundle rather than against a live read, so a catalogue edit cannot change what a question
  means between two invocations. It changes the digest, and the digest travels with the answer.

Nothing here edits a definition. Editing one would fork the definition from the number it
certifies, which is the only thing certifying it was worth.

## Data systems are behind a second port

Execution sits behind its own trait, separate from the catalogue one. ClickHouse and Postgres
are the near-term targets. DuckDB covers local development and single-file work, where the data
is a Parquet file and there is no server to authenticate against. The trait is named
`Warehouse`, which is the port's name and not a claim about what sits behind it.

These are adapters. What differs per data system is dialect, connection and how an identity is
presented. Neither the plan nor what a metric means differs;
[the semantic compiler](#the-semantic-compiler) decides both.

A plan resolves to exactly one data system. Federation across two is not a smaller version of
the same problem, it is a second identity to satisfy, and a plan that cannot run as one subject
in both places is refused rather than run partly as somebody else.

## Security is the reason for the shape

The three sections above are not features arranged around a core. They are what falls out of a
single requirement: an agent may be handed a database only if the database can still tell who
is asking.

**The caller's identity reaches the data system.** Not a service account holding the union of
everyone's access. A credential is minted per request for the calling principal, and a request
that cannot be run as the subject comes back as a refusal instead of falling back to the
service's own identity. That fallback is the convenient one, and it silently turns "you may not
see these rows" into "here are the rows". A row-level security policy that holds only for human
callers is decorative.

**Authorization stays in the data system.** sutura keeps no copy of who may see what, because a
copy is something that can disagree with the original. Grants, row-level policies and masking
already exist in ClickHouse and in Postgres, they are administered by the people who own the
data, and they are audited there. A second implementation here would produce a second answer
and no way to tell which one is right. One consequence is worth stating plainly: there is no
result cache. Under row-level security, a cache keyed on the query text is a cross-user leak.

**A narrow tool surface bounds a compromised agent.** For a system whose input is natural
language from wherever the user found it, a manipulated agent is the expected case rather than
the disaster case. The defence is not detecting the manipulation. It is that the most an
attacker can make the agent emit is a different certified question, asked as the same caller,
over the same pinned definitions, against the same authorization. The blast radius of a fully
manipulated agent is the set of questions its caller could already ask.

**Returned content is untrusted input.** Rows, column descriptions and glossary text are all
authored by somebody else, and any of it can contain something shaped like an instruction. A
delimiter does not separate instruction from data, because the content can contain the
delimiter, and neither does a preamble announcing that what follows is untrusted. So the
boundary is a constraint on the wire format rather than a convention in prose. Results leave as
Arrow, in their own frame, with provenance in the schema metadata: a typed field a caller reads
deliberately, never a string concatenated into the channel that carries instructions.
Descriptive text from the catalogue travels the same way. Both wire envelopes share one encoder,
so neither transport can grow a text-blob shortcut on its own.

## The semantic compiler

`sutura-semantic` turns a modelled question into one statement for one data system. It does not
exist yet: the crate is named in the layout table in `AGENTS.md` and nothing compiles it. The
stages below are the design.

A question names a metric, some dimensions, a grain and a bounded time range. Dimension values
are arguments, checked against an allowlist in the pinned bundle. What comes out is a statement
and the values bound to it. Nothing in between is text a caller wrote.

**Resolve.** Every name in the question is looked up in the pinned snapshot. The metric has to
exist, each dimension has to be one that metric declares, the grain has to be one it supports,
the range has to be bounded. The lookup goes to the pinned bundle and never to a live catalogue
read, which is what makes the answer independent of what the catalogue says at the moment of
asking. The output is a set of definitions, or a refusal naming the argument that failed.

**Plan.** The resolved question becomes a plan: which data system owns the metric, the
projection, the grouping keys, the date predicate and its bounds, and which values become bind
parameters. Two things are settled here and nowhere else. The plan names exactly one source, so
a question that would need two identities is refused before anything runs rather than half
executed. And every value from the question becomes a parameter, so no caller-supplied value
reaches the next stage as text. The plan holds no SQL: no quoting, no function names, no dialect.

**Generate.** The plan becomes one statement in one dialect. The dialect decides identifier
quoting, placeholder syntax, date arithmetic and how an aggregate is spelled. This is the only
stage that emits SQL, and what it emits is a wrapper: a projection, a `GROUP BY`, a bounded date
predicate, parameterized values, quoted identifiers. That is all of it.

### The splice

The certified statement is spliced into that wrapper as a derived table, byte for byte, without
being parsed. It is the decision that separates this from a SQL generator.

The statement is authored upstream by the semantic layer that renders it, and it arrives as text
with a digest. sutura puts it in the `FROM ( ... )` position and generates around it. It does not
read it, rewrite it, transpile it, or push a predicate into it.

Parsing it would mean re-emitting it, and re-emitting it substitutes our reading of the statement
for the author's. Every round trip through an AST is a chance to change the number quietly: a
window frame read slightly differently, a null ordering normalized, an implicit cast made
explicit. The digest would not move, because it covers the text we were handed rather than the
SQL we produced, so the change would be invisible in the one place this design exists to make
visible. Byte-for-byte passthrough is what keeps "certified" true after compilation.

The cost is real and worth stating. Nothing is optimized across the splice boundary: a predicate
in the wrapper filters the statement's output, not its input. The statement's column types are
not known without asking the data system. And the statement has to be valid in the dialect it
will run in, because the splice does not translate it, which makes the data system part of what
the definition means rather than a deployment choice.

The paragraph above is not what holds this. SQL goldens are regenerated and reviewed as a
diff rather than typed, and they assert the passthrough byte for byte. An anchor test re-executes
each pinned statement in CI and at startup, and a failure there fails readiness. One gap is open
and recorded in `AGENTS.md`: no lint yet bans a transpile call on the query path, so review is
what catches one until a lint does.

## Where the parts come from

The same line runs through every system of this kind: **semantic layer, plan, federation,
dialect, execution**. The projects worth reading sit at different points on it. The last stage is
the one none of them covers.

```mermaid
flowchart TB
    Q["a modelled question"]
    SL["semantic layer<br>what a metric means<br>Wren"]
    PL["plan<br>one source, grouping,<br>bounds, parameters<br>DataFusion"]
    FD["federation<br>where a subplan runs<br>datafusion-federation, Spice"]
    DI["dialect<br>quoting, placeholders, dates<br>polyglot"]
    EX["execution<br>as the calling principal<br>nothing above does this"]
    ST(["the certified statement,<br>spliced in unparsed"])
    A["Arrow, with provenance"]

    Q --> SL --> PL --> FD --> DI --> EX --> A
    SL -.->|bytes| ST
    ST -.-> DI
```

**The semantic layer decides what a question means.** [Wren](https://github.com/Canner/WrenAI) is
the reference for that shape: a modelling language, an engine that analyses and plans against it,
and MCP as the way an agent asks. Wren models the tables and derives a metric's SQL from the
model. We take the metric's statement as given and refuse to look inside it, which is a narrower
job and a different guarantee. Wren also carries access rules in the model, and that is a policy
copy in the middle tier - the thing
[this design declines to keep](#security-is-the-reason-for-the-shape).

**The plan is where a question stops being text.** [DataFusion](https://datafusion.apache.org/)
is what Wren and Spice both build on: a logical plan representation, an optimizer you extend with
rules, execution over Arrow, and extension points for table providers and functions. We do not
use it. `sutura-domain` names no framework - not tokio, not arrow, not datafusion - and the query
path is not built, so the first plan will be a small type in `sutura-semantic`. DataFusion is the
natural substrate for the stage after that, and what would make it worth adopting is the
optimizer and the federation rule below, not the SQL frontend. Worth noticing: the half of
DataFusion most projects reach for first is the half we would leave switched off, because
accepting SQL is the thing this system refuses to do.

**Federation decides where a subplan runs, and here that is a security question.**
[datafusion-federation](https://github.com/datafusion-contrib/datafusion-federation) registers an
optimizer rule that finds the largest subplan a single remote source can execute and hands it to
that source to run. The property we want is not latency. A predicate pushed into ClickHouse or
Postgres is evaluated by ClickHouse or Postgres, under the caller's own grants, row-level
policies and column masking. Rows excluded there never enter this process, so there is nothing
here to re-authorize, which is how
[authorization stays in the data system](#security-is-the-reason-for-the-shape) once a question
reaches more than one table. Pull the rows up and filter them locally and the filtering is ours,
and a second implementation of a policy is what we said we would not keep. So federation arrives
after per-leg identity rather than before it:
[a plan resolves to one data system](#data-systems-are-behind-a-second-port) today, and a
federated plan whose legs cannot all run as one subject is refused rather than run partly as
somebody else.

**The dialect stage is one plan and many adapters.**
[polyglot](https://github.com/tobilg/polyglot) is a Rust transpiler between more than thirty SQL
dialects, MIT-licensed, with ClickHouse, Postgres and DuckDB all on its list. The wrapper is
exactly its shape of problem: a projection, a `GROUP BY` and a date predicate, rendered per data
system. It must not touch the splice. A transpiler is a parse followed by a re-emit, and that is
the one operation the certified statement is never subjected to, so the boundary is that polyglot
may render what we generated while the pinned bytes pass through untouched. Today that boundary
is a review rule, not a lint.

**Execution is where the caller's identity has to arrive.**
[Spice](https://github.com/spiceai/spiceai) comes closest to everything above it: Rust,
Apache-2.0, DataFusion-based, federating across thirty-odd connectors and accelerating them by
materializing into Arrow, DuckDB, SQLite or Postgres on an interval, a trigger or a change
stream. The federation half is what we want, and better exercised than anything we will write
soon. The acceleration half is what we cannot take: a materialized copy is read under whoever
refreshed it, so under row-level security it is a cross-user leak with a refresh schedule. That
is the same reason there is no result cache. Spice's front door is also SQL, where ours has no
field for it.

| Stage | Decided there | Ours or theirs |
| --- | --- | --- |
| Semantic layer | what a metric means | Neither. Authored upstream, arriving pinned; Wren is the reference shape |
| Plan | source, projection, grouping, bounds, parameters | Build first, adopt later: a type in `sutura-semantic`, DataFusion when a type stops being enough |
| Federation | which subplan its owner runs | Adopt, once a credential exists per leg |
| Dialect | quoting, placeholders, date arithmetic | Adopt for the wrapper, never for the splice |
| Execution | the connection, and which principal the data system sees | Build. One adapter per data system, and the per-request credential is the part nothing above provides |

That last row is why this is a repository rather than a configuration file for one of the others.

## Hexagonal by construction

Ports live in the domain crate. Adapters live outside it. The domain crate depends on neither:
it names what it needs by trait, and the binary decides which implementation is passed in.

```text
        agent                        other callers
          |                                |
      MCP server                     HTTP / OpenAPI          transport, no logic
          +----------------+----------------+
                           |
                      sutura-app                             the service, generic over ports
                           |
                     sutura-domain                           domain types + port traits
          +----------------+----------------+
          |                |                |
   SemanticCatalog     Warehouse     CredentialBroker        ports, inside the hexagon
          |                |                |
    YAML in git,      ClickHouse,      the identity          adapters, outside it
    a metadata        Postgres,          provider
    catalogue         DuckDB
```

`cargo xtask check-boundaries` enforces the direction, so the diagram cannot quietly stop being
true. It fails the build if `sutura-domain` acquires a framework dependency anywhere in its
transitive tree, which catches a framework reached through an innocuous crate as well as one
declared outright. It fails again if a library crate's surface stops being a typed contract: a
`pub` field on a `pub struct`, a `Result` whose error type is `String`, or a declared
dynamic-error crate such as `anyhow`. The rule is worth more as a gate than as a paragraph,
because a paragraph cannot fail a pull request.

Two consequences follow from the direction rather than from taste. The domain names no
framework, so its test suite compiles nothing heavy and runs in well under a second, which is
what makes it the inner loop. And adapters are feature-gated and default-off, which is why
every lint and test entry point passes `--all-features`; see
[Contributing](contributing.md) for the commands.

## What ships

Four artifacts, two libc flavours on two architectures, each a distroless image holding one
binary. `cargo xtask check-workflows` and the `one-binary` check together keep that true: the
first fails if a workflow names a build output that does not exist, the second fails if an image
carries more than the binary.

The musl artifacts replace the system allocator, and that is not a performance nicety. musl's
mallocng serialises the whole process on one lock word: `src/malloc/mallocng/glue.h` defines
`rdlock` and `wrlock` as the same exclusive lock and `upgradelock` as a no-op, and
`struct malloc_context` is a single global with no arenas and no per-thread cache. Measured with
one binary and only threading toggled, the single-threaded control is a wash while a 48-core run
goes from 4.45s on glibc to 92.16s on musl - slower than musl's own single-core run, which is
what one global mutex predicts. Linking mimalloc in brings the same run to 3.83s, ahead of
glibc. Expect roughly parity single-threaded and a 4-20x gap for a threaded application.

mimalloc is built with `MI_SECURE=4`: guard pages, randomised placement, encoded free lists,
double-free detection. That costs 23-43% against plain mimalloc, which is far more than
upstream's README claims, and it is still the right trade here - mimalloc-secure remains well
ahead of glibc, and mallocng is behind it. The hardening is paid out of a margin this project
would not otherwise have had, and the latency budget is a warehouse round trip.

## What exists today

Four packages: `sutura-domain` (domain types), `sutura-cli` (the binary), `xtask` (the repo
gates, listed by `cargo xtask --help`) and `sutura-dev` (a local development CLI). Everything
else named on this page is design. The query path is not built.

`sutura-domain` holds no port traits yet, and that is deliberate. A port exists to invert a
dependency on something outside the hexagon, and none of the adapters exists yet to invert. A
trait with no implementor and no caller is a guess at a signature that only the first real
adapter can settle, and in a library crate `pub` hides it from `dead_code`, which is how an
unused item survives review. Each port arrives with the adapter beneath it, and each crate
arrives with the milestone that needs it. An empty crate is a compile target and a maintenance
surface returning nothing.

What does work is the environment, the gates and the release pipeline. The order is deliberate:
every claim on this page is meant to be held up by a mechanism rather than by intent, and a
mechanism is cheaper to build before there is code to retrofit it onto.
