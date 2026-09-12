---
title: API reference
description: The public Rust API, generated from rustdoc JSON and committed.
---

# API reference

The public Rust API, one page per library crate, **generated** from rustdoc JSON and committed. The
list is derived from `cargo metadata`, so a new library crate gets a page without anybody adding it
here - and `cargo xtask check-api-docs` fails until that page is generated and committed.

The order below is the order a question travels in, which is also the dependency direction: every
arrow points inward, at the domain.

- [sutura-domain](sutura-domain.md) - the domain types and the port traits. Depends on nothing but
  `serde` and `thiserror`, and a gate keeps it that way
- [sutura-catalog-local](sutura-catalog-local.md) - a `SemanticCatalog` adapter over a directory of
  markdown documents with YAML frontmatter
- [sutura-semantic](sutura-semantic.md) - the compiler: resolve and plan. It renders nothing, and has
  no SQL generator in its dependency tree
- [sutura-sql](sutura-sql.md) - rendering: a plan becomes one statement in one dialect. Depended on
  by the SQL adapters and by the CLI, and deliberately not by the compiler
- [sutura-exec-datafusion](sutura-exec-datafusion.md) - THE engine. A `Warehouse` adapter that
  executes a plan over Arrow and generates no SQL
- [sutura-exec-duckdb](sutura-exec-duckdb.md) - a `Warehouse` adapter over DuckDB as a DATA SOURCE:
  it renders the plan into DuckDB SQL and pushes it down. A development dependency, not shipped
- [sutura-app](sutura-app.md) - the service, generic over the ports. Also the `Surface` driving port
  and its one implementor, so a transport implements nothing another transport owns
- [sutura-config](sutura-config.md) - the settings tree and the startup refusals. Holds no
  framework: it decides what the service may do before anything is built
- [sutura-runtime](sutura-runtime.md) - the process-global concerns a library must not install as a
  side effect of being linked: the subscriber, the panic hook, the shutdown signal
- [sutura-http](sutura-http.md) - transport only. It consumes the `Surface` port rather than
  declaring it, so a second transport reaches for `sutura-app` and never for this crate
- [sutura-conformance](sutura-conformance.md) - the conformance packs: test bodies written once
  against the ports, bound to an adapter by a macro. It points inward like everything else - at the
  domain and at no adapter, which is what lets an adapter's own crate take it as a dev-dependency

## These pages are generated

Do not edit them; every page carries a header saying so. The source of the text is the doc comments
in the crate, and a correction belongs there. Regenerate with:

```bash
just api
```

That runs two steps. One `cargo doc` over the whole workspace, with rustdoc's
`--output-format json`, which is nightly-only because that is an
unstable rustdoc option - not a new pin, but the nightly this repository already keeps for the
cranelift backend (`devco/rust-toolchain-nightly.toml`). `docs/.tools/rustdoc_to_markdown.py` then
reads the JSON and writes markdown, using the pixi interpreter and the standard library only.

rustdoc JSON carries a `format_version` that changes between nightlies. The generator refuses to run
on a number it was not written for, printing the value it found against the value it expected. A
generator that guessed would one day emit a page with the interesting parts silently missing.

`cargo xtask check-api-docs` regenerates and byte-compares, so a doc comment that changed without a
regeneration fails the gate instead of publishing a stale page as current. CI runs it as the
`api-docs` flake check, its only nightly step, and it is self-scoping: a change touching only
private code regenerates to identical output and passes.

## Why markdown rather than rustdoc's own site

`cargo doc` produces a complete static site with its own search index. That index is rustdoc's, not
this site's, and the two cannot be merged, because mkdocs indexes markdown at build time.
Publishing the HTML would give a reader two search boxes and no way to know which one to use.

Generating markdown puts the API in the **same** search as the prose on every other page. It is the
shape `mkdocstrings` uses for Python: read structured data, emit pages, rather than scrape a
rendered site.

The trade is depth. These pages carry each public item, its signature, its documentation, its
methods and the traits it implements. They do not carry rustdoc's source links, its type-directed
navigation or its "implementors" listings. For those, run `cargo doc --workspace --no-deps` and open
what it writes under `target/doc`.
