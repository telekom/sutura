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

Give an agent a database connection and it answers with SQL it invented, run under whatever
credential the service holds. Two failures, not one. The number is uncertified, so nobody can
say whether "revenue" means what finance means by it. And the rows are the ones the *service*
may read rather than the ones the *caller* may read, which is how a row-level security policy
becomes decorative.

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

## What is different

**Every query runs as the caller.** Not as a service account holding the union of everyone's
access. A query that cannot run as the subject comes back as a refusal, never downgraded to
sutura's own identity: that downgrade turns "you may not see these rows" into "here are the
rows".

**A refusal is an answer, not a failure.** It is a variant of the result type carrying a
reason, so a caller cannot mistake it for a hiccup and retry until something works.

**You cannot ask it to run SQL.** There is no field for a query, a table or a filter. An
uncertified question is unsayable rather than refused. The most a manipulated agent can do is
ask a different certified question, as the same caller.

**Definitions come from elsewhere.** They are authored in a semantic layer and arrive pinned
and hashed. Nothing here edits one; editing forks the definition from the number it certifies.

## What it borrows

Two Apache-2.0 projects got there first. **[Wren](https://github.com/Canner/WrenAI)** compiles
a modelled question into SQL over [DataFusion](https://datafusion.apache.org/).
**[Spice](https://github.com/spiceai/spiceai)**, also DataFusion-based, federates and
accelerates across sources. Neither answers who is asking or whether they may see the answer,
and that is what sutura adds.

One plan has to render as valid SQL in every dialect an adapter targets.
**[polyglot](https://github.com/tobilg/polyglot)** is a Rust transpiler between more than
thirty of them, ClickHouse, Postgres and DuckDB included.

## Status

The query path is built for the single-player case: a catalogue of markdown documents in git, a
question naming a metric and a bounded range, one statement generated for DuckDB, Postgres or
ClickHouse, and execution against a DuckDB file. Every metric that declares a certified number
re-executes and reproduces it before the bundle can be served.

The part that makes the first line of this README true of a *warehouse* is not built. There is no
credential broker yet, so "as the person or agent asking" holds here only because a file has nobody
else to be. The MCP and HTTP surfaces, Arrow results with provenance, and federation are ahead of it.
See [what exists today](https://telekom.github.io/sutura/latest/architecture/#what-exists-today).

## Documentation

<https://telekom.github.io/sutura/>, built from `docs/` in this repository.

## Licence

Apache-2.0. Third-party material adapted here is recorded in [VENDOR.md](VENDOR.md).
