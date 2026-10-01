<p align="center">
  <img src="docs/assets/sutura.svg" alt="" width="78" height="90">
</p>

<h1 align="center">sutura</h1>

<p align="center">
  <a href="https://github.com/telekom/sutura/actions/workflows/ci.yml"><img src="https://github.com/telekom/sutura/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/licence-Apache--2.0-blue.svg" alt="Apache-2.0"></a>
  <img src="https://img.shields.io/badge/rust-2024-orange.svg" alt="Rust 2024">
  <a href="https://zizmor.sh"><img src="https://img.shields.io/badge/workflows-zizmor-brightgreen.svg" alt="zizmor"></a>
  <a href="https://api.reuse.software/info/github.com/telekom/sutura"><img src="https://api.reuse.software/badge/github.com/telekom/sutura" alt="REUSE status"></a>
  <a href="https://scorecard.dev/viewer/?uri=github.com/telekom/sutura"><img src="https://api.scorecard.dev/projects/github.com/telekom/sutura/badge" alt="OpenSSF Scorecard"></a>
</p>

sutura is an identity-aware, semantics-first data runtime. It answers questions about data **as the
person or agent asking**, from metric definitions somebody certified, and refuses when it cannot do
either. The [introduction](docs/index.md) states the design up front; this page is what is built
today, and it is deliberately shorter than the vision.

## What it is

- A **governed question surface**, over HTTP (`sutura serve`) and MCP. A question names a metric,
  a grain, a bounded range and some dimensions - there is no field for SQL, a table or a row id, so
  an uncertified question is unrepresentable rather than merely refused. [Serving](docs/serving.md)
  is the configuration reference.
- An **engine**: a compiled plan executed locally over [DataFusion](https://datafusion.apache.org/),
  with `polyglot-sql` rendering SQL where a query is pushed down to a data system.
- **Pluggable metadata and data sources** behind ports. Definitions arrive as a pinned, hashed
  snapshot; a catalog cannot see who is asking, and a tampered bundle changes the digest that
  travels with the answer.
- **Light federation**: a plan can read more than one source, each leg carrying its own credential
  and reported identity posture. [How a join key is compared](docs/how-a-join-key-is-compared.md)
  records what the combiner refuses.

## What it is built on

Read [Architecture](docs/architecture.md) for the settled design and which parts are compiled today;
[What exists today](docs/architecture.md#what-exists-today) is the honest inventory. A question
becomes one statement in three stages - the semantic model is certified, the compiler plans it, and
an adapter renders it per dialect. [Concepts](docs/concepts.md) defines the tool-surface words, and
[Questions and answers](docs/qa.md) the short answers.

## What it is NOT

Saying what is absent is as important as what is here:

- **Not a general SQL ORM handed to an agent.** The tool surface is deliberately narrow, and a query
  that is not certified-defined is refused, not auto-generated.
- **Not an authorization source.** sutura keeps no copy of who may see what; grants, row-level
  policies and masking stay in the data system, where their owners already audit them.
- **Not a proven end-to-end impersonator.** Leg 2 below is **built and unproven** - do not read this
  page as a claim that a served question has executed as the asking subject.
- **No Arrow result envelope.** Results leave the process as rows, not Arrow.

## What is built today

The query path is built and proven for **one combination**: metadata from a catalogue of markdown
documents with YAML frontmatter in git, executed by the in-process engine over the CSV or Parquet
files the deployment points it at. A named metric's declared anchors re-execute before the bundle is
served, and every outcome goes through an audit sink before it is returned.

The served surface is built over that same corpus: `sutura serve` speaks HTTP and MCP, can verify a
caller's own token where the deployment declares `security.inbound`, and composes each source
according to its declared identity posture. Two end-to-end suites hold it - `just serve-e2e` and
`just mcp-e2e` (both run by `just validate`).

### The demo paths

The fastest way to see it is the local chat demo over [`examples/single-player`](examples/), the one
corpus both test suites run on:

```bash
just demo       # a served single-player catalog beside a chat client over HTTP
just demo-mcp   # the same surface over /mcp, verified against a provisioned identity provider
```

`docs/demo.md` is the walkthrough, and `examples/demo-chatinterface/start.sh` is the supervisor.
`just demo-check` validates the configuration without building or contacting a model. These are
**demonstrations, not deployments** - and, below, they are not a venue where identity is proven.

## Identity, stated exactly as it is proven

sutura's central claim is impersonation, so this is stated in the words of
[Where each identity claim is proven](docs/where-identity-is-proven.md), the one authority on what
may be cited where. Two legs, proven to different depths:

- **Leg 1 - knowing who is asking.** **Built.** A deployment that declares `security.inbound`
  verifies a caller's own token; scopes decide which operations that caller may invoke.
- **Leg 2 - a source executing *as* the caller.** **Built and unproven.** The shipped BigQuery
  adapter can carry a verified caller's assertion through its declared per-subject account map - an
  undeclared subject is refused, never run as the deployment - but the venue that would show two
  subjects resolving to two accounts is **wired with no observed run**. No served binary has
  executed as a caller yet. Postgres declares `NoPlaceForASubject`: one connection is one shared
  database role, never an asker's. The served venues are **not built**.

The result for a reader: the demo and the single-player corpus read under one shared source
identity, acknowledged by the operator, so **they prove no caller identity and no source
impersonation**. [Where each identity claim is proven](docs/where-identity-is-proven.md) is the
venue-by-venue table that decides which statement may be cited for which claim, and
[Integrations](docs/integrations.md) lists every adapter's identity posture.

## The examples are authoritative

[`examples/`](examples/README.md) are examples and tests at the same time - a quickstart that stops
working fails the build rather than failing the next person who tried it. `examples/single-player` is
a complete input to the binary; `examples/multi-player/` documents the served per-caller shape no
binary in this repository can open yet; `examples/authored-sql/` is a catalog every published binary
refuses to start on. There is no separate copy of the commands in these pages for CI to run
separately from the ones a reader follows. [Getting started](docs/getting-started.md) walks a no-clone
install and a first certified question.

## Documentation and licence

The full documentation is at <https://telekom.github.io/sutura/>, built from `docs/` in this
repository. For a suspected vulnerability, report it privately rather than in a public issue - see
[SECURITY.md](SECURITY.md).

Apache-2.0. Third-party material adapted here is recorded in [VENDOR.md](VENDOR.md).
