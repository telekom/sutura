---
name: dependencies
description: Dependency currency here - why the engine owns the Arrow major, how to tell a harmless duplicate from a blocking type boundary, the four escalation steps in order, and when a requirement needs a tilde. Open before bumping, patching, vendoring or resolving a version conflict.
---

# Dependency currency

**The target is the newest set of versions that resolve together on the day the work is done** -
not the newest of each crate, which is not a coherent set, and not whatever a record happened to
name when it was written.

A version number in a design record is stale before anybody builds from it. So a record states the
**constraint** - what the code needs to be true of a dependency - and where a number is unavoidable
it carries the date it was checked and an instruction to re-check. `rust-toolchain.toml` is the one
place a version is authoritative rather than indicative.

That rule applies to any figure, not just versions: **a bare count with no command and no date
attached is one nobody can check.** This repo has had the same test tally wrong six times in one
file. Write the command beside the number, and state its blind spot - an anchored
`grep -c '^#\[test\]$'` answers zero for tests in an indented inline `mod tests`.

## Arrow's major belongs to the engine

`sutura-exec-datafusion` is THE engine. **Its Arrow major is the workspace's Arrow major, and every
other Arrow-consuming dependency conforms to it.** Not the reverse, and not a negotiation per
adapter: a data source is replaceable and the engine is not, so the engine sets the type
vocabulary. Downgrading the engine to match an adapter is not on the table.

## A duplicate and a type boundary are different problems

Getting this backwards wastes a week in either direction.

| | What it costs | How urgent |
| --- | --- | --- |
| **A duplicate** - two majors present, no first-party code crossing between them | Build time, and supply-chain surface: two copies to patch when one has an advisory | Real, bounded, **not** a correctness risk |
| **A type boundary** - first-party code holds a value from one major and hands it to something expecting the other | It does not compile; or, forced across FFI, it is undefined behaviour | Blocking. Nothing ships through it |

**The test for which one you have is not the lock file - it is whether any first-party crate names
the type.** The current Arrow split is a duplicate, because `sutura-exec-duckdb` declares no `arrow`
dependency, names no Arrow type, and converts results into a neutral row type. `differential.rs`
compares rows rather than batches for the same reason.

## The mechanism

`deny.toml` sets `multiple-versions = "warn"`, and that stays: denying it needs a skip list of
dozens of entries that rots on every `just update`, and **a gate that fails on correct code gets
disabled** - the same reasoning `allow-wildcard-paths` already carries there.

So the mechanism is narrow and aimed at the case that matters: `cargo xtask check-arrow` reads
`Cargo.lock` and fails when the Arrow family spans more than one major, unless every major present
is named in `devco/arrow-majors-allow` with a date and a reason. It is a hygiene gate, so
`just validate` runs it. It checks the whole `arrow-*` family rather than the `arrow` crate alone,
because a transitive dependant can pin `arrow-schema` by itself and that is the same defect. Warn
is what let the current split arrive unremarked - the `deny.toml` comment predicted it, deferred
it, and was right - so the gate exists to make the *next* one arrive in a diff.

## When the newest set cannot be made compatible

**Do not vendor, inline, fork or pin back on your own judgement. Raise it.** In preference order,
and each step is only reached because the one above it failed:

1. **Bump the conforming crate** to a release that already agrees with the engine.
2. **Disable the feature that pulls the conflicting version**, where the code does not use it.
   Cheapest possible outcome and the easiest to miss.
3. **`[patch.crates-io]` on the DEPENDENT crate - never on the shared dependency.** Written out
   because the obvious form does not work and the failure is silent: `[patch]` replaces a SOURCE, so
   it cannot widen a requirement, and against disjoint requirements cargo reports `patch ... was not
   used in the crate graph` as a *warning* and keeps the old version. *Verified against this
   workspace.* What works is patching the crate that declares the stale requirement, with its own
   manifest line changed - also one line, also no fork - and it must be proved by a build rather
   than assumed.
4. **Vendor**, last, and never silently: `VENDOR.md` takes upstream repo, licence, commit, date and
   local changes, the `cargo-deny` licence gate applies, and *"inspired by" is not a licence
   position*. The real cost is not the patch - it is that **a vendored copy makes us the security
   response for it**, permanently and invisibly after the first commit.

**Step 1 was available for the Arrow case and was taken, which is why the order is not
decoration:** `duckdb-rs` had simply not bumped, so the fix went upstream as a one-line manifest
change rather than living here as a patch, with both legs measured on the same toolchain and
feature set and zero source changes needed.

An escalation raised at step 4 states four things: which crates conflict; whether it is a duplicate
or a type boundary; what each of the four options costs; and what breaks if nothing is done. Without
those it is a question, not a decision.

## A requirement that encodes something else needs a tilde, not a caret

A caret requirement is the right default, and there is one shape where it is actively wrong: **a
crate whose version encodes the version of a native library it expects to find.** `duckdb`'s second
semver component is the DuckDB C release, which is what `nix/duckdb.nix` supplies, and the crate's
own README recommends a tilde for exactly this reason.

Under a caret, `just update` may move the crate to a release expecting a newer native library than
the sandbox provides. **It links and then fails at runtime on a missing symbol** - the worst shape
of failure available, because the build is green and the fault appears when the code runs. So such a
dependency pins with `~`, and the reason goes next to the pin rather than in a commit message.

`cargo xtask check-shared-client` is the related gate on the other side: it holds that the HTTP
client stays *shared* with an existing resolution rather than adding packages to `Cargo.lock`.
