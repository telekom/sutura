---
name: secure-by-design
description: The reasoning underneath this workspace's types - untrusted input, bounding at the edge, unrepresentable over checked, which way each default fails, and what deserves a clone or an Arc. Open when shaping a parse, an error boundary, or an allocation on the query path.
---

# Secure by design

**Nothing here is an invariant, and none of it may be cited as one.** Where a rule has a mechanism,
that mechanism is a row in `../invariants/SKILL.md` and is named rather than restated. Everything
else is **advisory - review catches it or nothing does.**

`AGENTS.md` names the four sources this repo adopts as policy. For the *mechanics* - the
mistake-by-mistake tables with a **Caught by** column - open `engineering/rust`. This file is the
argument underneath both.

Adopted as policy means every distinction the source draws, including the inconvenient ones. Three
places we deviate, deliberately:

- **`new_unchecked` does not exist here.** It is `unsafe`, `unsafe_code` is `forbid`, and a crate
  cannot re-allow it - so re-parsing an already-parsed value is the price of the ban, paid on
  purpose.
- **No `nutype`, no `derive_more`.** Write the boilerplate by hand first, and note the second reason:
  the domain's dependency allowlist is walked transitively, so a macro crate arriving in it is an
  architecture decision.
- **No `anyhow` and no `Unknown(..)` catch-all**, both of which the guides allow. `std::io::Error` is
  the cautionary tale - `ErrorKind::Other` became load-bearing because callers matched on it, and the
  repair was a hidden `Uncategorized`. Here the pressure runs the other way: a refusal variant no
  test can provoke is one the enum refuses to carry.

## Why the variant is the contract

Hyrum's Law rather than taste: with enough callers every observable behaviour gets depended on,
error strings included. The guide's example is Go's `http.MaxBytesError`, whose text carries a
comment saying it cannot be changed because something downstream matches on it.

And the whole return on the newtype pattern is *"if an instance exists, we know it is valid"*,
because that is what lets downstream code stop asking. A newtype that can still be checked has moved
the question rather than answered it, and **a check that accepts more than the type's name claims is
worse than none** - this repo shipped `DefinitionDigest::parse("not a hash")` succeeding.

## An expected outcome is not an error at all

The sharpest departure from the error guide, which is about what to put in `Err`. The question that
comes first here is whether the failure belongs there. **A governance refusal lives inside the
`Ok`.** `Err` is for something that went wrong, and a question this deployment declines is something
that went right. The reason is in the port's own doc comment: a caller must not be able to mistake
*"you may not ask that"* for a hiccup and retry until something works.

**Erasure has one honest place, and it is a boundary.** `ErasedCause` is not a counterexample to the
typed-error rule: `SurfaceFailure` keeps each cause as an owned `#[source]`, so the chain still walks
and a caller that knows the adapter can still downcast, and `ServiceError<E>` stays generic in the
adapter's error precisely so the typed error survives to that point. The shape this replaced was a
message plus a `Vec<String>` of causes - a *presentation* of an error rather than an error API.
Flattening to text still happens at the logging sink, the one place text is the point. `Send + Sync`
is not decoration: a transport answering on a blocking pool sends the failure back across a thread
boundary.

## The habits

- **A catalog document is untrusted input, not trusted configuration.** It reaches a parser, and the
  ban on the panicking fragment API exists because an abort was reachable from a catalog file.
  Anything read off disk gets the treatment a question off the wire gets.
- **Bound the input at the edge, before anything does work proportional to it.** An unbounded input
  is a denial-of-service primitive whatever else it is - which is why **availability is treated as a
  security property here, not an operational one.**
- **Prefer unrepresentable to checked.** No unbounded time range. No `expression:` field and no
  `Option<String>` at any depth on a measure. No `Display` and no `PartialEq` on a secret. A check
  can be moved, skipped or ordered wrongly; a shape that cannot hold the value cannot. The worked
  example is `docs/adr/0020`: nothing had ever leaked, and the defect was that `tracing::info!(%token)`
  *compiled* and printed `REDACTED` where its author believed a value had been logged.
- **Order the checks inside a parse for the clearest diagnostic, and say that is what you are
  doing.** Origin, size, lexical content, syntax, meaning exists so an expensive check never runs on
  input a cheap one would have rejected - but once the input is already bounded that argument is
  spent, and `NoteBody::parse` deliberately asks *is there prose here at all* before it counts bytes,
  because "this note is empty" is more accurate for its author than "this note has a hidden
  character". The source says so at the branch. **A defence and a diagnostic are different jobs; do
  not let a comment claim one and deliver the other.**
- **Sanitize, then validate, both inside the constructor**, so derived `PartialEq`, `Hash` and
  `Serialize` all agree which value this is and no comparison site has to normalise. State the limit
  with it: normalisation reaches exactly as far as what it names, and ours stops short of Unicode
  normalisation.
- **Fail closed on the query path, and state which way each default points.** A refusal the caller
  can see beats a degraded answer it cannot. The deliberate opposite is the tooling - see
  `../gates/SKILL.md`. **Neither direction is a default: what a wrong answer costs decides it, per
  mechanism, written down where the mechanism is.**
- **Nothing sensitive in an error.** The typed fields are read by machines and the `Display` by
  humans; neither is a place for a credential, a row, or a path.
- **State the limit next to the claim.** The strongest habit here and the easiest to lose. A control
  described as stronger than it is spends trust a reviewer needed elsewhere, so **an overstated claim
  is itself the defect.**

## Borrowing, and what deserves an `Arc`

Security and performance are decided together here, and both early. A needless copy on the federated
path multiplies the working set against a memory bound that REFUSES, so **an allocation in a leg is a
correctness question rather than a style one.**

- **A clone is a decision with a reason, never a way past the borrow checker.** If a lifetime is
  hard the shape is usually wrong: something is held across an await it need not cross, or owned
  where a reference would do.
- **`Arc` is for state genuinely shared across tasks and immutable once built** - the pinned bundle,
  the certified key the TLS resolver hands out. `Arc<Mutex<_>>` around per-request state is the shape
  to stop and rethink.
- **The scoped view BORROWS the pinned definitions.** Not an optimisation: it is what keeps `load()`
  off the request path and makes visibility filtering incapable of acquiring I/O.
- **Know which clones are cheap.** Arrow buffers are reference-counted by construction, so cloning a
  batch moves no data; treating it as a copy produces worse code, not safer code. The opposite
  mistake is cloning a `String` per row because a signature asked for one.
- **Measure rather than assert.** The numbers that decided the federation shape were wall clock and
  peak resident set on a real corpus. A claim about cost in a review is worth what its measurement is
  worth.
