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

These are adapters. The core is the plan: a projection, a `GROUP BY`, a bounded date predicate,
parameterized values and quoted identifiers, wrapped around a pinned statement that is spliced
in without being parsed. What differs per data system is dialect, connection and how an
identity is presented. What a metric means does not differ.

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
over the same pinned definitions, against the same authorization. There is no field in which
"and also read the payroll table" can be written. The blast radius of a fully manipulated agent
is the set of questions its caller could already ask.

**Returned content is untrusted input.** Rows, column descriptions and glossary text are all
authored by somebody else, and any of it can contain something shaped like an instruction. A
delimiter does not separate instruction from data, because the content can contain the
delimiter; nor does a prefix, a marker, or a preamble announcing that what follows is
untrusted. So the boundary is a constraint on the wire format rather than a convention in
prose. Results leave as Arrow, in their own frame, with provenance in the schema metadata: a
typed field a caller reads deliberately, never a string concatenated into the channel that
carries instructions. Descriptive text from the catalogue travels the same way. Both wire
envelopes share one encoder, so neither transport can grow a text-blob shortcut on its own.

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
[Getting started](getting-started.md) for the commands.

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
