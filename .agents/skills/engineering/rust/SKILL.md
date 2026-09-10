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
rule is real but unenforced, and this file says so rather than implying a gate exists. Every such
rule is gathered again under *What stays advisory* at the foot, so the honest half is handed to a
reader rather than inferred from a column: **a rule described as enforced when it is not spends a
reviewer's trust exactly where they needed it.** A *Caught by* cell is prose, and no gate compares
it against the gate it names - so read it against the tree rather than trusting it.

### Newtypes parse; they do not validate

The point is that a value which violates the invariant cannot be constructed, so no code
downstream re-checks it. A newtype that merely *can* be checked has moved the problem.

| The mistake | Write instead | Caught by |
| --- | --- | --- |
| `pub struct Digest(pub String)` | a private field, and `parse` as the only way in | `check-boundaries` - a `pub` field on a `pub struct` in a library crate fails it |
| a constructor returning `Self` plus a separate `is_valid()` | one `parse(..) -> Result<Self, E>`; after `Ok`, nothing re-checks | *review* |
| a check that accepts more than the name claims - `!raw.trim().is_empty()` for a content hash | validate the actual shape: hex, and the exact length | *review*. This was a real bug here: `DefinitionDigest::parse("not a hash")` succeeded |
| `#[derive(Deserialize)]` on a struct or enum with a fallible constructor | `#[serde(try_from = "Input")]` plus a `TryFrom` that calls the constructor | `cargo xtask check-serde-parse` recognises `-> Result<Self` and requires a route: `try_from` or a handwritten `Deserialize`. It checks the spelling, not whether the implementation calls the constructor; that needs behavioural proof. Declaration and constructor must be recognised by the same file's lexical scan |
| `#[serde(try_from = ..)]` beside a derived `Serialize` on a struct | `#[serde(into = ..)]` too, or a hand-written `Serialize` | `cargo xtask check-serde-parse` - `try_from` moves `Deserialize` and leaves `Serialize` where it was, so the type reads text and writes a field layout. `Date` shipped that, and the definition digest is taken over the serialized form. A newtype over the `try_from` target is exempt: serde writes one as its inner value. Enum layouts are outside this rule |
| a second constructor that repeats the checks | `From` / `TryFrom` delegate to `parse` - one place a future rule gets added | *review* |
| normalising at comparison sites (`eq_ignore_ascii_case`) | normalise inside `parse`, so derived `PartialEq`/`Hash`/`Serialize` all agree which value this is | *review* |
| `impl Deref for MyNewtype` | an inherent method, or `AsRef<T>` if a borrow is genuinely wanted | `cargo xtask check-newtype-leaks` - `Deref` re-exports the inner type's whole API, and the invariant leaks out with it |
| `impl Borrow<T> for MyNewtype` | `AsRef<T>`, or key the map on the newtype and parse at the call site | `cargo xtask check-newtype-leaks`. The guide calls `Borrow` *"unofficially unsafe"*: it PROMISES the wrapper hashes, compares and orders identically to what it borrows, and the compiler checks nothing - so a `parse` that folds case turns a map lookup into a silent miss on an entry that is present |
| deriving `PartialEq` on credential material | no impl at all - a derived comparison is byte-wise and early-returning, which is a timing oracle at whatever call site adds it later | the compiler: `==` on a `Secret` does not compile |
| letting a credential out of `Secret` on the way to a log | keep it wrapped, and expose it only where the value itself is the payload | `clippy.toml` disallows `Secret::expose_secret`, so an exposure is an error under `-D warnings` until an `#[expect]` beside it names the destination. A lint, not a type: it reaches this workspace, an `#[allow]` walks past it, and doctests are outside it |

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
| an expected outcome returned as `Err` | a variant of the result. A governance refusal is `ToolOutcome::Refusal { reason }`, never an error | *review* for the CHOICE of `Ok` over `Err`. `check-refusal-coverage` fails a variant of an ENROLLED refusal enum that no test NAMES and no snapshot records, unless that enum's own allow file excuses it with a date and reason. Enrolled today: `RefusalReason`, `NotFitToServe`, `NotValidated`. **A name is not a provocation or venue execution**; snapshot words are unqualified. **A refusal-shaped enum nobody enrolled is subject to nothing**, and the enrolment is a list rather than a discovery. `xtask/src/refusals.rs` holds that narrow check, not the choice of result type |

Two local deviations from the guide, both deliberate: `#[non_exhaustive]` is **not** used
(`exhaustive_enums` is allowed in the lint table - nothing is published, so the compatibility
guarantee is worth nothing and costs a match arm at every use site), and `missing_errors_doc`
is allowed because a typed, exhaustive error enum already is the documentation.

### Dependencies point inward

`sutura-domain` is the hexagon's interior. Everything else is an adapter that depends on it,
and nothing depends on an adapter.

**Every port here arrived with its implementor**, which is the rule `AGENTS.md` states as "a port
trait arrives with its first implementor". A trait with no implementor and no caller is a guess at a
signature only the first real adapter can settle, and in a library crate `pub` hides it from
`dead_code`, which is how an unused item survives review. So do not add one speculatively -
`ls crates/*/src` and `grep 'pub trait'` is the current set.

`CredentialBroker` is the worked example of the rule being honoured rather than of abstinence: it
was named in three module comments as the port that *would* mint a credential per request and left
deliberately absent, and it arrived in `sutura-domain`'s `identity` module with a real implementor
in `sutura-config` - and a second, exchanging one in the BigQuery adapter. Which is the point: the
signature it has now is the one two implementors settled, not the one a sketch guessed.

| Port | Kind | Note |
| --- | --- | --- |
| `Warehouse` | driven | The engine that ships, the dev-dependency legs, the networked adapter behind its feature, the fakes |
| `SemanticCatalog` | driven | The local catalog, the declaring one, the fakes, and the hand-written oracle the goldens compare against |
| `CredentialBroker` | driven | Two real implementors - see `sutura/identity` for which one every shipped binary builds |
| `AuditSink` | driven | The tracing sink in `sutura-runtime`, and the recording fakes |
| `Surface` | driving | `LocalService`, this crate's own service, plus a failing double |

**This table is the hexagon's BOUNDARY and not the grep result, and the difference is not a
rounding error.** An adapter may declare a port of its own for a seam inside itself - a transport
for a job API, a reader for a metadata aspect - and several do; those are internal and belong to
their crate, not to the interior. One of them, `sutura_http::inbound::keys::KeySetSource`, is
allowlisted BY NAME in `xtask/src/boundaries/ports.rs` with the reason. The whole set is what
`grep -rn 'pub trait ' crates --include='*.rs' | grep '/src/'` answers:
12 `pub trait` declarations under `crates/*/src` against the rows above, and that gap is what this
paragraph is about.

**The count is gated; the rows are not, and the difference is worth reading exactly.**
`check-guidance` counts that same literal over `crates/*/src/**/*.rs` - occurrences, so two
declarations in one file are two - and fails this page when the number here disagrees, so
`just hygiene` is what keeps it equal to the tree. **The glob is the authority and the command
above is an approximation of it**, agreeing on today's tree rather than by construction: the
trailing `grep '/src/'` filters output LINES, so a hit in a non-`src` file on a line that mentions
a `/src/` path counts for the reader and not for the gate, and `*` does not cross `/`, so a crate
nested deeper than `crates/<name>/src` would be inside the command and outside the glob. Nothing compares the ROWS against anything: which ports the interior
owns, and which of them is driving, is prose. A port table is the shape this page is most prone to
rotting into - the paragraph above once named `CredentialBroker` as a port deliberately ABSENT,
accurate when written, and it has had two implementors since - so measure the rows rather than
reading them. Two limits on the gated half: it counts the literal wherever it appears, a comment or
a string included, which is exactly what the grep above does, and it says nothing about which of
the declarations is a port.

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
| A **driving** port is declared by the application, never by one of its callers | `check-boundaries` - no `pub trait` in a crate that declares a normal dependency on `sutura-app`, except one allowlisted in `boundaries/ports.rs` with a reason. `AGENTS.md` carried this as an invariant and said plainly that it was not gated; that sentence is now spent - see `sutura/invariants` for its remaining limits |
| A port's methods take and return domain types and domain errors only | *review*. `Surface` is where that costs something: erasing the warehouse's generic parameter erases the adapter's error TYPE, so `SurfaceFailure` keeps the error itself, owned, as a `#[source]` - the reasoning is in `sutura/secure-by-design` |
| A port's methods stay synchronous for as long as `Warehouse` is | *review*. The engine drives its own runtime and blocks on it, so an `async` port would hide the requirement that a transport move the call onto a blocking pool - a runtime cannot be entered from within a runtime |
| The adapter wraps the third-party library and maps its errors to the domain's at the boundary | *review* |
| Composition happens in a composition root and nowhere else - `sutura-cli` for the CLI, `sutura-serve` for the HTTP surface | *review*. Both consume both driven ports, which is what lets a transport be transport-only: it never reads a catalog directory and never opens a data system |
| Generics with trait bounds for a driven port; `dyn` exactly once, and only for the driving one | *review*. `LocalService<W>` is generic in the warehouse and `start` is generic in the catalog; `ServiceState` holds `Arc<dyn Surface>`, because an `axum` handler is a concrete function - a generic port there would make the router, its state and the generated interface description generic too |
| An adapter never calls another adapter | `check-boundaries` for the half that has an honest definition: no NORMAL dependency between two adapters of the same class - `sutura-exec-*`, `sutura-catalog-*`, or the two transports. `sutura-sql`, `sutura-runtime` and a composition root are legitimate cross-adapter edges and are in no class; a dev-dependency is exempt, because that is how a corpus reaches a real system. *review* for anything outside those three classes |
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

## 3. `--all-features` on every entry point - now load-bearing

The flag went onto every entry point while it was still a no-op, which was the cheapest time to do
it. **That day has passed:** several crates declare features and have optional dependencies, so an
entry point missing the flag now lints and tests nothing behind them. `check-guidance` fails a cited
cargo line without it, and `just gates` adds a DEFAULT-feature lane besides - because a
`#[cfg(feature = ..)]` compiled only with the feature on is exactly the shipped set's blind spot.
See `sutura/crate-map` for why an adapter is behind a default-off feature at all.

**The limit that survives, because a green run invites the wider reading.** Its package list is
DERIVED from `nix/shipped.nix`'s `binaries`, so it reaches the binaries a release publishes and
nothing else - a feature on a crate that does not ship is compiled by the `--all-features` gates
only, and by nothing at the default set. The lane's CI half is no longer partial: the required `ci`
job runs `nix run .#default-features` for the compile and lint halves and
`nix run .#default-feature-tests` for the tests, both gated on the Rust classification.
`xtask/src/default_features.rs` and `xtask/src/default_feature_tests.rs` each state their own limit,
and the shared one is that neither LINKS - both stop where `cargo check` and a host-triple test
binary stop, so the four `cross` builds remain the authority on a musl link. Still run `just gates`
when you touch a `#[cfg(feature = ..)]`: it is the same two gates, minutes earlier.

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

The second line runs once in the hooks: the `rust-clippy` commit hook is the only place clippy runs.
It USED to run twice - once on commit and again on a `pre-push` alias, so that `git rebase` and
`git rebase --continue`, which run no commit hook at all, could not push a conflict resolution
nobody had compiled. That push-compile tier is deliberately retired: `just lint` and the commit hook
are the compile gates, and an uncompiled rebase now reaches CI rather than being caught locally. Do
not resolve a conflict and trust `just check`: it compiles one crate, and the merge that broke this
repo was a clean one whose call site no longer matched a changed signature.

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
| `cargo xtask check-boundaries` | a framework dependency reaching the domain crate; a normal dependency between two adapters of the same class; a `pub trait` in a crate that declares `sutura-app`; and, in any library crate, a `pub` field on a `pub struct`, a declared dynamic-error crate, or a `Result` whose error type is `String` |
| `cargo xtask check-serde-parse` | a recognised struct or enum deriving `Deserialize` beside a fallible constructor without an allowed route; for structs only, a `try_from`/derived `Serialize` shape mismatch or a named input struct without `deny_unknown_fields` |
| `cargo xtask check-refusal-coverage` | a variant of an enrolled refusal enum (`RefusalReason`, `NotFitToServe`, `NotValidated`) that no test names and no snapshot records, unless separately excused - a file naming EVERY variant of an enum is a census and counts for none of that enum. Also a walk that disagrees with the enrolled variant count, in either direction |
| `cargo xtask check-newtype-leaks` | a first-party `impl Deref`, `DerefMut`, `Borrow` or `BorrowMut`. It started green and its job is to stay that way |
| `just lint`'s `disallowed_methods` | a call to `Secret::expose_secret`, `Warehouse::verify_anchor`, `tokio::task::spawn_blocking` or the panicking fragment parser with no `#[expect]` naming why |
| `cargo xtask check-hook-tiers` | a `pre-push` stage that compiles first-party code, or that runs anything outside the two security checks (`secret-sweep`, `cargo-deny`) |
| `cargo xtask commit-msg` | a subject that is not a conventional commit, over 72 chars |

## What stays advisory, and this list is the point of it

Every row above whose *Caught by* cell says *review*, plus **borrowing**, which has no row of its
own. Naming them is the deliverable, because the failure this page guards against is a rule
described as enforced when it is not - and `AGENTS.md` calls an overstated control the defect
itself. Nothing below fails a build. A reviewer catches it or nothing does.

**Deliberately no count.** Nothing keeps a number here equal to the rows above;
`grep -c '^|.*\*review\*'` over this file is the answer, and it is a raw `grep` rather than a `just`
task because no task counts it.

**Newtypes.** One `parse` and no separate `is_valid`. A check that accepts more than its name
claims. A second constructor that repeats the checks instead of delegating. Normalising at
comparison sites rather than inside `parse`.

**Errors.** Typed fields rather than a sentence in the message. One enum per fallible operation
rather than an umbrella per module. And the CHOICE of `Ok` over `Err` for an expected outcome,
which is the least mechanisable of the three principles' consequences: only a reader can tell that
a thing which went *right* is being returned as an error. What `check-refusal-coverage` gates is
narrower than the choice and narrower than a provocation.

**Ports and adapters.** A driven port declared by the domain and named for what the domain needs. A
port's methods taking and returning domain types only. A port staying synchronous while `Warehouse`
is. An adapter mapping its library's errors at the boundary. Composition in a composition root.
Generics for a driven port and `dyn` exactly once for the driving one. No serde derive on a domain
type for a transport's convenience. And any cross-adapter edge outside the classes
`check-boundaries` knows about, since a crate joins one by name.

**Assertions over rendered text.** A substring assertion is a probabilistic one unless something
makes it deterministic, and two properties do - **either** suffices:

- the needle carries a character the **haystack's own alphabet cannot spell**. Haystack-relative, and
  it cannot be written as a fixed list of characters: `-` and `_` are IN base64url, so `key-id` over
  a JWK coordinate collides exactly as `kid` did, and `/`, `.` and `-` are all in a filesystem path.
- the haystack is **deterministic** - free of nondeterministic bytes. Not "hand-written": a `Debug`
  rendering and a line parsed out of this repository's own source both qualify. And the reason is
  never *"the value cannot be in there"* when that is the property the assertion exists to test -
  that argument assumes its own conclusion.

Three further habits, each of which has cost a test its meaning here:

- **Read the value, not the text**, wherever a parsed form is reachable - the member rather than the
  serialized document, the field set rather than the rendered log line.
- **Presence is not a role.** A sentence promising two numbers needs each asserted where it belongs;
  two bare numbers pass with the two swapped.
- **Search the words, not the lines**, over anything wrapped, or a formatter reddens the cell instead
  of the thing it names.

A **needle-side** lint would catch the first of these and needs no knowledge of haystacks - *a needle
carries a non-alphanumeric character, or is at least N characters*. It is not here on measured cost
rather than difficulty: over `crates/`, `xtask/` and `dev/` a `.contains("...")` literal appears 1116
times, 268 with an alphanumeric needle, and the rule flags **52 at N=5**, 95 at N=6, 179 at N=8. The
predicate is quoted because a bare count is not checkable, and because the figure moves with the
tree - the same count against two merge bases four weeks apart differs by more than the rule's own
threshold does. And the two commonest real offenders are a `const` and a `to_string()`, which a
literal scan does not see at all. The decision is `github.com/telekom/sutura#429`.

**Borrowing.** Preferring a borrow to a clone, and knowing which clones are cheap. There is no
mechanism and there is not going to be one: a clone is a decision with a reason, and a gate cannot
read the reason.

Two of these could plausibly become gates and deliberately are not. *One `parse` and no `is_valid`*
would have to guess which method is the constructor, and *a port's methods take domain types only*
would have to know what a domain type is. Both would fail correct code, and a gate that fails
correct code gets disabled - which costs more than the rule was worth.

## Conventions

- Rust 2024. One version for the workspace; crates inherit with `version.workspace = true`.
- The compiler pin is the single pinned nightly in `devco/rust-toolchain-nightly.toml`, which
  Nix reads; the top-level `rust-toolchain.toml` is the rustup-facing copy rustup reads directly.
- Ports get **fakes**, not mocked HTTP. A test asserting on source text proves nothing.
- Adding a dependency: `unused-deps` requires it to be referenced, and `cargo-deny` checks
  its licence and advisories. Both run in the gates.

## Before claiming completion

Paste the command and its output. A new or changed test must also satisfy the causality requirement in `AGENTS.md`: red against base behaviour, green on your change.
