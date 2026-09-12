---
title: Conformance packs for inputs and adapters
description: How the semantic compiler gets tested across catalogs and data systems - test bodies written once as packs and bound to an adapter by a macro so each behaviour keeps its own name per adapter, a typed declaration that selects the packs and, for the capabilities whose absence has something to try, is exercised in that direction too, what "identical rows" actually means in canonical form and tolerance and ordering, a corpus of question files so a case is a file rather than code, and the split that lets the compiler be conformance-tested with no data source at all.
---

# Conformance packs for inputs and adapters

Status: **accepted as the shape. None of it is built.**

The requirement is that every metadata input and every data adapter conforms to the **same tests and
functions**, so a new connector proves itself by registering and declaring rather than by anyone
editing a test. This record is the construction.

## The unit is a pack, and the reason is where the assertions live

**A pack is a group of test bodies for one behaviour, written ONCE against the port and never against
an adapter.** An adapter's test file then names the packs it must satisfy and supplies one fixture
that constructs the adapter. Three properties, and each is the reason for a rule further down:

1. **The test bodies are written once.** An adapter contributes a constructor, not assertions. An
   assertion that appears twice is an assertion that will disagree with itself, and the disagreement
   will be read as a difference between two data systems rather than as a difference between two
   copies of a test.
2. **Which packs an adapter must satisfy is its capability declaration**, in one place, so a missing
   pack is a thing a reviewer can see rather than a skip nobody reads. The version of that below is
   stronger: the declaration is TYPED and the macro selects from it, so it is not a list somebody
   keeps in step by hand.
3. **The packs live in their own crate with their own manifest**, so the harness is a dependency
   rather than a directory that test files reach into sideways. That is also what keeps it out of
   every shipped artifact: a dev-only crate that depends on the domain's ports and on no adapter.

## What Rust makes this cost, and where the shape has to change

Rust has no test inheritance, and the naive substitute loses the thing that makes packs useful. A
generic function per pack - `fn write_pack<W: Warehouse>(w: &W)` - gives ONE test name per adapter, so
a failure says "the postgres pack failed" and not which behaviour. Every behaviour has to keep its own
identity per adapter, or the report is a worse diagnostic than the suite it replaced.

**So a pack is a set of generic functions, and a macro binds a pack to an adapter by generating one
named `#[test]` per behaviour.** The pack bodies stay written once; the macro exists only to give each
behaviour a name the runner can report and a filter can select:

```rust
// In the packs crate, written once, against the port.
pub fn rows_match_the_reference<W: Warehouse>(warehouse: &W, case: &Case) { /* ... */ }
pub fn a_refusal_names_the_same_variant<W: Warehouse>(warehouse: &W, case: &Case) { /* ... */ }

// In the adapter's test file, once.
sutura_conformance::execute_packs! {
    adapter: duckdb,
    build: || DuckDbWarehouse::in_memory(),
    declares: DuckDbWarehouse::CAPABILITIES,
}
```

That expands to `conformance::duckdb::rows_match_the_reference` and one name per behaviour, which is
what `cargo nextest run -E 'test(conformance::duckdb)'` needs to select a tier and what a failure
report needs to be readable.

**`adapter` is an IDENT and not a string, and that is the whole of what decides `macro_rules!` versus
a proc-macro.** An earlier draft of this record wrote `adapter: "duckdb"`, which does not build: a
`literal` fragment in `mod $name` position is `error: expected identifier, found metavariable`, and
getting from a string to a path segment needs a paste-style crate - a second dependency, in the test
tree, to do something the language already does. With `adapter: duckdb` the expansion is
`mod duckdb { #[test] fn rows_match_the_reference() { .. } }` and the reported name is
`duckdb::rows_match_the_reference`. Both halves were checked by compiling them rather than reasoned
about.

**So it is `macro_rules!`, exported from the packs crate, and no second crate exists.** The property
that makes that sufficient is that the naming scheme NESTS rather than CONCATENATES: the adapter is a
module and the behaviour is a function inside it, so nothing ever has to build an identifier out of
two pieces - which is the one thing `macro_rules!` cannot do on stable and the only reason a
proc-macro would have been needed. A scheme spelled `duckdb_rows_match_the_reference` would have
forced the second crate for a worse filter expression.

**The declaration selects the packs, and a skip is never silent.** Naming packs by hand is a
declaration a reviewer can read and nothing verifies. Selecting them from the adapter's typed
capabilities per [Pluggable by declaration](0011-pluggable-by-declaration.md) is verifiable, and it
means a gap in the matrix is a test whose NAME says which declaration produced it rather than the
absence of a line somebody would have to notice.

**And where a declared absence has something to try, the pack tries it and the absence must hold.** An
earlier draft of this record put that as a general rule - "a behaviour an adapter declared it does not
support must FAIL if it turns out to work" - and it is not general. For most capabilities there is
nothing to perform, and a green test named `..._is_declared_unsupported_...` over nothing is
coverage-shaped and measures nothing, which this repository holds to be worse than no test at all. So
the capabilities are split, and the split is the decision:

| Declared absence        | Is there an action?                          | What the pack does                                                                                                                                                                                                                                                                                                                                                                                      |
| ----------------------- | -------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A pushed aggregate kind | **yes**                                      | Hands the adapter a plan carrying a pushed aggregate of the undeclared kind and asserts a TYPED REFUSAL. Worth having precisely because the planner is supposed to never generate one: this is the only thing that exercises the adapter's own guard, and a guard that silently computed it locally instead would be the failure                                                                        |
| A dialect               | **yes**                                      | Asks for the plan rendered in a dialect the adapter did not declare, and asserts a typed refusal. A compile-tier pack, so it needs no source                                                                                                                                                                                                                                                            |
| Mutual TLS              | **yes**                                      | Constructs the adapter with a `Mutual` transport it did not declare and asserts the construction refuses. It is the same shape [transport security](0010-transport-security-for-a-source.md) already decides one variant down, where a driver that cannot verify at all offers no `Verified` construction and the deployment refuses - so this pack asserts the refusal exists rather than inventing it |
| Arrow-native access     | **no, not yet**                              | `Warehouse` has one result-bearing method and it returns a `RowSet`, so there is no Arrow entry point to call and nothing an absence could be observed at. Where an Arrow-typed boundary lives is undecided in [the plan](0009-the-plan-from-one-source-to-many.md); the pack arrives with the boundary and not before                                                                                  |
| Impersonation           | **no, and this one is the instructive case** | Nothing. See below                                                                                                                                                                                                                                                                                                                                                                                      |

**Why impersonation gets no negative pack.** Two candidate actions, and both are wrong. Presenting a
subject credential to an adapter that declared it cannot impersonate, and asserting that the adapter
*ignored* it, asserts as correct the exact fallback
[a credential per leg](0008-a-credential-per-leg-for-the-calling-subject.md) forbids - the execution
port takes a credential and there is no downgrade. And the direction the draft was actually worried
about, a source that declares no impersonation and impersonates anyway, is not observable from the
port at all: seeing it needs two subjects whose grants differ at the source and a way to tell whose
rows came back, which is the test `AGENTS.md` already records as one that "does not exist and cannot,
until a credential exists per leg". So the mechanism for impersonation is the **boot refusal**, and
its test is one over the composition root - a deployment declaring an impersonation it cannot perform
does not start - which lives beside the startup checks rather than in a pack over the port.

An illustrative name, then, from a row that has an action rather than from the row that does not:
`conformance::duckdb::a_pushed_median_is_refused_because_the_adapter_declares_it_cannot_receive_one`.

## The split that makes the compiler cheap to test

**The semantic compiler needs no data source to be conformance-tested**, and that is the most valuable
structural point here. Splitting the packs by what they require gives one tier that is hermetic and
fast and one that is not:

| Pack family       | Needs                            | Pins                                                                                                         |
| ----------------- | -------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| **Compile packs** | A catalog. No data system at all | The plan's serialized form, the rendered statement per dialect, the bind parameters, and the refusal variant |
| **Execute packs** | A live source                    | The ROWS, and that they are identical to every other adapter's rows for the same case                        |

So a metadata adapter - markdown today, Datahub or OpenMetadata later - is conformance-tested entirely
by compile packs, against every question in the corpus, with no container anywhere. A data adapter runs
the execute packs on top.

What must be identical and what may differ, restated because it is the whole meaning of conformance:
**rows identical, refusals identical, rendered SQL different and snapshotted per dialect.** No
assertion inside a pack may hard-code dialect syntax; the moment one does, the pack has quietly become
per-adapter.

### What "identical rows" means, defined here because this is where the assertion is

[The plan](0009-the-plan-from-one-source-to-many.md)'s fourth decision delegates the definition to
this record, and the delegation is not a formality: *"identical" needs defining or the execute packs go
red on the first network source for reasons that are not defects* - floating-point sums differ by
summation order, decimal scale and rounding differ per system, tie order and NULL placement differ, and
date truncation differs across date and timestamp types and time zones. `differential.rs` has met none
of those because it compares two engines under one type mapping.

So three rules, in one comparison module and nowhere else. A comparison written independently at two
call sites eventually differs at the second one, and the failure then reads as a difference between
data systems rather than between two copies of a policy.

**Rule 1: the canonical form is CLASSED, and the class comparison comes before the value
comparison.** A `RowSet` is compared as the column labels in projection order, then the rows, then
each cell against the cell in the same column position. Each cell is reduced to a class and a value,
and the classes are:

| Class              | Which `Value` variants | Canonical value                                       |
| ------------------ | ---------------------- | ----------------------------------------------------- |
| null               | `Null`                 | none. A null cell equals a null cell and nothing else |
| exact integer      | `Integer`              | the integer itself                                    |
| approximate number | `Real`                 | the digits rule 2 states                              |
| text               | `Text`                 | the bytes, unchanged                                  |

Three things that shape is chosen to avoid, and the first is a defect in the harness this replaces.
**`Value::render()` alone is not a canonical form for this comparison**, because `Value::Null` renders
as the string `null` and so does `Value::Text("null")` - `differential.rs`'s `rendered` therefore
cannot tell a null cell from a cell containing that word, and copying it into the packs would carry the
collision to every adapter. **A cell that is `Integer` on one adapter and `Real` on another is a
type-mapping difference and the packs refuse it, naming both sides.** That is a decision with a cost,
stated: the first network adapter will go red until its type mapping agrees, because `SUM` over a wide
integer comes back as `NUMERIC` in one system and as `HUGEINT` in another and it is the adapter's job
to land both on the same exact-text fallback class once the value is past `i64`. Tolerating an exact
class against an approximate one would mean an exact count on one source and an
approximated one on another were "the same answer", which is the opposite of what a conformance suite
is for. **And a date is in the text class**, because a date cell arrives as ISO text - the golden row
snapshots record `Text(2026-01-01)` - so the date-truncation differences 0009 names show up as a text
difference, which is the readable failure rather than a numeric one.

**Rule 2: no tolerance for the exact classes, twelve significant digits for the approximate one.**
Null, text and `Integer` are compared exactly: byte for byte for text and value for an integer.
`Real` is compared at twelve significant digits, written `{:.12e}` through `Real`'s `LowerExp`
implementation, which exists for this comparison and says so in its own documentation. The argument is
already in the tree and is not a loosening: summing the same rows in a different order changes the last
place of an `f64`, so comparing the full binary expansion asserts that both sides summed in the same
order, which is not a property any adapter promises. Twelve digits is far beyond any figure a metric
reports and far short of the noise, and it has fired once for real on the example corpus.

Four limits, next to the claim. **Significant digits rather than an epsilon**, because an epsilon has
to be defended per column magnitude and significant digits are scale-free - and because the number
already exists in three places in this repository, so adopting it adds no constant. **A real difference
in the twelfth significant digit of a reported number is invisible to these packs**, deliberately;
nothing in the corpus reports one, and a metric that did would need its own case rather than a tighter
comparison. **There is no NaN case to decide**, because `Real::parse` refuses a non-finite value at the
adapter boundary, so `NaN`-compares-unequal-to-itself cannot arise inside a comparison here. And
**text is compared without Unicode normalisation**, because there is none anywhere in this workspace -
so two spellings a reader cannot tell apart are two values, which is the same limit `Phrase` states
about itself.

**Rule 3: the packs impose a total order over the canonical form before comparing, and the assertion
is over a MULTISET.** The generated statement already carries an `ORDER BY` over every group column -
the dimension columns first and the truncated time bucket last, which the SQL goldens show - so this is
a re-sort rather than a sort of unordered rows, and it exists because the source executes that
`ORDER BY` under its own **collation**
and its own **NULL placement**, neither of which the plan states and both of which differ per system.
The order imposed is: rows sorted by their canonical cells left to right in projection order, the null
class sorting FIRST as a stated position rather than an inherited one, and text ordered by bytes,
which is the C collation named rather than whatever the source's locale happens to be. Column labels
are NOT reordered - projection order is part of what the plan decides, and an adapter that returned the
columns in another order is a defect rather than a variation.

**Multiset and not set, and that is load-bearing rather than pedantic:** a duplicated row is exactly
what a fan-out defect produces, and it is the failure the cardinality precondition in `AGENTS.md`
rests on trust for, so a comparison that deduplicated would hide the one class of bug the corpus's own
named cases are aimed at.

**The re-sort above was SUPERSEDED before the packs were built, and the limit it carried is now
false.** What landed is two functions rather than one: `agree_on_content` tallies rows into a
multiset, so it needs no sort at all to be order-independent, and `agree_on_order` compares position
for position. The execute packs call both - `Behaviour::Order` is `agree_on_order`, and a wrong cell
reddens it naming `row 0, column 2 (amount_total)` - so *the execute packs cannot see a row-ORDER
defect* is exactly backwards about the built harness. The stronger behaviour is the right call, and
correcting the record matters because the sentence would otherwise be cited to justify dropping it.

**Half of what the re-sort existed for is now DECIDED and the other half is still open, and this
record used to state them as one.** The sentence was: a source executes the plan's `ORDER BY` under
its own **collation** and its own **NULL placement**, *"neither of which the plan states"*. That is
now false about the second of them, so the per-case opt-out this record asked the corpus branch to
build alongside the null case **is not built and is not owed**.

**NULL placement IS stated, and uniformly: `ASC NULLS LAST`.** `sutura_sql`'s `ordered_nulls_last`
puts it in the AST for every dialect and `every_order_by_states_nulls_last` holds it there;
`sutura-exec-datafusion` renders no SQL at all and reaches the same placement, because
`LogicalPlanBuilder::sort_by` is `Expr::sort(true, false)`. So there is no case whose null order a
conforming source may legitimately answer differently, and the null-in-a-group-key case lands on
its own with the null group expected LAST.

**What that case detects is NOT our statement of the placement, and the record should not be read
as claiming it is.** Measured: the layer collapses `NULLS LAST` away for every target whose default
is already nulls-last, so deleting `nulls_first: Some(false)` leaves `DuckDB`'s and Postgres's
rendered SQL **byte-identical** and changes only `BigQuery`'s text - and `BigQuery` has no
`execute_packs!` binding, so no corpus cell executes it. Our statement of the placement is held by
`every_order_by_states_nulls_last`, over the AST, which is the right venue for a rendering
decision.

**Two things the case does buy.** `Behaviour::Order` over a null key pins the three bound engines'
own default null ordering - most usefully `datafusion`'s, which a version bump could change with no
diff of ours, so this is a dependency regression detector rather than a check on first-party code.
`Behaviour::Content` over the same row is first-party: a null key must be a GROUP and not a row a
join or a filter dropped.

**COLLATION is still open, and the corpus still avoids it** - all-lowercase ASCII keys with distinct
first letters. A case whose TEXT order a source could legitimately answer differently would still
need the field the packs do not have, and would still report a locale as a conformance failure. The
per-source row snapshot in `sutura-app/tests` remains the place a collation difference is a diff a
reviewer reads rather than a red cell.

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

## The corpus data is a file; cases are code

The shared input rows live in `crates/sutura-conformance/corpus/conformance_events.csv`. Plans and
expected rows are currently constructors in `crates/sutura-conformance/src/corpus.rs`; there is no
file-backed question loader or expected-output format in this pack.

Adding a case therefore changes code today. Adding an adapter remains one macro invocation and a
capability declaration without touching a pack body.

## Two mechanics to get right, because both are how a suite rots

- **Orphaned snapshots, and the tool that has to be installed before any of this is checkable.** A
  corpus multiplied by adapters accumulates pins nothing reads unless something reports them, and the
  report is what keeps a snapshot suite honest: an orphan is a case that was renamed or deleted while
  its expected output stayed behind, so the suite is green about a question nobody asks any more. The
  mechanism here is insta's unreferenced check -
  `cargo insta test --unreferenced` - and the honest statement of its status is that **nothing in this
  workspace runs it and no gate mentions it**: `insta` is a normal dev-dependency, but `cargo-insta`,
  the CLI subcommand that check needs, is **not installed anywhere**. Not in `devenv.nix`, not on the
  dev shell's path, not in any flake check. An earlier draft of this record attributed that to a note in
  AGENTS.md; there is no such note, and the correction matters because "recorded as unavailable" and
  "nobody has installed it" call for different work.

  **So the first task is an installation, and where it goes is already decided by a rule.** AGENTS.md:
  *nix is the only pin for a tool whose version changes what it reports; pixi holds only `prek` and
  `python`, and `cargo xtask check-pins` fails if a tool appears in both.* A snapshot tool's version
  decides whether an orphan is reported, so it is exactly that class of tool. **`cargo-insta` goes into
  `devenv.nix`** - not pixi, which `check-pins` would fail, and not `cargo install`, which pins
  nothing. Future tense on purpose: `grep -rn cargo-insta devenv.nix nix/ flake.nix` returns nothing
  today, so this is the decision about where it goes and not a description of where it is.

  **And pinning it in the dev shell is necessary but not sufficient, which is the part worth deciding
  now.** `flake.nix` already records the general case: a dev-shell tool is not on a flake check's path,
  and installing one in CI just to reach a check adds a dependency the pipeline does not otherwise need.
  Two routes, and this record picks the second:

  1. Add `cargo-insta` to the `nextest` check's inputs and run the unreferenced check there. It reads
     the unfiltered tree already, so the snapshots are visible. The cost is a tool in the CI closure.
  2. **A gate of our own in `xtask`**, which reads the snapshot directory and compares it against the
     cases the conformance macro generates. Chosen, for three reasons: `xtask` gates are unit-tested and
     already walk repo files, the comparison is *more* precise than "unreferenced" because the macro
     knows the exact case list, and it adds nothing to the closure. `cargo-insta` is then pinned in the
     dev shell and nowhere else, for `cargo insta review` - accepting a changed snapshot interactively,
     which is what a developer actually needs it for. **Future tense, like the sentence above it:** it
     is not pinned anywhere today, and "stays pinned" is what this line said until review caught the
     two halves of one paragraph disagreeing about whether the tool exists.

  With a corpus multiplied by adapters an orphan is a real gap rather than a nicety, so both halves -
  the pin and the gate - belong with this work.
- **Timing.** A conformance matrix grows multiplicatively, and the tier that is supposed to be fast
  stops being fast quietly. So per-pack timings belong in it, and the reason is the same one that
  governs every other number in this repository: the fast tier is defended with a measurement or it is
  defended with a feeling. `cargo nextest` already reports per-test durations, so the missing half is
  aggregating them per pack and per adapter rather than instrumenting anything.

  **"Reported from the start" is what this line said until the first packs landed without them, and
  the correction was the point:** nothing aggregated a duration per pack or per adapter, and no gate
  read one. What existed was nextest's own per-test line, which is per CELL and is discarded unless a
  reader passes a flag.

  **What ships now, and the sentence above it that was wrong about the MECHANISM rather than the
  tense.** *"`cargo nextest` already reports per-test durations, so the missing half is aggregating
  them rather than instrumenting anything"* is false, and measuring is what showed it. nextest
  reports one duration per cell, which cannot be split at the seam that makes this matrix
  multiplicative: `execute_packs!` rebuilds the fixture once per BEHAVIOUR, so a cell's number is
  *fixture plus behaviour* and aggregating it would report the fixture as the behaviour's cost.
  **Measured, and stated as the measurement survived re-taking.** The figure this record rests on is
  a FRACTION rather than an ordering: over the two bindings with the instrumented split, *the
  fixture is between a third and three quarters of every cell* - 43-68% per cell across four
  serialized re-takes of
  `cargo nextest run -p sutura-exec-duckdb -p sutura-exec-datafusion --all-features -E 'binary(conformance)'`
  with `--test-threads=1`, and 35-71% per cell on a `just test` run of the whole suite, where the
  per-adapter totals came out 74.1 of 125.0 ms for `duckdb` and 42.5 of 88.1 ms for `datafusion`.
  The limit case is the DECLINED cell, and it is the clearest statement of the seam: `datafusion`'s
  pre-flight behaviour ran 5.6 ms of fixture against 22.5 µs of pack, because `dry_run` answered
  `NotAsked` and nothing was checked - a per-cell duration would have reported that as the cost of
  the behaviour.

  **An earlier version of this paragraph read the ORDERING off one sample, and that does not
  reproduce - withdrawn rather than quietly kept.** It said the fixture was the larger term for one
  adapter and the smaller for the other, from `duckdb` at 39.2 against 16.4 ms and `datafusion` at
  24.2 against 49.9 ms; re-taken, `datafusion`'s fixture was the larger term in three of four runs,
  so the reading held in one run of four. The absolute numbers moved by about 5x across those runs
  on a machine with concurrent builds, which is exactly why a single sample cannot carry an
  ordering - and why `n` and the spread are printed here beside the number that survived.

  The structural argument needs no ordering, and that is the point of keeping it separate: nextest
  reports one duration per cell, the fixture is INSIDE it, and under nextest each test is its own
  process, so nothing in a run can join two numbers. The aggregation the withdrawn sentence assumed
  would have to be a gate reading a run's machine-readable output.

  So the deliverable is a REPORT, taken at the two `Instant`s that sentence said were unnecessary.
  `sutura_conformance::Spent` is a witness whose fields are private and whose constructors both
  measure - a `Duration` parameter would have let a caller report a number nobody took - every cell
  prints `total (fixture + pack)` beside its behaviour's name, and the census prints the per-adapter
  FLOOR: `behaviours x fixture`, the cost a binding pays before an assertion runs, multiplied by the
  same list the census compares, so the multiplier is the number of tests actually emitted rather
  than a constant beside it. `.config/nextest.toml`'s second override is what keeps all of it on a
  green run.

  **Two halves deliberately not taken, each for its own reason.** Aggregating per PACK across
  adapters is a gate over a run's output rather than a measurement, and it needs the run artefacts a
  test process has not got. And a BUDGET: a threshold nobody measured cannot be re-taken, so the
  numbers above are cited with the command that produced them and any budget comes second with its
  own measurement.

## Consequences

- A new dev-only workspace crate holds the packs and the macro. It is never shipped, and it depends on
  the domain's ports rather than on any adapter.
- **The harness duplication this would have extracted does not exist on this branch yet.** An earlier
  draft cited `crates/sutura-cli/tests/federation.rs` and about ninety duplicated lines "at two
  corpora, so this is the third". That file is not here - `crates/sutura-cli/tests` holds `example.rs`
  and its snapshots - and the claim came from a branch that was never merged. What is true: the golden
  and refusal corpora in `sutura-app/tests` are the first corpora, and the shared module is therefore
  written ONCE here rather than extracted from a duplication. Cheaper than the draft claimed, and
  worth correcting in the other direction too: a record that invents the debt it is paying off cannot
  be checked by a reader.

  **And "the second consumer of the same fixtures" is now false as well.** The packs do not read the
  example corpus: reaching it needs a catalog adapter and the compiler, and the harness may depend on
  neither - `check-boundaries`' harness half is what holds that. So the packs carry a corpus of their
  own and the workspace has a THIRD, which is a cost this record should not have hidden inside a
  sentence about avoiding duplication. It is paid deliberately: the alternative is a harness that
  cannot be a dependency an adapter's own crate takes, which is the whole reason the crate exists.
- The existing `tests/adapters` registry becomes the place an adapter is registered for the matrix,
  and the macro invocation is what registers it for the packs. One registration, not two - **and as
  built it was two, with nothing relating them.** A data system is named in
  `crates/sutura-app/tests/adapters/mod.rs` and bound again in its own crate's
  `tests/conformance.rs`, and no gate compared the two lists: deleting a binding left every check
  green.

  **`cargo xtask check-conformance-bindings` is what relates them now**, and the shape is the part
  worth recording: the registry side is DERIVED - the `data_systems` arm read out of the tree, with
  each entry's crate taken from the first path segment of the adapter type it names - because a
  hand-written list of adapters is a second thing to keep true. Two candidate sources were rejected
  and the reasons generalise: `Warehouse` implementors cannot be told apart from the fakes by any
  text scan, and crate membership counts a crate nobody registered (`sutura-exec-bigquery` is
  exactly that). It holds both directions, and it also holds the two properties the per-adapter and
  whole-suite nextest selectors rest on and nothing enforced - the binding's file name and its
  wrapper module - which is why `telekom/sutura#135` wants the same gate.

  **What it compares is a written INVOCATION and its position, never an emitted test name, and the
  strong form is DEFERRED rather than claimed.** Three text shapes were measured satisfying a needle
  while emitting no cell - a `cfg` the gate cannot evaluate, a one-line string literal spelling the
  invocation, and the invocation inside an uninvoked `macro_rules!` body - and each is now a refusal
  naming the file and line, alongside a manifest that turns off test autodiscovery. That is as far
  as text reaches: proving the test EXISTS needs a run's machine-readable output, which is the same
  artefact the withdrawn aggregation sentence above assumed and the same one a per-pack aggregate
  would need. Recording it here is the decision; a gate over a run's output is where it would live.

  **It arrived with ONE declared exemption, that exemption WAS this consequence going unmet, and it
  is gone:** `sutura-exec-postgres`, registered and deliberately unbound because the packs had no
  way to say *no tier is up here* that was not a pass. The golden matrix had one
  (`DataSystemUnderTest::available`) and the packs carried no equivalent, so a binding written then
  would have been green over a data system that never answered. `telekom/sutura#348` gave them one -
  `sutura_conformance::Fixture`, which is what a binding's `open` RETURNS, so an adapter whose data
  system may not be listening cannot omit the question and a reviewer reads which answer it gave -
  and `crates/sutura-exec-postgres/tests/conformance.rs` binds the packs. So the exemption list is
  empty, and *one registration, not two* is met for every registered data system. An exemption that
  stops being true - naming an entry the registry no longer carries, naming the wrong crate, or
  excusing something that turns out to be bound - fails the gate rather than quietly widening it,
  which is why the entry had to be deleted rather than reworded.

  **The distinction that made the availability path safe to add, recorded because collapsing it is
  the failure this whole record is written against:** *this adapter cannot do that* is
  `Outcome::Declined`, a typed statement about the ADAPTER, and *this venue has not provisioned a
  tier* is a statement about the ENVIRONMENT. They print under different words and are decided at
  different levels - a declination comes out of a pack that ran, an absence stops the pack running -
  and a census over an absent fixture prints no coverage line at all, because a behaviour count
  beside cells that each reported `NOT RUN` is the skip that reads as coverage. **Which venue may
  skip is not the packs' decision**: `sutura_dev::requirement` decides it once for every harness
  here, and only the thing that provisioned a tier declares one. **And a DECLARED absence is refused
  wherever a venue did provision one** - the hole was measured before that arm existed (tier up,
  requirement set, a fixture answering absent unconditionally: everything passed, with only
  printed lines as a tell). What remains is a developer machine that provisioned nothing, where a declared
  absence and a discovered one are indistinguishable and the diff is where the fixture's one line is
  read.

  And that registry becomes the source CI's own job matrix is EMITTED from, rather than a category
  list typed into a workflow file. A registered adapter absent from CI's matrix is not an error in
  YAML, which is the quiet way a new adapter ends up conformance-tested locally and untested in CI.
- Compile packs make the semantic compiler testable against N catalogs with no data system, which is
  the tier most of the value lives in and the one that can run on every push.
- **The compile packs live behind a default-off `compile` feature of the harness crate.** They need
  `sutura-semantic` and `sutura-sql` - two crates the harness is otherwise forbidden from reaching,
  because crossing to them would let a pack body be written against something concrete. Growing them
  unconditionally would break the portability contract `crates/sutura-conformance/src/corpus.rs`
  states - a pack can be bound to an adapter without acquiring a catalog adapter, the compiler or the
  renderer - and would add the compiler+renderer+dialect closure to data adapters' test builds, most
  materially `sutura-exec-datafusion`, whose test build links neither today. So the manifest names an
  empty `default`, a `compile` feature carrying exactly the three dependencies
  (`sutura-semantic`, `sutura-sql`, `serde_json`), and `xtask/src/boundaries/harness.rs`'s harness
  gate holds the shape: the default-feature walk keeps the closure to the interior, a `compile_feature`
  check refuses `default = ["compile"]`, and the `--all-features` lanes of `just test`/`just lint`
  build and run the compile cells, so neither direction is a switch nobody flips.
