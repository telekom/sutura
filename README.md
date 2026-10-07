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

sutura is a semantic data runtime written in Rust. It serves queries over HTTP and MCP.
Queries use certified metric definitions. Metadata and data sources connect through adapters.
A query can combine data from several sources.

Deployments can verify caller tokens. Each data source declares the identity it uses.
See [identity verification](docs/where-identity-is-proven.md) for the tested paths and their limits.

## Technology

- [Apache DataFusion](https://datafusion.apache.org/) executes local queries.
- [Apache Arrow](https://arrow.apache.org/) carries result batches inside the runtime.
- [ADBC](https://arrow.apache.org/adbc/) connects to data systems.
- [polyglot-sql](https://github.com/tobilg/polyglot) renders SQL for each dialect.
- [Wren](https://github.com/Canner/WrenAI) and [Spice](https://github.com/spiceai/spiceai)
  provide design references for semantic models and query federation.

See [architecture](docs/architecture.md) and [integrations](docs/integrations.md) for details.

## Documentation and examples

- [Documentation](https://telekom.github.io/sutura/latest/)
- [Getting started](docs/getting-started.md)
- [Examples](examples/README.md)
- [Contributing](CONTRIBUTING.md)

## Licence

Apache-2.0. See [VENDOR.md](VENDOR.md) for third-party material.
Report suspected vulnerabilities through [SECURITY.md](SECURITY.md).
