---
title: Conformance packs for inputs and adapters
description: How the semantic compiler gets tested across catalogs and data systems - test bodies written once as packs and bound to an adapter by a macro so each behaviour keeps its own name per adapter, a declaration that selects packs and is checked in both directions, a corpus of question files so a case is a file rather than code, and the split that lets the compiler be conformance-tested with no data source at all.
---

# Conformance packs for inputs and adapters

Status: **accepted as the shape. None of it is built.**

The requirement is that every metadata input and every data adapter conforms to the **same tests and
functions**, so a new connector proves itself by registering and declaring rather than by anyone
editing a test. This record is the construction, and it is a translation of a pattern that works in a
comparable project rather than an invention.

## The pattern being translated

In the reference implementation, conformance lives in **packs**: a group of test bodies for one
behaviour - create and read, deletion, filters, versioning, writing, display - written ONCE against
the port and not against any backend. A backend's test file then composes the packs it must satisfy
and supplies one fixture that constructs the adapter. Two properties come out of that and both are
worth keeping:

1. **The test bodies are written once.** A backend contributes a constructor, not assertions.
2. **The list of packs a backend composes IS its capability declaration**, visible in one place, and a
   missing pack is visible in review rather than hidden in a skip.

There is a third detail worth copying: the packs live in their own package with their own manifest,
so the harness is a dependency rather than a directory that test files reach into sideways.

## The translation into Rust, and where it must differ

Rust has no test inheritance, and the naive substitute loses the thing that makes packs useful. A
generic function per pack - `fn write_pack<W: Warehouse>(w: &W)` - gives ONE test name per adapter, so
a failure says "the postgres pack failed" and not which behaviour. That is a worse diagnostic than the
pattern being copied, where every behaviour keeps its own identity per backend.

**So a pack is a set of generic functions, and a macro binds a pack to an adapter by generating one
named `#[test]` per behaviour.** The pack bodies stay written once; the macro exists only to give each
behaviour a name the runner can report and a filter can select:

```rust
// In the packs crate, written once, against the port.
pub fn rows_match_the_reference<W: Warehouse>(warehouse: &W, case: &Case) { /* ... */ }
pub fn a_refusal_names_the_same_variant<W: Warehouse>(warehouse: &W, case: &Case) { /* ... */ }

// In the adapter's test file, once.
sutura_conformance::execute_packs! {
    adapter: "duckdb",
    build: || DuckDbWarehouse::in_memory(),
    declares: DuckDbWarehouse::CAPABILITIES,
}
```

That expands to `conformance::duckdb::rows_match_the_reference` and one name per behaviour, which is
what `cargo nextest run -E 'test(conformance::duckdb)'` needs to select a tier and what a failure
report needs to be readable.

**The declaration selects the packs, and it is checked in BOTH directions.** Composing packs by hand
is a declaration a reviewer can read but nothing verifies. Selecting them from the adapter's typed
capabilities per
[Pluggable by declaration](0011-pluggable-by-declaration.md) is verifiable, and it lets the macro do
something the inheritance version cannot: **a behaviour an adapter declared it does not support must
FAIL if it turns out to work.** A source declaring no impersonation that impersonates is as much a
defect as the reverse, because the boot refusal is keyed on the declaration.

A skip is therefore never silent and never a missing pack. It is a passing test whose name says what
was declared - `conformance::duckdb::impersonation_is_declared_unsupported_and_is_not_available` - so
the report shows the gap rather than the absence of a line.

## The split that makes the compiler cheap to test

**The semantic compiler needs no data source to be conformance-tested**, and that is the most valuable
structural point here. Splitting the packs by what they require gives one tier that is hermetic and
fast and one that is not:

| Pack family | Needs | Pins |
| --- | --- | --- |
| **Compile packs** | A catalog. No data system at all | The plan's serialized form, the rendered statement per dialect, the bind parameters, and the refusal variant |
| **Execute packs** | A live source | The ROWS, and that they are identical to every other adapter's rows for the same case |

So a metadata adapter - markdown today, Datahub or OpenMetadata later - is conformance-tested entirely
by compile packs, against every question in the corpus, with no container anywhere. A data adapter runs
the execute packs on top.

What must be identical and what may differ, restated because it is the whole meaning of conformance:
**rows identical, refusals identical, rendered SQL different and snapshotted per dialect.** No
assertion inside a pack may hard-code dialect syntax; the moment one does, the pack has quietly become
per-adapter.


### Cases the corpus must contain by name, because nothing else finds them

A corpus grown from whatever question somebody happened to ask converges on the easy cases. Three are
named here because each was found by reasoning about the design rather than by a test, and each passes
under a partial implementation:

- **A filter on a remote dimension, together with an orphan key in the fact table.** The join-kind
  finding in [federating across different data systems](0007-federating-across-different-data-systems.md):
  a filter pushed into a lookup leg plus a left join adds a null bucket instead of narrowing the answer.
  Neither half alone catches it - an unfiltered question passes under either join kind, and a filtered
  question with no orphans passes under an inner join everywhere - so it is one case with both halves
  present, asserted against the single-source rows.
- **A ratio whose denominator is zero for one subgroup only.** The zero guard applied per leg drops that
  subgroup's numerator rather than nulling the subgroup, so the total is wrong and nothing errors. The
  case needs a subgroup with a zero denominator and a non-zero numerator, which is the shape a
  hand-written fixture does not happen to have.
- **A `CountDistinct` whose distinct key spans two join keys.** The one that cannot be re-aggregated at
  all: two customer keys sharing a subscription makes two exact distinct counts over-count when summed.
  A fixture whose keys never overlap passes with the wrong implementation.

Each is a directory like any other case. Naming them here is not a substitute for writing them - it is
what stops the corpus from being complete-looking and blind in exactly the places the design is hard.

## The corpus is files, not code

A case is a directory entry, not a function. That is already how the repository works - questions are
files under an example's `questions/`, read and iterated - and the conformance corpus extends it:
a catalog, a set of question files, and the expected rows for the reference run.

Adding a case is adding a file. Adding an adapter is one macro invocation and a capability
declaration. Neither touches a pack body, and that is the property the requirement asks for.

## Two mechanics to get right, because both are how a suite rots

- **Orphaned snapshots.** The reference project runs its snapshot tool with unused-snapshot warnings
  on, which is what stops a corpus accumulating pins nothing reads. The equivalent here is insta's
  unreferenced check - `cargo insta test --unreferenced` - and the honest statement of its status is
  that **nothing in this workspace runs it and no gate mentions it**: `cargo-insta` is not a pinned
  tool in `devenv.nix`, not on the dev shell's path, and not installed in any flake check. An earlier
  draft of this record attributed that to a note in AGENTS.md; there is no such note, and the
  correction matters because "recorded as unavailable" and "nobody has set it up" call for different
  work. With a corpus multiplied by adapters an orphan is a real gap rather than a nicety, so pinning
  the tool and adding the check belongs with this work - and if it turns out not to run in the
  sandboxed checks, the fallback is a gate of our own that reads the snapshot directory against the
  cases the macro generates, which is a thing `xtask` already knows how to do.
- **Timing.** A conformance matrix grows multiplicatively, and the tier that is supposed to be fast
  stops being fast quietly. Worth reporting per-pack timings from the start, as the reference project
  does with its own timing analyser, so the fast tier can be defended with a number.

## Consequences

- A new dev-only workspace crate holds the packs and the macro. It is never shipped, and it depends on
  the domain's ports rather than on any adapter.
- **The harness duplication this would have extracted does not exist on this branch yet.** An earlier
  draft cited `crates/sutura-cli/tests/federation.rs` and about ninety duplicated lines "at two
  corpora, so this is the third". That file is not here - `crates/sutura-cli/tests` holds `example.rs`
  and its snapshots - and the claim came from a branch that was never merged. What is true: the golden
  and refusal corpora in `sutura-app/tests` are the first corpora, the packs are the second consumer of
  the same fixtures, and the shared module is therefore written ONCE here rather than extracted from a
  duplication. Cheaper than the draft claimed, and worth correcting in the other direction too: a
  record that invents the debt it is paying off cannot be checked by a reader.
- The existing `tests/adapters` registry becomes the place an adapter is registered for the matrix,
  and the macro invocation is what registers it for the packs. One registration, not two.
- Compile packs make the semantic compiler testable against N catalogs with no data system, which is
  the tier most of the value lives in and the one that can run on every push.
