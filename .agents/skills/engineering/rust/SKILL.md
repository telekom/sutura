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
| `#[derive(Deserialize)]` on a validated newtype | `#[serde(try_from = "String")]` plus a `TryFrom` that calls `parse` | `cargo xtask check-serde-parse` - **the one that bites**, and the reason it is gated first: a derived `Deserialize` writes straight into the private field, so every check is bypassed by the one path that carries untrusted input. What makes a type subject to it is having a fallible constructor (`-> Result<Self`); a hand-written `impl Deserialize` counts as a route through it |
| `#[serde(try_from = ..)]` beside a derived `Serialize` | `#[serde(into = ..)]` too, or a hand-written `Serialize` | `cargo xtask check-serde-parse` - `try_from` moves `Deserialize` and leaves `Serialize` where it was, so the type reads text and writes a field layout. `Date` shipped that, and the definition digest is taken over the serialized form. A newtype over the `try_from` target is exempt: serde writes one as its inner value |
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

**Three ports exist, and each arrived with its implementor.** That is the rule - `AGENTS.md`'s
*Layout* states it as "a port trait arrives with its first implementor" - and it is also why there
is no fourth. A trait with no implementor and no caller is a guess at a signature only the first
real adapter can settle, and in a library crate `pub` hides it from `dead_code`, which is how an
unused item survives review. `CredentialBroker` is the live example: module comments in
`sutura-domain`, `sutura-config` and `sutura-http` name it as the port that would mint a credential
per request, and it is deliberately **absent** rather than sketched. Do not add one speculatively.

| Port | Declared in | Implemented by |
| --- | --- | --- |
| `Warehouse` - driven | `sutura-domain`, `warehouse.rs` | `DataFusionWarehouse`, the engine that ships; `DuckDbWarehouse` on the dev-dependency leg; the fakes |
| `SemanticCatalog` - driven | `sutura-domain`, `pinned.rs` | `LocalCatalog` in `sutura-catalog-local`; the fakes, and the hand-written oracle the goldens compare against |
| `Surface` - driving | `sutura-app`, `surface.rs` | `LocalService<W>`, and nothing else |

**Driven and driving are not the same rule, and the difference decides which crate declares the
trait.** A driven port is dependency inversion: the interior declares what it needs and an adapter
outside implements it, so the trait sits inside the hexagon or the direction reverses. A driving
port inverts nothing - the caller is already outside and the implementation is already the
application - so it has no adapter to arrive with, and `LocalService` is not an adapter but this
crate's own service with the warehouse's generic parameter erased. `Surface` was declared in
`sutura-http` once, and a review was right that a transport is the wrong crate for it: a second
transport would have had to reach the application's interface through the HTTP one, and nothing here
depends on an adapter. The whole argument - including why deleting a one-implementation trait was
the weaker option - is in `sutura_app::surface`'s own module documentation. Read that before moving
a port or adding one.

The rules, now that there are ports to apply them to:

| Rule | Caught by |
| --- | --- |
| No framework type reaches the domain - no `tokio`, `axum`, `rmcp`, `datafusion`, `arrow` in its tree | `check-boundaries` - a transitive **allowlist**, so a framework arriving through an innocuous crate fails too. The strongest of the three: a type cannot appear without its crate |
| A **driven** port is declared by the domain, named for what the domain needs | *review* |
| A **driving** port is declared by the application, never by one of its callers | *review*. `AGENTS.md` carries this one as an invariant and says plainly that it is not gated: `check-boundaries` reads dependency direction, not which crate declares a trait |
| A port's methods take and return domain types and domain errors only | *review*. `Surface` is where that costs something: erasing the warehouse's generic parameter erases the adapter's error TYPE, so `SurfaceFailure` keeps the error itself, owned, as a `#[source]` - the reasoning is under *Design Principles* in `AGENTS.md` |
| A port's methods stay synchronous for as long as `Warehouse` is | *review*. The engine drives its own runtime and blocks on it, so an `async` port would hide the requirement that a transport move the call onto a blocking pool - a runtime cannot be entered from within a runtime |
| The adapter wraps the third-party library and maps its errors to the domain's at the boundary | *review* |
| Composition happens in a composition root and nowhere else - `sutura-cli` for the CLI, `sutura-serve` for the HTTP surface | *review*. Both consume both driven ports, which is what lets a transport be transport-only: it never reads a catalog directory and never opens a data system |
| Generics with trait bounds for a driven port; `dyn` exactly once, and only for the driving one | *review*. `LocalService<W>` is generic in the warehouse and `start` is generic in the catalog; `ServiceState` holds `Arc<dyn Surface>`, because an `axum` handler is a concrete function - a generic port there would make the router, its state and the generated interface description generic too |
| An adapter never calls another adapter | *review* |
| No `#[derive(Serialize)]` on a domain type for a transport's convenience - a wire shape belongs to the transport | *review*. `DefinitionDigest` does derive serde, because a pinned snapshot is persisted data rather than a transport shape; that is the exception, and `#[serde(try_from)]` is what keeps it from being a hole |

Ports get **fakes**, not mocked HTTP - that is what lets the whole tool surface, refusals
included, be tested without a warehouse. `sutura-http::testing` holds a working double and a
failing one for both driven ports; `sutura-app/tests/support` holds the recording and oracle ones.

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

## 3. `--all-features` on every entry point, even though it is a no-op

No crate here declares a feature today, and nothing is `optional = true`. So the flag currently
changes nothing - and it is on every gate for exactly that reason: the day an adapter goes behind a
feature, coverage must not silently drop to nothing without anyone noticing.

The inner loop is deliberately narrow:

```bash
cargo check -p sutura-domain --no-default-features   # must stay sub-second
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features
```

The first line is `just check`, and **a green run there is not a green tree** - it compiles one
crate. The task prints that in its own output, so you do not have to remember it, and
`cargo xtask check-scope` fails if the printed scope and the `-p` flags ever disagree. When the
question is "does what I touched compile", `just check-changed` with no arguments reads the working
tree and narrows to those packages; `just lint` is the workspace gate.

The second line runs **twice** in the hooks - once on commit and again on push, from the same YAML
node so the two are the identical invocation and the second reuses the first's fingerprints. The
push run is there because `git rebase` and `git rebase --continue` run no commit hook at all, so a
conflict resolution used to reach the remote with nothing having compiled it. Do not resolve a
conflict and trust `just check`: it compiles one crate, and the merge that broke this repo was a
clean one whose call site no longer matched a changed signature.

`sutura-domain` must acquire **no** framework dependency - no tokio, axum, rmcp, datafusion,
arrow. `cargo xtask check-boundaries` enforces it.

## 4. The gates reject before review does

Run `gates` before you claim done. Individually:

| Gate | Rejects |
| --- | --- |
| `cargo xtask max-lines` | any file over 1000 lines; exemptions only in `devco/max-lines-ignore`, and never under `crates/` or `xtask/` |
| `cargo xtask unused-deps` | a declared dependency nothing references, and a `[workspace.dependencies]` entry nobody inherits |
| `cargo xtask line-endings` | CRLF. `fmt` fixes it |
| `cargo xtask text-hygiene` | conflict markers, trailing whitespace, missing final newline, files over 512 kB |
| `cargo xtask check-boundaries` | a framework dependency reaching the domain crate; and, in any library crate, a `pub` field on a `pub struct`, a declared dynamic-error crate, or a `Result` whose error type is `String` |
| `cargo xtask check-serde-parse` | a derived `Deserialize` on a type with a fallible constructor and no `#[serde(try_from = ..)]`; and a `try_from` whose derived `Serialize` writes a different shape |
| `cargo xtask check-hook-tiers` | a `pre-push` stage that compiles nothing, and a push-stage clippy invocation that is not the commit stage's own |
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
