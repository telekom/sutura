# API reference

The public Rust API, one page per library crate, **generated** from rustdoc JSON and committed.
Today that is one crate, because one library crate exists.

- [sutura-domain](sutura-domain.md) - the domain types

## These pages are generated

Do not edit them. Every page carries a header saying so. The source of the text is the doc
comments in the crate, and a correction belongs there.

Two commands produce them, from the repository root and inside the dev shell, where the bare
`cargo` is the pinned nightly:

```bash
cargo rustdoc -q -p sutura-domain --all-features -- -Z unstable-options --output-format json
pixi run --frozen python docs/.tools/rustdoc_to_markdown.py target/doc/sutura_domain.json
```

The first is nightly-only: `--output-format json` is an unstable rustdoc option. That is not a new
pin, it is the nightly this repository already keeps for the cranelift backend
(`rust-toolchain-nightly.toml`). The second reads the JSON and writes markdown, using the pixi
interpreter and the standard library only.

rustdoc JSON carries a `format_version` that changes between nightlies. The generator reads it and
refuses to run on a number it was not written for, printing the value it found against the value it
expected. A generator that guessed would one day emit a page with the interesting parts silently
missing, which is worse than a failed run.

## Why markdown rather than rustdoc's own site

`cargo doc` produces a complete static site with its own search index. That index is rustdoc's, not
this site's, and the two cannot be merged: mkdocs indexes markdown at build time. Publishing the
HTML would give a reader two search boxes and no way to know which one to use.

Generating markdown instead means the API is in the **same** search as the prose on every other
page. It is the shape `mkdocstrings` uses for Python, which reads structured data and emits pages
rather than scraping a rendered site.

The trade is depth. These pages carry each public item, its signature, its documentation, its
methods and the traits it implements. They do not carry rustdoc's source links, its type-directed
navigation or its "implementors" listings. If you need those, run `cargo doc --workspace --no-deps`
locally and open what it writes under `target/doc`.

## What is not yet enforced

The pages are committed, so nothing needs a nightly toolchain to publish the site. The cost is that
nothing **fails** when a source change lands without a regeneration: a stale page reads as current.
Closing it needs a nightly toolchain in CI to regenerate and byte-compare, the same treatment the
tool schemas will get once they exist. Until then, regenerating is a step in review rather than a
gate, and this paragraph is here so the gap is visible rather than assumed away.
