<p align="center">
  <img src="docs/assets/sutura.svg" alt="" width="78" height="90">
</p>

<h1 align="center">sutura</h1>

<p align="center">
  <a href="https://github.com/telekom/sutura/actions/workflows/ci.yml"><img src="https://github.com/telekom/sutura/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/licence-Apache--2.0-blue.svg" alt="Apache-2.0"></a>
  <img src="https://img.shields.io/badge/rust-2024-orange.svg" alt="Rust 2024">
  <a href="https://zizmor.sh"><img src="https://img.shields.io/badge/workflows-zizmor-brightgreen.svg" alt="zizmor"></a>
</p>

sutura answers questions about data **as the person or agent asking**, using metric
definitions somebody certified, and refuses when it cannot do either.

Give an agent a database connection and it will answer with SQL it invented, run under
whatever credential the service happens to hold. Both halves are wrong. The number is
uncertified, so nobody can say whether "revenue" means what finance means by it. And the rows
come back according to what the *service* may read, not what the *caller* may read, which is
how a row-level security policy becomes decorative.

sutura sits in between:

```
   agent ──"what was MRR in June?"──►  sutura
                                         │
                    ┌────────────────────┼────────────────────┐
                    │                    │                    │
             pinned definition    caller's identity      one source
             (certified, hashed)   (token exchange)      per plan
                    │                    │                    │
                    └────────────────────┼────────────────────┘
                                         ▼
                                    warehouse
                              (rows the CALLER may see)
                                         │
             Arrow + provenance ◄────────┘
```

## What is different about it

**Every query runs as the caller.** Not as a service account holding the union of everyone's
access. When a query cannot be run as the subject, sutura returns a refusal; it does not fall
back to its own identity. That fallback is convenient and it silently turns "you may not see
these rows" into "here are the rows".

**A refusal is an answer, not a failure.** It comes back as a result with a reason, so a
caller cannot mistake it for a hiccup and retry until something works.

**You cannot ask it to run SQL.** There is no field for a query, a table, or a filter. An
uncertified question is not refused so much as unsayable. The most a manipulated agent can do
is ask a different certified question, as the same caller.

**Definitions come from somewhere else.** They are authored in a semantic layer, pinned and
hashed. Nothing here edits one, because editing it would separate the definition from the
number it certifies.

## What it borrows

Two projects got to the semantic half first, both Apache-2.0 and both worth reading:
**[Wren](https://github.com/Canner/WrenAI)**, which compiles a modelled question into SQL over
[DataFusion](https://datafusion.apache.org/), and **[Spice](https://github.com/spiceai/spiceai)**,
also DataFusion-based, for federating and accelerating across sources.

What sutura adds is identity. They answer "what does this question mean"; sutura also answers
"who is asking, and may they see the answer". Both the semantic layer and the data system sit
behind ports, so swapping either is an adapter rather than a rewrite.

One adapter per data system means the same plan has to come out as valid SQL in more than
one dialect. **[polyglot](https://github.com/tobilg/polyglot)** is that problem on its own:
a Rust transpiler between more than thirty SQL dialects, ClickHouse, Postgres and DuckDB
among them.

## Status

The design is settled and the code is a walking skeleton. What works today is the environment,
the release pipeline, and the gates that keep the guarantees above from quietly becoming
aspirations. The query path is not built yet.

## Documentation

The [documentation](https://telekom.github.io/sutura/) covers how to get set up, what the
pieces are, and why they are that way. It is built from `docs/` in this repository.

## Licence

Apache-2.0. Third-party material adapted here is recorded in [VENDOR.md](VENDOR.md).
