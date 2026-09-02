---
title: A credential the compiler refuses to print
description: Why sutura_domain::identity::Secret is built on secrecy rather than on a hand-written redaction - what four accidents stopped compiling, what the dependency costs measured against the shipped binary rather than the workspace, why the serde feature is off and what that does and does not buy, and the two BigQuery wire documents where the accident was still representable one layer up.
---

# A credential the compiler refuses to print

Status: **accepted.** Built. `sutura_domain::identity::Secret` is a newtype over
`secrecy::SecretString`; four `compile_fail` doctests with compiling twins pin what stopped building,
each verified non-vacuous by unmarking it and reading the compiler's reason. The dependency cost is
**+2 packages in the shipped binary's graph and +1 in `Cargo.lock`** - measured, below.

## What was wrong, and it was not a leak

`Secret` was `Secret(String)` with a hand-written `Debug` **and** a hand-written `Display`, both
printing a placeholder. Nothing ever leaked through either one, and there is no incident behind this
record.

The defect is that the accident stayed **representable**. Both of these compiled:

```rust,ignore
tracing::info!(token = %token, "a caller presented a token");
let line = format!("token={token}");
```

Each produced a line reading `REDACTED`, at a call site whose author believed a value had been
logged. That is a redaction - a *check* - and this repository's own rule is *prefer unrepresentable to
checked*: a check can be moved, skipped or ordered wrongly, and a shape that cannot hold the value
cannot. A `Display` on a credential is a shape that holds it.

Two further gaps sat beside it. There was **no zeroization**: the value stayed in the process image
after the type dropped, for as long as the allocator left the page alone. And in
`sutura-exec-bigquery`'s credential wire, `Secret` was applied only *after* parsing - so one layer up,
two private documents derived `Debug` while holding a private key, a refresh token, a client secret
and an access token as raw `Option<String>`.

## Decision

**`sutura_domain::identity::Secret` is a newtype over `secrecy::SecretString`, and the `Display` is
gone.**

```rust,ignore
pub struct Secret(SecretString);
```

`secrecy::SecretBox<str>` has no `Display`, no `PartialEq`, and - with its `serde` feature off - no
`Deserialize`. So four things that used to compile no longer do, and each is pinned by a
`compile_fail` doctest with a compiling twin beside it that differs by one token:

| Accident | Compiler's reason, from unmarking the doctest |
| --- | --- |
| `format!("token={token}")` | `E0277: Secret doesn't implement std::fmt::Display` - *"cannot be formatted with the default formatter"* |
| a `Display` bound, which is what `%` desugars to | `E0277: Secret doesn't implement std::fmt::Display`, at the `impl Display` parameter |
| `a == b` | `E0369: binary operation == cannot be applied to type Secret` |
| `serde_json::from_str::<Secret>(..)` | `E0277: the trait bound Secret: serde::Deserialize<'de> is not satisfied` |

The real macro is pinned separately, in `sutura_http::inbound` - the one module holding caller-supplied
token material *and* a `tracing` dependency. `tracing::info!(token = %token, ..)` fails with
`E0277: Secret doesn't implement std::fmt::Display`, pointing at `tracing_core::field::display`. It
cannot live in the domain: `cargo xtask check-boundaries` walks `sutura-domain`'s whole resolve graph,
which includes dev-dependencies, so a `tracing` dev-dependency there would fail the gate. The domain
therefore pins the **bound** and says so; the transport pins the **macro**.

### Why a newtype and not the library type directly

Three properties, and the first is the one that matters:

1. **A `Deserialize` cannot arrive by feature unification.** `secrecy`'s `serde` feature gives
   `SecretBox` a `Deserialize`, cargo unions features across a build graph, and a crate added later
   could turn it on for its own reasons. A newtype that derives nothing is unaffected. So
   `default-features = false` in the manifest is a supply-chain decision and **not** the mechanism -
   which matters, because the "a caller cannot state its own identity" invariant rests on the absence
   of that impl.
2. `SecretString` has a `Default`. That is the newtype guide's own counterexample pointed at
   credentials: a default secret is not a thing, and it would be a value that never passed a
   constructor.
3. `SecretString` has `From<String>` and `From<&str>` - two ways in that bypass `Secret::new`. The
   wrapper has one canonical constructor because it does not restate them.

`Secret::expose` is renamed `Secret::expose_secret`, after `secrecy`'s own trait method, so the
vocabulary is one word rather than two. It is an **inherent** method and not an `ExposeSecret` impl: a
trait would have to be in scope at every call site, and implementing it would make `Secret`
substitutable for the library type in generic code, which is the opposite of what the newtype is for.
[0008](0008-a-credential-per-leg-for-the-calling-subject.md) refers to the old name in two places;
those sentences are left as written, because an accepted record is a record of what was decided then.

### The two BigQuery wire documents

`wire::credential`'s private `Document` and `TokenResponse` **lose their `Debug` derives.** Their
fields stay `Option<String>`: `Secret` has no `Deserialize`, deliberately, so deserialize-then-wrap is
the only shape available and these types are the layer where the value is still bare.

Nothing printed them - `serde_json`'s errors carry a line and a column rather than the input, and the
one `{document:?}` in that crate's tests is an unrelated `&str` fixture. **The mechanism here is
weaker than the one above, and it is worth being plain about that:** these are private types, so there
is no `compile_fail` doctest to write and no gate that fails. What holds is that the derive is absent
and that adding it back is a line in a diff. Review catches it or nothing does.

## What this costs, measured

**One new entry in `Cargo.lock`: 446 packages to 447, and it is `secrecy`.** `zeroize 1.9.0` was
already resolved - `rustls` and `rustls-pki-types` both depend on it - at the same version, so this
introduces no duplicate; `zeroize_derive 1.5.0` was already in the lock beside it. Checked by grepping
the base lockfile for all three names rather than by reading the diff, because a lockfile diff of a
compacted file is not readable evidence.

**Two in the shipped binary's graph: 231 packages to 233.** `sutura-cli` links the engine and no TLS
stack, so it had neither. That is the honest number for a release artifact, and it is the one to quote.

`cargo tree -p sutura-domain --all-features -e normal`:

```text
sutura-domain v0.2.4
├── secrecy v0.10.3
│   └── zeroize v1.9.0
├── serde v1.0.229
...
```

`secrecy` pulls **only** `zeroize`, and pulls it with `default-features = false, features = ["alloc"]`,
so nothing follows behind it. Neither is a framework: no runtime, no client, no engine, no proc macro.
Both are `Apache-2.0 OR MIT`, which `deny.toml` already allows - so the licence allowlist needs no new
entry and no existing allowance becomes unused, which `unused-allowed-license = "deny"` would have
failed on. Checked with `cargo deny`, not assumed.

Two costs are real and are not on that list:

* **`zeroize` contains `unsafe`** - volatile writes and a compiler fence. That is precisely why it is a
  dependency rather than sixty local lines: `unsafe_code` is `forbid` across this workspace, the
  guarantee is that the writes are not optimised away, and a test cannot observe whether they were.
  The same argument already carries `sha2` and `subtle` into this tree. `secrecy` itself is
  `forbid(unsafe_code)`.
* **`secrecy 0.10.3` was published 2024-10-09** and is the newest release; it has not moved in nearly
  two years. For a ~330-line crate whose whole content is three trait impls and a `Drop`, "finished"
  is a reasonable reading of that - but it is a maintenance bet, and the newtype is what makes it a
  cheap one to unwind: `Secret`'s public surface is `new`, `expose_secret`, `Debug` and `Clone`, so
  replacing the inner type touches one file.

`sutura-domain`'s dependency list is now five crates rather than four, and `ALLOWED_IN_DOMAIN` in
`xtask/src/boundaries.rs` names **three** new packages with the reason. That list exists so this is an
argument in a diff, and it worked as intended: adding `secrecy` and `zeroize` alone failed
`check-boundaries` by name -

```text
xtask check-boundaries: FAILED - sutura-domain reaches crates it may not:
  zeroize_derive
```

- because that gate walks the whole-workspace resolve graph rather than deciding which optional edges
a feature resolver would really enable. `zeroize`'s `derive` feature is off, nothing in this workspace
turns it on, and `cargo tree -p sutura-domain --all-features` does not list `zeroize_derive`; it is an
entry of the over-broad kind that list already documents, and its own tree - `proc-macro2`, `quote`,
`syn` - is the serde derives' tree, already allowed. The domain's walked tree is 31 crates, all
allowlisted.

## What was considered instead

**Delete only the `Display` from the hand-written `Secret`, and add no dependency.** This gets the
first two rows of the table above - `{}` and `%` both stop compiling - for zero new packages, and it
was put to the user as the cheaper option.

It was **not** chosen, and the decision is settled rather than open. It buys nothing for `==` beyond
what was already there, buys nothing at all for zeroization, and leaves the redaction a hand-written
formatter that a future diff can edit. The point of the change is that no first-party code formats a
credential; keeping our own `Debug` keeps a function whose job is to be careful. Against a measured +1
lockfile entry, that is not a trade worth taking.

**Take `zeroize` alone and keep the hand-written type.** Same package count as `secrecy`, and it
delivers the wipe. Rejected because it delivers *only* the wipe: the `Display` removal would still be
ours to remember, and `secrecy` is the vocabulary a reader already knows.

**A `Secret<T>` generic over the material.** Everything credential-shaped here is UTF-8 text.
`SecretSlice<u8>` is available under the same dependency on the day something holds key bytes, and a
generic invented before its second case would be a shape nobody could check.

## What is claimed, and what is not

**Claimed.** No first-party code can render a `Secret`: there is no `Display` to call, the `Debug` is
`secrecy`'s and prints `SecretBox<str>([REDACTED])`, and `==` does not compile. Neither impl can be
reintroduced by a derive on the wrapper, because the inner type has neither. The value is wiped on
drop.

**Not claimed.**

* **The wipe covers the buffer this type holds, and no copy made before the value reached it.** A
  settings file read into a `String`, a `serde`-deserialized `Option<String>` on a wire document, and
  the `String` `Secret::new` consumes are ordinary allocations - and `String::into_boxed_str`
  reallocates whenever capacity exceeds length, freeing the original buffer unwiped. Shortening that
  window means parsing into `Secret` closer to where bytes are read, not a stronger claim here.
* **There is no test for the wipe.** Observing it means reading memory after free, which is undefined
  behaviour. What is tested is that the type composes; the property is `zeroize`'s.
* **`{secret:?}` still compiles**, and is safe only because that `Debug` cannot render the value. A
  call site can still write `token.expose_secret()` into a log, which is why that method is named to
  be conspicuous in a grep rather than relied upon to be absent.
* **The BigQuery `Debug` removals are held by review**, not by a mechanism. Said again here because
  the rest of this record is about mechanisms.
* **Nothing about `Serialize`.** `Secret` has none and never had one, and no doctest pins that
  separately - the `Deserialize` one is the half a request could reach.
* **Only two of the five `compile_fail` doctests are red against the base behaviour**, and this file
  says which rather than letting five look like five. The `{}` one and the two `Display`-bound ones -
  the domain's and the real `tracing` macro in `sutura_http::inbound` - all PASS on base, which is a
  failure for a `compile_fail` test, because the old type had a `Display`; that was demonstrated by
  restoring the impl and watching them fail. The `==` and `Deserialize` ones would have passed before
  this change too: both impls were already absent. They are in anyway, as **regression pins on two
  invariant rows whose mechanism moved** - from *we did not write the impl* to *the inner type has
  none, so a derive on the wrapper cannot produce one*. A row being restated is the moment to pin it;
  calling either one red-before-green would not be true.

## Amendment, 2026-09-02: the `expose_secret` limit is now a lint

The bullet above - *"a call site can still write `token.expose_secret()` into a log, which is why
that method is named to be conspicuous in a grep rather than relied upon to be absent"* - is the one
limit in this record that a mechanism could close, and #145 closed it. `clippy.toml` disallows
`sutura_domain::identity::Secret::expose_secret`, so an exposure is an error under `-D warnings`
until somebody writes an `#[expect]` beside it saying what the exposed value is for. That is the
`Warehouse::verify_anchor` and `tokio::task::spawn_blocking` shape, and it was **verified to resolve**
the way this repository's `clippy.toml` header demands: the entry was added and clippy rejected the
existing call sites by file and line - twenty-one diagnostics across eleven files, answered by
eighteen expectations, because `#[expect]` is per item and three functions expose twice.

**What changed is the DEFAULT, not the possibility.** The exposures that were legitimate are still
there and still legitimate - a constant-time compare of the deployment token, the bearer header a job
is submitted with, the RFC 8693 subject token, a `PKCS#8` key handed to `ring`, an assertion handed to
a broker, and the tests that read a minted credential in order to say whose it was. None of them
reaches a log. What is new is that a twenty-second one cannot arrive without a diff a reviewer sees.

**Three limits, and the first is the one to read.** A lint is not a type: it reaches this workspace,
an `#[allow]` walks past it, and clippy does not lint doctests at all - the three doctests in
`sutura_domain::identity` that call the method are outside it. `#[expect]` is per ITEM rather than per
call, so a function carrying one expectation may make two exposures; the reason text is what a
reviewer reads, and `#[expect]` at least fails when the last one goes away. And the ban says nothing
about what the exposed `&str` is then *used* for - that is still review, which is why every reason
here names the destination.
