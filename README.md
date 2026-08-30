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

sutura is being built to answer questions about data **as the person or agent asking**, using
metric definitions somebody certified, and to refuse when it cannot do either.

Give an agent a database connection and it answers with SQL it invented, run under whatever
credential the service holds. Two failures, not one. The number is uncertified, so nobody can
say whether "revenue" means what finance means by it. And the rows are the ones the *service*
may read rather than the ones the *caller* may read, which is how a row-level security policy
becomes decorative.

**What is here today is the first half: a governed single-player semantic compiler and executor
over local files.** The identity half is designed and not built - there is no request context, no
credential broker, no audit sink and no Arrow result envelope - so read the diagram below as the
target and [Status](#status) as the inventory. Each claim under *What is different* says which it is.

sutura sits in between (the target shape; the middle branch and the Arrow envelope are unbuilt):

```
   agent ──"what was MRR in June?"──►  sutura
                                         │
                    ┌────────────────────┼────────────────────┐
                    │                    │                    │
             pinned definition    caller's identity      one source
             (certified, hashed)   (token exchange)      per plan
                  BUILT              NOT BUILT            BUILT
                    │                    │                    │
                    └────────────────────┼────────────────────┘
                                         ▼
                                    warehouse
                              (rows the CALLER may see)
                                         │
             Arrow + provenance ◄────────┘
                 NOT BUILT
```

## What is different

**Every query runs as the caller. Design target, not built.** Not as a service account holding
the union of everyone's access. A query that cannot run as the subject comes back as a refusal,
never downgraded to sutura's own identity: that downgrade turns "you may not see these rows" into
"here are the rows". None of that mechanism is in the workspace: no request context, no credential
broker, so no caller identity reaches the query path at all, and no downgrade path only because
there is no second identity to downgrade to. Over a local file the property is trivially true and
buys nothing, because a file has no login.

**A refusal is an answer, not a failure. Enforced today.** It is a variant of the result type
carrying a reason, so a caller cannot mistake it for a hiccup and retry until something works, and
the golden suite provokes every variant a question can reach. Recording a refusal against the
principal chain is a design target: there is no audit sink.

**You cannot ask it to run SQL. Enforced today.** There is no field for a query, a table or a
filter. An uncertified question is unsayable rather than refused, and a question carrying `sql:` is
an error naming the field rather than one silently dropped. Every value a question does carry binds
as a parameter, asserted over the whole corpus by a golden. The most a manipulated agent can do is
ask a different certified question - though "as the same caller" is the design target above, not
something enforced here.

**Definitions come from elsewhere. Enforced today.** They are authored in a semantic layer and
arrive pinned and hashed. Nothing here edits one; editing forks the definition from the number it
certifies. The load path takes no request context, and every declared anchor re-executes before the
bundle may be served.

## What it borrows

Two Apache-2.0 projects got there first. **[Wren](https://github.com/Canner/WrenAI)** compiles
a modelled question into SQL over [DataFusion](https://datafusion.apache.org/).
**[Spice](https://github.com/spiceai/spiceai)**, also DataFusion-based, federates and
accelerates across sources. Neither answers who is asking or whether they may see the answer,
and that is what sutura means to add - the part that is designed and not yet built.

One plan has to render as valid SQL in every dialect an adapter targets.
**[polyglot](https://github.com/tobilg/polyglot)** is a Rust transpiler between more than
thirty of them, ClickHouse, Postgres and DuckDB included.

## Status

The query path is built for the single-player case, and the supported combination is exactly this:
**metadata from a catalogue of markdown documents with YAML frontmatter in git, and execution by the
in-process engine over the CSV or Parquet files you point it at.** A question naming a metric, a grain
and a bounded range compiles to a plan; every metric that declares a certified number re-executes and
reproduces it before the bundle can be served.

```
sutura query <catalog-dir> <question.yaml> <data-dir>
```

Two things read as more than they are. The plan can be **rendered** as SQL for DuckDB, Postgres or
ClickHouse - `sutura compile` does that and the goldens parse-check each one - but rendering a dialect
is not a data system, and there is no Postgres or ClickHouse adapter. And **DuckDB is a test
dependency, not the runtime data source**: the adapter that pushes SQL down to it is exercised by the
golden suite and by a differential test that runs one plan both ways and compares the rows, but the
binary does not link it and you need no `libduckdb` to run the command above. `sutura doctor` prints
`data systems : none - this build reads files, and pushes down to nothing`.

Which adapter is used is decided at compile time. There is no configuration that selects a metadata
provider or a data system yet, which is why "pluggable" describes the ports and not an operator's
options. [What can be plugged in today](https://telekom.github.io/sutura/latest/architecture/#what-can-be-plugged-in-today-and-what-the-shipped-binary-actually-uses)
is the table.

The part that makes the first line of this README true of a *warehouse* is still not built, and it is
now one thing rather than several. A request context, a credential broker port and an audit sink all
exist: a question cannot execute without a credential minted for the source it reads, and every
outcome is recorded before it is returned. What is absent is **a data system that evaluates the asking
subject** - no adapter in this build can carry a per-subject credential, so "as the person or agent
asking" holds here only because a file has nobody else to be. Arrow results with provenance and
federation are ahead of it too. The HTTP surface is not: it
ships, and its bearer token authenticates the *deployment* rather than the caller - so it mounts
the tool surface over a network without making any of the per-caller claims above true.
See [what exists today](https://telekom.github.io/sutura/latest/architecture/#what-exists-today).

## Documentation

<https://telekom.github.io/sutura/>, built from `docs/` in this repository.

## Licence

Apache-2.0. Third-party material adapted here is recorded in [VENDOR.md](VENDOR.md).
