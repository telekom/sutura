---
title: API reference
description: The public Rust API, generated from rustdoc JSON and committed.
---

# API reference

The public Rust API, one page per library crate, **generated** from rustdoc JSON and committed.
Today that is one crate, because one library crate exists.

- [sutura-domain](sutura-domain.md) - the domain types

## These pages are generated

Do not edit them; every page carries a header saying so. The source of the text is the doc comments
in the crate, and a correction belongs there. Regenerate with:

```bash
just api
```

That runs two steps. `cargo rustdoc --output-format json` is nightly-only, because that is an
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
