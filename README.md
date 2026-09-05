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

(Italian) sutura is a surgical stitching technique to bring wound edges together.

sutura:
- is an MCP server
- to answer questions about data **as the person or agent asking** using metadata; strong metadata like blessed metrics are preferred.
- can connect systems across boundaries.
- has a strong focus on security and E2E impersonation
- clear refusal results based on the metadata


## Status

Very much WIP; basic skeleton and CI pipelines there.

There are 2 flavours of sutura:
- single player (shared service user)
- multiplayer (full E2E impersonation)

Per connection the mode can be configured.

## Vision

- Have pluggable metadata source/s
- Have pluggable data source/s
- Have a focus on security (impersonation, prompt injection, semantic & rust compiler verification)
- Support query federation (at least lightly)

We are standing on the shoulders of giants/ecosystems/role models but sometimes just taking inspiration and combining:

- https://datafusion.apache.org/index.html
- https://github.com/Canner/WrenAI
- https://github.com/spiceai/spiceai

Building around arrow & arrow flight.


## Documentation

<https://telekom.github.io/sutura/>, built from `docs/` in this repository.

## Licence

Apache-2.0. Third-party material adapted here is recorded in [VENDOR.md](VENDOR.md).
