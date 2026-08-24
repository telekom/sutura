---
name: rust
description: Rust in this workspace - the three principles that shape types and errors, the lint table, the panic ban, feature gating, and what the gates reject before review does.
---

# Rust here

Three principles decide how types and errors are shaped here. Four workspace choices decide
what the compiler will let you write at all. The principles come first, because a lint can
only reject - it cannot tell you what to write instead.

## The three principles

Sources, and the definition of "correct" for a review here:
[newtypes](https://www.howtocodeit.com/guides/ultimate-guide-rust-newtypes),
[error handling](https://www.howtocodeit.com/guides/the-definitive-guide-to-rust-error-handling#structured-error-handling-in-rust),
[hexagonal architecture](https://www.howtocodeit.com/guides/master-hexagonal-architecture-in-rust).

Read the **Caught by** column literally. Where it says *review*, nothing fails the build - the
rule is real but unenforced, and this file says so rather than implying a gate exists.

### Newtypes parse; they do not validate

The point is that a value which violates the invariant cannot be constructed, so no code
downstream re-checks it. A newtype that merely *can* be checked has moved the problem.

| The mistake | Write instead | Caught by |
| --- | --- | --- |
| `pub struct Digest(pub String)` | a private field, and `parse` as the only way in | `check-boundaries` - a `pub` field on a `pub struct` in a library crate fails it |
| a constructor returning `Self` plus a separate `is_valid()` | one `parse(..) -> Result<Self, E>`; after `Ok`, nothing re-checks | *review* |
| a check that accepts more than the name claims - `!raw.trim().is_empty()` for a content hash | validate the actual shape: hex, and the exact length | *review*. This was a real bug here: `DefinitionDigest::parse("not a hash")` succeeded |
| `#[derive(Deserialize)]` on a validated newtype | `#[serde(try_from = "String")]` plus a `TryFrom` that calls `parse` | *review*. **The one that bites:** a derived `Deserialize` writes straight into the private field, so every check is bypassed by the one path that carries untrusted input |
| a second constructor that repeats the checks | `From` / `TryFrom` delegate to `parse` - one place a future rule gets added | *review* |
| normalising at comparison sites (`eq_ignore_ascii_case`) | normalise inside `parse`, so derived `PartialEq`/`Hash`/`Serialize` all agree which value this is | *review* |
| `impl Deref for MyNewtype` | an inherent method, or `AsRef<T>` if a borrow is genuinely wanted | *review*. `Deref` re-exports the inner type's whole API, and the invariant leaks out with it |
| deriving `PartialEq` on credential material | no impl at all - a derived comparison is byte-wise and early-returning, which is a timing oracle at whatever call site adds it later | the compiler: `==` on a `Secret` does not compile |

An error type per constructor, and keep it small: if testing every failure permutation is a
chore, the type is doing too much.

### The error type is part of the API

| The mistake | Write instead | Caught by |
| --- | --- | --- |
| `Result<T, String>` in a library crate | a `thiserror` enum naming the failure modes | `check-boundaries` - a `String` error type in a library crate fails it |
| `anyhow::Result` in a library crate | your own enum. `anyhow` is right in a binary, where the error's audience is a human reading stderr | `check-boundaries` - a dynamic-error crate (`anyhow`, `eyre`, ..) declared by a crate with a `[lib]` target fails it |
| `Err(MyError::Invalid(format!("digest has {n} chars")))` | `MyError::WrongLength { value, len, expected }` - typed fields, not a sentence | *review*. The variant and its fields are the contract; the `#[error(..)]` text may be reworded, and a caller that parsed it was never promised anything |
| one umbrella `Error` for a whole module | one enum per fallible operation, carrying only what that operation can produce | *review*. Ten variants where two apply makes the caller filter noise |
| `.map_err(\|_\| MyError::Bad)` | `#[from]` or `#[source]`, so the cause survives the boundary | `clippy::map_err_ignore` - the `restriction` category is on |
| an expected outcome returned as `Err` | a variant of the result. A governance refusal is `ToolOutcome::Refusal { reason }`, never an error | *review*, plus the per-variant test each `RefusalReason` requires |

Two local deviations from the guide, both deliberate: `#[non_exhaustive]` is **not** used
(`exhaustive_enums` is allowed in the lint table - nothing is published, so the compatibility
guarantee is worth nothing and costs a match arm at every use site), and `missing_errors_doc`
is allowed because a typed, exhaustive error enum already is the documentation.

### Dependencies point inward

`sutura-domain` is the hexagon's interior. Everything else is an adapter that depends on it,
and nothing depends on an adapter.

**There are no port traits yet, and that is deliberate.** A port exists to invert a dependency
on something outside the hexagon, and no adapter exists to invert. A trait with no implementor
and no caller is a guess at a signature only the first real adapter can settle - and in a
library crate `pub` hides it from `dead_code`, which is how an unused item survives review. Do
not add one speculatively.

When the first adapter does arrive:

| Rule | Caught by |
| --- | --- |
| No framework type reaches the domain - no `tokio`, `axum`, `rmcp`, `datafusion`, `arrow` in its tree | `check-boundaries` - a transitive **allowlist**, so a framework arriving through an innocuous crate fails too. The strongest of the three: a type cannot appear without its crate |
| The **domain** declares the trait, named for what the domain needs (`Warehouse`, `SemanticCatalog`, `CredentialBroker`) | *review* |
| A port's methods take and return domain types and domain errors only | *review* |
| The adapter wraps the third-party library and maps its errors to the domain's at the boundary | *review* |
| Composition happens once, in `sutura-cli`; generics with trait bounds, not `dyn` | *review* |
| An adapter never calls another adapter | *review* |
| No `#[derive(Serialize)]` on a domain type for a transport's convenience - a wire shape belongs to the transport | *review*. `DefinitionDigest` does derive serde, because a pinned snapshot is persisted data rather than a transport shape; that is the exception, and `#[serde(try_from)]` is what keeps it from being a hole |

Ports get **fakes**, not mocked HTTP - that is what lets the whole tool surface, refusals
included, be tested without a warehouse.

## 1. The whole `restriction` category is on

`clippy.toml` and the workspace lint table enable `restriction` at `warn`, and CI runs
`-D warnings`. Consequences that bite in normal code:

| You wrote | Use instead |
| --- | --- |
| `x.to_string()` | `String::from(x)` - `str_to_string` is denied |
| `format!` into an existing `String` | `push_str` / `write!` - `format_push_string` |
| `let _ = f()` on a `#[must_use]` | handle it, or `drop(f())` if truly discardable |
| `use` after a statement | move it to the top of the module |
| `#[allow(..)]` | `#[expect(.., reason = "..")]` - `allow_attributes` fails a bare allow |
| `match x { Some(v) => v, None => .. }` | `if let` / `?` - clippy will name the lint |

Overrides live in the lint table, each with its reason. Disagree with a specific line there;
do not add a blanket allow.

## 2. No panic path from input

`unwrap_used`, `expect_used`, `panic` and `indexing_slicing` are **denied** for library code
and exempt in tests (`allow-*-in-tests` in `clippy.toml`). Shipped profiles use
`panic = "abort"`, so a panic is a process death, not an exception.

Slices are walked with `split_first` rather than indexed. `unsafe_code` is `forbid` - not
`deny` - so a crate cannot re-allow it locally.

## 3. Features are default-off, so `--all-features` is mandatory

Adapters are feature-gated and off by default. A bare `cargo clippy --workspace` inspects
almost nothing and still reports success. Every gate passes `--all-features`; so should you.

The exception is the fast inner loop, deliberately narrow:

```bash
cargo check -p sutura-domain --no-default-features   # must stay sub-second
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features
```

`sutura-domain` must acquire **no** framework dependency - no tokio, axum, rmcp, datafusion,
arrow. `cargo xtask check-boundaries` enforces it.

## 4. The gates reject before review does

Run `gates` before you claim done. Individually:

| Gate | Rejects |
| --- | --- |
| `cargo xtask max-lines` | any file over 1000 lines; exemptions only in `.max-lines-ignore`, and never under `crates/` or `xtask/` |
| `cargo xtask unused-deps` | a declared dependency nothing references, and a `[workspace.dependencies]` entry nobody inherits |
| `cargo xtask line-endings` | CRLF. `fmt` fixes it |
| `cargo xtask text-hygiene` | conflict markers, trailing whitespace, missing final newline, files over 512 kB |
| `cargo xtask check-boundaries` | a framework dependency reaching the domain crate; and, in any library crate, a `pub` field on a `pub struct`, a declared dynamic-error crate, or a `Result` whose error type is `String` |
| `cargo xtask commit-msg` | a subject that is not a conventional commit, over 72 chars |

## Conventions

- Rust 2024. One version for the workspace; crates inherit with `version.workspace = true`.
- The compiler pin is `rust-toolchain.toml` and nowhere else - rustup and Nix both read it.
- Ports get **fakes**, not mocked HTTP. A test asserting on source text proves nothing.
- Adding a dependency: `unused-deps` requires it to be referenced, and `cargo-deny` checks
  its licence and advisories. Both run in the gates.

## Before claiming completion

Paste the command and its output. A new or changed test must also satisfy the causality
requirement in `AGENTS.md`: red against base behaviour, green on your change.
