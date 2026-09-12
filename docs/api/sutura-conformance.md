<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-conformance

The public API of `sutura-conformance`, rendered from rustdoc JSON.

The conformance packs: one set of test bodies over the ports, bound to an adapter by a macro.

`docs/adr/0012` is the construction and this crate is the first piece of it built. The
requirement it serves is that **a new data system proves itself by registering and declaring**
rather than by anybody editing a test - so the bodies live here, written once against the port,
and an adapter contributes a constructor.

```ignore
// In the adapter's own crate, in `tests/conformance.rs`.
#[cfg(test)]
mod conformance {
    fn open() -> Fixture<DuckDbWarehouse> { /* attach `corpus::on_disk()` */ }

    sutura_conformance::execute_packs! {
        adapter: duckdb,
        warehouse: sutura_exec_duckdb::DuckDbWarehouse,
        open: crate::conformance::open,
        executes_legs,
    }
}
```

# Three properties, and each is the reason for a rule below

1. **The packs live in their own crate**, depending on `sutura-domain` and on no adapter - so the
   harness is a dependency an adapter's own crate can take rather than a directory another
   crate's tests reach into sideways. That is what `crates/sutura-exec-bigquery/tests/corpus.rs`
   could not do and had to hand-write instead.
2. **Every behaviour keeps its own name per adapter.** A generic function per pack would give one
   test name per adapter, so a failure would say *the duckdb pack failed* and not which
   behaviour. `execute_packs` exists only to give each behaviour a name the runner reports and
   a filter selects.
3. **A behaviour an adapter cannot satisfy, or that stopped existing, is never a silent pass.**
   Four mechanisms, because the cases are different and they are not equally strong - see below.

# How an unsupported capability is kept out of the green count

| Case | Mechanism |
| --- | --- |
| A capability with a **typed declaration** on the port (`EXECUTES_LEGS`) | The binding names the declaration, a `const` assertion tears a binding that disagrees with it, and the two directions get DIFFERENT test names - so which one ran is in the report rather than in a skip nobody reads |
| A capability with **no constant to declare** (`Warehouse::dry_run`) | The pack returns `Outcome::Declined` carrying a typed `Declination`, which `hold` prints as `DECLINED` and `.config/nextest.toml`'s second override keeps on a green run. Stated limit: it is observed at RUN TIME, so it is weaker than the row above and would become that row the day the port carries the constant |
| A behaviour that loses its test | The `#[test]`s and the list `census` compares are ONE repetition inside `execute_packs`, so a test cannot be deleted without deleting its census element, and the element is compared against `Behaviour::EVERY`. **This is the corrected version:** it was two hand-written lists 370 lines apart in this file, and a review deleted the content behaviour's test while leaving the variant in both - every census passed |
| The corpus being empty, which would make every behaviour vacuously green | `census` fails on a corpus with no cases, and prints the case count beside the behaviour count. Reachable as a `Fault::EmptyCorpus` too, through `execute::a_leg_is_refused_over` |
| The **environment** a networked adapter needs not being here, which is not a case above because it is not about the adapter at all | `Fixture` is what a binding's `open` returns, so an absence is a VALUE rather than a panic in a fixture; `not_here` prints `NOT RUN` under the behaviour's own name, and `census` prints no coverage line at all where the fixture did not stand up. The skip-or-fail DIRECTION is deliberately not this crate's - a provisioner that set `SUTURA_DEV_REQUIRE_TIER` gets a failure out of `sutura_dev::provisioned::here` before an absence can reach here at all |

# What a green conformance run does NOT establish

- **Ordering within a leg, and impersonation in any form.** `execute`'s header says which and
  why; `docs/adr/0012` decides the impersonation one.
- **Most of the port.** The packs call `execute` and `dry_run`. `verify_anchor`,
  `working_set_exhausted`, `result_did_not_fit`, `preflight` and `preflight_was_refused` are
  never called, so *held to the same test bodies* is a statement about two methods and not about
  `Warehouse`. Three of those five carry guarantees of their own in
  `.agents/skills/sutura/invariants`, held by other mechanisms.
- **That the corpus is hard.** It is two questions over one table - `corpus` lists by name the
  cases `docs/adr/0012` says nothing else finds, none of which is here yet.
- **That every adapter is held IS held now, and not by anything in this crate.** A pack is bound
  where an adapter's own crate binds it, so which adapters conform used to be a reading of which
  crates carry a `tests/conformance.rs` - deleting one left `just validate` green.
  `cargo xtask check-conformance-bindings` compares the golden matrix's `data_systems` registry
  against the crates holding a binding, with one declared exemption. **Its limit is the one this
  crate cannot help with:** it holds that a registered data system HAS a binding, never that a
  pack's body asserts anything - the four mechanisms above are what cover that, and a pack
  returning `Ok` unconditionally passes all of them and the gate.
- **That a binding reporting its fixture ABSENT asked anything, on a machine that provisioned
  nothing.** `Fixture` is a type a binding fills in and this crate cannot see a socket:
  `xtask/src/boundaries/harness.rs` holds it to `sutura-domain` alone, and that gate's own
  remedy assigns *reaching a provisioned tier* to the adapter's fixture. **In a venue that
  provisioned one this is closed** - `not_here` and `census` both fail a declared absence
  wherever `REQUIRE_TIER` is set, and `nix/with-tier.sh`'s `sutura_tier_up` STARTS a tier and
  then exports it, so that is `just test`, `just gates`, `just causality`'s head run and
  `nix/run-gate.sh tests` on a machine with the tier binary, plus `checks.nextest` in the
  sandbox. Measured before that arm existed: a fixture answering `Fixture::Absent`
  unconditionally, with the tier UP and the variable set, was 21 passed and the only tell was
  seven printed `NOT RUN` lines; with it, 7 of 7 fail.

  **The residual is two venues rather than *a developer machine*, and both are checkable** -
  *a developer machine* was the sentence that stood here and it points at the case where the
  refusal DOES fire. (1) A host with no `sutura-postgres-tier` on `PATH`: `sutura_tier_up`'s
  `command -v` arm returns before the export, which is that file's *a hook that cannot run must
  not be a wall* posture. (2) `just causality`'s BASE run, where `xtask::causality` removes the
  variable on purpose, because an export follows a process tree and the endpoint file the base
  worktree would need does not. In those two a declared absence and a discovered one are the
  same value.
- **That `corpus::on_disk` is this run's corpus and nobody else's.** It renames the rows onto
  `<temp_dir>/sutura-conformance/<table>.csv`, a name carrying no worktree and no digest, so a
  second checkout of this repository is a second WRITER of that file. *The bytes are identical
  either side* holds per tree, not per machine, and two `just test` runs in two worktrees is how
  the change that wrote this paragraph was reviewed. A cell would fail as a
  `Fault::Content` naming the case and the adapter while the run that caused it stayed green -
  the one reading these packs exist to make unambiguous. `telekom/sutura#405` is where the path
  gets per-worktree isolation; it is deliberately not fixed here, because it is a change to a
  fixture every binding shares and this file's diff is about one adapter.
- **A COST, rather than a budget.** `Spent` reports what every cell and every fixture took,
  and `census` prints the per-adapter floor; nothing thresholds either, and nothing joins two
  adapters' numbers. `docs/adr/0012` carries what the remaining half would need.

## `enum Behaviour`

```rust
pub enum Behaviour
```

One behaviour in a pack: the unit a test name, a failure report and a CI filter all key on.

An enum rather than a string, so the pack's own list and the tests the macro emits are compared
by the compiler at one end and by `census` at the other.

### Variants

- `Labels` - The answer's labels are the ones the plan projects.
- `Content` - The rows are the reference's rows, as a multiset.
- `Order` - The rows are in the order the plan's `ORDER BY` claims.
- `Determinism` - One plan, asked twice, answered the same way twice.
- `PreFlight` - A pre-flight that accepted the plan is followed by an answer.
- `Leg` - A leg is executed, or refused, as the adapter's declaration says.

### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The name a report carries.

### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

## `enum Outcome`

```rust
pub enum Outcome
```

What running one behaviour against one adapter established.

Two variants and no third, because *this adapter cannot do that* and *this adapter did that* are
the only honest readings of a behaviour that did not fail. A boolean here would collapse them.

### Variants

- `Held` - The behaviour ran and holds.
- `Declined` - The adapter cannot satisfy it, and said which way.

### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

## `enum Declination`

```rust
pub enum Declination
```

Why an adapter declined a behaviour.

Typed rather than a message, for the reason every refusal in this workspace is: a reader that
matched on the text would be depending on the text. One variant today; a second arrives with the
behaviour that can be declined.

### Variants

- `OffersNoPreFlight` - `dry_run` answered `NotAsked` for every case, so nothing was checked before the rows were read.

### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

## `struct Spent`

```rust
pub struct Spent
```

What one cell of the matrix cost, measured rather than stated.

**`docs/adr/0012` said per-pack timings were reported *from the start* and nothing measured
one** (`telekom/sutura#353`). The record's own argument for having them is the one that governs
every number in this repository: a conformance matrix grows multiplicatively - adapters times
behaviours times cases - so *the tier that is supposed to be fast stops being fast quietly*, and
the fast tier is defended with a measurement or it is defended with a feeling.

# Two numbers, and the seam between them is where the multiplication is

`execute_packs` calls the binding's `open` once per BEHAVIOUR, so the fixture - opening the
adapter and attaching the corpus - is paid once per emitted test rather than once per binding.
That is deliberate, because no state may cross between tests, and it is also the term that
grows fastest - so one total would hide the thing a reader needs: whether a slow cell is a slow
behaviour or a slow fixture paid six times.

# It cannot be fabricated, which is why it is a type

The fields are private and both constructors MEASURE. A `Duration` parameter would have let a
caller report a number nobody took, which is the shape this repository has already paid for: a
count in a message is not a witness.

# What it does not reach, next to the claim

**Nothing joins two adapters' numbers.** A pack is a behaviour name shared across adapters, and
each binding is its own test binary in its own crate - under nextest each test is its own
PROCESS - so no value here can see another binding's. Aggregating per pack ACROSS adapters
needs a reader of a run's machine-readable output, which is a gate rather than a measurement;
`docs/adr/0012` carries that split. **And nothing thresholds any of this**: a budget with no
run beside it cannot be re-taken, so the report is the deliverable and a budget comes second
with its own measurement.

### Methods

```rust
pub fn building<W>(open: impl FnOnce() -> W) -> Self
```

The fixture alone: what `census` measures, because it runs no behaviour.

```rust
pub const fn fixture(self) -> core::time::Duration
```

What the fixture cost.

```rust
pub fn measuring<W, T>(open: impl FnOnce() -> W, run: impl FnOnce(&W) -> T) -> (T, Self)
```

Builds the fixture, runs the behaviour against it, and reports what each cost.

Both halves are measured here rather than by the caller, so a cell's numbers and the work
they are about cannot be paired wrongly and the split is the same split in every binding.

```rust
pub const fn pack(self) -> core::time::Duration
```

What the behaviour cost, once the fixture was standing.

### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `PartialEq`

## `enum Fault`

```rust
pub enum Fault<E>
```

Why a behaviour did not hold.

Generic in the adapter's own error, so a data system's typed failure survives to the report
instead of being flattened into a sentence at the pack boundary. Every variant names the case, so
a corpus of many says which one.

### Variants

- `NotAnswered` - The data system did not answer at all.
- `Labels` - The answer is not labelled the way the plan projects it.
- `Content` - The rows are not the reference's rows.
- `Order` - The rows are not in the order the plan claims.
- `PreFlightRefused` - The pre-flight refused a plan the adapter is expected to be able to execute.
- `AcceptedThenDidNotAnswer` - The pre-flight accepted the plan and the execution then failed.
- `ALegWasAnswered` - An adapter that declares it does not execute a leg executed one.
- `EmptyCorpus` - The corpus has no cases, so nothing could be asked.

### Implements

`Debug`, `Display`, `Error`

## `fn hold`

```rust
pub fn hold<E>(adapter: &str, behaviour: Behaviour, conformed: Conformed<E>, spent: Spent)
```

Reports one behaviour with what it cost, and fails the test if it did not hold.

One of the two places in this crate that end a test - `not_here` is the other, for a reason
that is not about the adapter at all - so what a failure prints is decided once: the adapter,
the behaviour, the cost, and the whole cause chain. `Display` on a `thiserror` enum
prints the outermost message and stops, and the outermost message here is the pack's - what
tells a rejected statement from an outage is one and two levels down.

**The cost is on the failing line too**, and that is not symmetry for its own sake: a cell that
failed in two milliseconds and one that failed after thirty seconds are different diagnoses, and
the second is the one `docs/adr/0012` says goes quiet.

## `fn not_here`

```rust
pub fn not_here(adapter: &str, behaviour: Behaviour, missing: &Missing, spent: Spent, declared: Option<&str>)
```

Reports a behaviour that did not run, because this venue could not stand the fixture up.

**`NOT RUN` in the first column, and not `DECLINED`**, for the reason `Fixture`'s header
gives: one word is about the adapter and the other is about the environment, and a reader who
could not tell them apart would read a green run over an absent Postgres as a green run against
one. `.config/nextest.toml` already keeps this line on a green run - it scopes
`success-output` by BINARY, so `binary(conformance)` covers it with no second edit.

**Which venue may skip is deliberately not this crate's to choose, and where a venue said it
provisioned a tier this REFUSES.** `sutura_dev::requirement` decides skip-or-fail once for
every harness in this repository, from `REQUIRE_TIER`, and only the thing that provisioned a
tier sets it - so an honest fixture in that venue never reaches here at all, because
`sutura_dev::provisioned::here` has already failed the run. What does reach here is a fixture
that answered `Fixture::Absent` without asking, and `absence_is_impossible` is what makes
that cost something rather than taking a whole tier quiet and green. Everywhere else - a machine
that provisioned nothing - it prints and returns, which is the fail-OPEN direction that module
decided and this one does not re-decide.

## `fn conduct`

```rust
pub fn conduct<W, E>(adapter: &str, behaviour: Behaviour, declared: Option<&str>, open: impl FnOnce() -> Fixture<W>, pack: impl FnOnce(&W) -> Conformed<E>)
```

Builds the fixture, runs the behaviour where the environment stood one up, and reports either way.

**The one place a cell's endings are decided**, which is why the test `execute_packs` emits is
a single call to this: HELD or DECLINED through `hold`, `NOT RUN` through `not_here`, and a
`Fault` is `hold`'s panic.

Both halves of `Spent` are measured around the work they are about, so the pack half of an
absent fixture is a measurement of nothing rather than a number nobody took - and the fixture
half is still real, because asking a provisioner and being told no costs something.

The pack arrives as `impl FnOnce(&W) -> Conformed<E>` and every binding hands over a FUNCTION
ITEM, which is what keeps `clippy::result_large_err` off the adapter's own crate: that lint
inspects a closure's return type at its definition site, and the one closure this needs is
defined here, generic in the adapter's error, rather than six times per binding.

## `fn census`

```rust
pub fn census<W>(adapter: &str, bound: &[Behaviour], declared: Option<&str>, open: impl FnOnce() -> Fixture<W>)
```

What a binding actually covered, asserted and printed - with the cost of covering it.

Five things, and the first is the one a review had to correct:

1. **the behaviours the binding actually emitted tests for are `Behaviour::EVERY`**. `bound`
   is not a second hand-written list: `execute_packs` generates it from the same repetition
   that generates the `#[test]`s, one element per emitted test, so a deleted test is a deleted
   element and this comparison reddens. The version this replaced compared two lists nobody had
   tied together, and deleting the content behaviour's test left every census green;
2. the corpus is not empty, which is the state that would make every behaviour above vacuously
   green;
3. the counts are PRINTED - behaviours, cases, and which direction the leg declaration selected.
   A suite that reports a ratio it has not earned is the failure this repository has already met
   twice, and `.config/nextest.toml`'s second override is what makes this line survive a green
   run instead of being captured and discarded;
4. **the per-adapter FLOOR is printed, from a measurement.** `docs/adr/0012` asks for timings
   aggregated per pack and per adapter (`telekom/sutura#353`); this is the per-adapter half that
   a test process can actually take. `execute_packs` rebuilds the fixture once per behaviour,
   so `behaviours x fixture` is the cost this binding pays before a single assertion runs - the
   multiplicative term the record's *stops being fast quietly* is about. It is derived from the
   same `bound` slice the comparison above uses, so the multiplier is the number of tests that
   were actually emitted rather than a constant beside it;
5. **a venue where the fixture did not stand up prints NO coverage line at all.** It takes the
   `open` path rather than a `Spent` for exactly this: a census that printed *6 behaviour(s)
   over the corpus's cases* beside six cells that each reported `NOT RUN` is the skip that reads as
   coverage, which is the failure mode the packs were built against. What it prints instead
   names the count as one that asserted nothing, and carries the provisioner's diagnostic. The
   two assertions above it still run, because what a binding emitted and whether the corpus has
   cases are facts about this tree rather than about this venue.

What it cannot do: know that a behaviour's BODY asserts anything. A pack that returned `Ok`
unconditionally passes every census, which is what `tests/bound.rs`'s fault half is for. And
the floor is a FLOOR: it is not the tier's cost, it says nothing about another adapter's cells,
and no gate reads it - see `Spent` for why each of those is deliberate.

## `use sutura_domain`

The domain, re-exported so `execute_packs` can name the port without the consuming crate
having to depend on `sutura-domain` under that spelling.

A `macro_rules!` body resolves item paths at the EXPANSION site, so a bare `sutura_domain::` in
the expansion would compile only for a consumer that happens to have that dependency under that
name. `$crate::sutura_domain` always resolves.

## `use Fixture`

An adapter's fixture, or the reason this venue could not stand one up.

**The type every binding's `open` path returns, and it is the mechanism rather than a
convention.** `crate::execute_packs` used to call `open` for a `W`, so an adapter whose data system
may not be reachable here had exactly one option - panic in its fixture - and therefore could
not be bound at all: `sutura-exec-postgres` was registered in the golden matrix and carried the
one declared exemption in `cargo xtask check-conformance-bindings` for precisely that reason
(`telekom/sutura#348`).

**What the return type buys, stated exactly, because the sentence that stood here read wider
than the mechanism.** It forces a VALUE, not a question: `Fixture::standing(connect().unwrap())`
asks nothing and PANICS, which is loud and fail-closed; `Fixture::Absent(Missing::tier(s, &".."))`
asks nothing and is silent in the two venues named in this module's header. So what a binding
cannot do is leave the two cases unconsidered - a fixture returning `W` does not compile - and
what it can still do is answer either one dishonestly. That is one line, in a file whose whole
content is a fixture and a declaration, and the diff is where it is read.

# Why this is not an `crate::Outcome`, which is the distinction the design turns on

`crate::Outcome::Declined` is a statement about the ADAPTER - *this adapter cannot do that*, carrying
a typed `crate::Declination`. An absent tier is a statement about the ENVIRONMENT. Collapsing the two
would make a green run over an absent Postgres indistinguishable from a green run against one,
which is the failure mode the packs were built against. So the two are reported under different
words (`crate::hold` prints `DECLINED`, `crate::not_here` prints `NOT RUN`) and decided at different
levels: a declination comes out of a pack that RAN, and an absence stops the pack running.

# What it does NOT establish

See this module's header: nothing here can tell an absence that was DISCOVERED from one that was
merely declared, and the reason the harness cannot is a dependency rule that has its own gate.

## `use Missing`

Why this venue could not stand a fixture up. **About the environment, never about the adapter.**

Typed rather than a message, for the reason every refusal in this workspace is: a reader that
matched on the text would be depending on the text. One variant today - a second arrives with
the first adapter whose absence is not a tier, and cloud state a run cannot create is the shape
that asks for it. It arrives WITH that adapter rather than ahead of it, because a variant
nothing constructs is a claim nothing provokes, and this crate has paid for one of those already
(`crate::Fault::EmptyCorpus`, which needed a seam before it was reachable at all).

## `use REQUIRE_TIER`

The variable a provisioner sets when it has brought a tier up, spelled here as well.

**`sutura_dev::requirement::FORCE`'s name, duplicated, and the duplication is PINNED rather than
hoped about.** This crate may not take `sutura-dev` through a normal dependency -
`xtask/src/boundaries/harness.rs` holds it to `sutura-domain` alone - so the name and its
truthiness are spelled twice, and two statements about one fact can disagree.
`tests/bound.rs`'s `the_requirement_this_harness_reads_is_the_one_the_provisioner_writes` is the
mechanism that keeps them equal: it takes `sutura-dev` as a DEV-dependency, which that gate
permits by design (what may not happen is a pack BODY compiled against something, and a pack
body is `src/`), and compares both halves against `FORCE` and `requirement::decide`.

## `use a_tier_is_required`

Whether an absent tier is a failure here, decided over the VALUE rather than the environment.

Over the value for the reason `sutura_dev::requirement::decide` is: an environment read is not
testable across a threaded runner, and this is the half a test has to be able to compare.

**The falsy spellings are a COPY and the owner is `sutura_dev::requirement::NOT_REQUIRED`**,
because that crate cannot be reached from here through a normal dependency. The copy is not
held by the eye: `tests/bound.rs` iterates the owner's list, so a spelling added there fails
this crate's own cell until this line agrees. Review found the version before that - a fixed
array of eleven values chosen HERE - and named the scenario: add `"off"`, the obvious next
spelling for a variable people set by hand, and `SUTURA_DEV_REQUIRE_TIER=off` means *optional*
to `provisioned::here`, which skips, and *required* here, which then refuses the absence that
skip produced.

## `use absence_is_impossible`

Whether a DECLARED absence is a defect here rather than a skip.

**Pure, over the value, because the alternative is not available and would be wrong anyway.**
`unsafe_code` is `forbid` across this workspace and `std::env::set_var` is `unsafe` on Rust
2024, so a test cannot manipulate the environment here at all - and
`sutura_dev::requirement`'s own tests refuse to do it for the second reason, which is that it
races across a threaded runner. So the decision is a value every caller passes down from
`declared_here`, which is what lets `tests/bound.rs` provoke the refusal end to end, message
included, in both endings and in either direction.

**An exhaustive `match` and not a `matches!`, and the difference is the whole of this claim.**
`REQUIRE_TIER` is a statement about TIERS, so the variant that arrives for cloud state a run
cannot create has to decide its own direction - and a `matches!` gave it one by omission:
`false`, silently, with `cargo check --all-features` exit 0. That is this branch's own hole
reopened one adapter later and inside the venue this crate says is closed - a fixture answering
`Absent(Cloud)` without asking anything, in `checks.nextest`, which sets the variable. Measured
with the refusal absent: `21 tests run: 21 passed`, the only tell printed lines nobody diffs.

With the `match` a new variant does not compile until somebody writes its arm, so the fail-open
direction cannot be chosen by not looking. **No test asserts that and none can** - a compile
error is not an outcome libtest has - so the evidence is the mutation, re-taken on 2026-09-06:
adding a `Missing::Cloud` variant made `just lint` fail with
`E0004` - a pattern for the new variant not covered - at this arm, where the same mutation
against the `matches!` version was exit 0.

## `use declared_here`

What this venue declared about tiers, read from the environment.

**The only environment read in this crate, and everything below it takes the VALUE.** That is
what makes the reporters testable at all, and it was measured rather than reasoned about: with
the read inside `not_here` and `census`, `just validate` refused two of THIS crate's own cells -
`checks.nextest` provisions the Postgres tier and sets the variable, and a fake absence in a
fake venue is indistinguishable from a fabricated one. `unsafe_code` is `forbid` across this
workspace and `std::env::set_var` is unsafe on Rust 2024, so no test can turn it off either. So
the macro reads it once per cell and hands it down, which is also the shape
`sutura_dev::requirement::decide` chose for the same reason.

## `type_alias Conformed`

What one behaviour of one pack answers.

## `macro compile_packs`

Binds the compile pack to one catalog, as one named `#[test]` per behaviour.

```ignore
// In this crate's own binding, `tests/compile.rs`.
use sutura_conformance::compile::{DeclaringSubject, GoldenSubject};

sutura_conformance::compile_packs! {
    adapter: golden,
    catalog: GoldenSubject,
    golden,
}

sutura_conformance::compile_packs! {
    adapter: declaring,
    catalog: DeclaringSubject,
    declaring,
}
```

# The `golden`/`declaring` tag

`macro_rules!` cannot read `SemanticCatalog::KIND`, so the kind is written at the binding the
way `crate::execute_packs`'s leg declaration is, and the arm's `const` assert is where the two
are torn unless they agree: tag a catalog against its own `CatalogKind` and the binding does
not build. The tag also selects WHICH behaviours the arm emits - a `golden` binding gets all six,
a `declaring` one gets `CompileBehaviour::UNIVERSAL` - and both feed `compile_census` the one
repetition's list, so either way a deleted cell reddens.

**The golden-only cells are additionally bound on `GoldenCatalog`, so the tag is a second line
of defence and not the mechanism**: a golden cell's function takes `C: GoldenCatalog`, so a
`golden` tag handed a catalog that does not implement the marker does not compile even before the
`KIND` assert. The split holds by the type system rather than by review, which is the whole point
of the marker.

# What a binding names

- `catalog` is the under-test type, named from this crate's `compile` module. The cells load it
  via `SemanticCatalog::load` and this crate supplies the corpus and the oracle.
- `adapter` is an ident that names the emitted module, so a binding can hold several catalogs and
  filter by name the way the execute pack allows.

## `macro execute_packs`

Binds the execute pack to one adapter, as one named `#[test]` per behaviour.

```ignore
#[cfg(test)]
mod conformance {
    fn open() -> Fixture<DuckDbWarehouse> { /* attach `corpus::on_disk()` */ }

    sutura_conformance::execute_packs! {
        adapter: duckdb,
        warehouse: sutura_exec_duckdb::DuckDbWarehouse,
        open: crate::conformance::open,
        executes_legs,
    }
}
```

# The four arguments

- `adapter` is an **ident**, not a string, and that is the whole of what decides `macro_rules!`
  over a proc-macro. A `literal` in `mod $name` position is `error: expected identifier, found
  metavariable`, and getting from a string to a path segment needs a paste-style crate. With an
  ident the expansion is `mod duckdb { #[test] fn .. }` and the naming scheme NESTS rather than
  concatenates, which is the one thing `macro_rules!` cannot do and the only reason a proc-macro
  would have been needed.
- `warehouse` is the adapter type. It is what the `const` assertion below reads the declaration
  off, and what `census` is instantiated at.
- `open` is a **path** to a function returning `Fixture`, called once per test so no state
  crosses between them. A path rather than a closure because a `macro_rules!` body resolves
  items at the expansion site: a closure naming a type the caller imported at file scope would
  not resolve inside the generated module, and a `crate::`-rooted path always does. The fixture
  lives in a `#[cfg(test)]` module because the strict lints exempt what is inside one, which is
  why the path in the example names that module. **The return type is `Fixture` and not `W`**,
  which is what lets an adapter needing a provisioned service be bound at all - see that type
  for why an absence is a value here rather than a panic, and why it is not an
  `Outcome::Declined`.
- the last tag is the adapter's leg **declaration**, `executes_legs` or `refuses_legs`.

# Why the declaration is written at the binding as well as on the adapter

`macro_rules!` cannot read an associated constant, so a macro cannot branch on
`Warehouse::EXECUTES_LEGS` to choose which test to emit. The tag is this crate's routing copy of
it, and the arm's `const _: () = assert!(..)` is where the two are torn unless they agree: tag an
adapter against its own declaration and the binding does not build. That is the same mechanism
`sutura-app`'s catalog registry uses for `SemanticCatalog::KIND`, and it is what makes a
declaration a thing the compiler checks rather than a list somebody keeps in step.

# Selecting a tier

The emitted names are `<adapter>::<behaviour>` inside whatever module the invocation sits in, so
the convention above - `tests/conformance.rs`, wrapped in `mod conformance` - gives
`conformance::duckdb::the_rows_are_the_reference_rows`. One adapter's tier is then
`cargo nextest run --workspace --all-features -E 'test(conformance::duckdb)'` and every adapter's is
`cargo nextest run --workspace --all-features -E 'binary(conformance)'`, which is what `just test` runs as part of the
workspace. **Both are enforced now** - `cargo xtask check-conformance-bindings` refuses a
registered adapter whose binding is in another file or another module, with the selector
COMPUTED from where the invocation sits, because those two properties are what the filters above
rest on. What it still cannot see is an emitted test: the evidence is a written invocation and
its position.

## Module `corpus`

The corpus the execute packs run: one table, four questions, and the answer written ONCE.

**Written once is the whole property.** Every registered adapter is asked the same plan and
compared against the same `Case::expected` rows, so *these two data systems answer this
question the same way* is a claim about the answer rather than about two hand-written
expectations that happen to agree. The comparison itself is
`sutura_domain::warehouse::agreement`'s, not this module's.

# The plan is built from domain types, and that is what keeps this crate portable

There is no catalog here and no compiler: a `QueryPlan` is a domain value, so the corpus can
state one directly. That is the reason a pack can be bound to an adapter that lives in its own
crate - the packs depend on the interior and on nothing else, so `sutura-exec-duckdb` can take
them as a dev-dependency without acquiring a catalog adapter, `sutura-semantic` or
`sutura-app`.

# What this corpus does NOT contain, stated so nobody reads it as the whole suite

- **The three cases `docs/adr/0012` names** - a filter on a remote dimension over an orphan key,
  a ratio whose denominator is zero for one subgroup, and a `CountDistinct` spanning two join
  keys. Each needs a second table and a federated plan; none is here.
- **An integral total strictly PAST `i64`, which is not a gap but an undecidable row.** Read off
  the three bound adapters: `sutura-exec-postgres` REFUSES one
  (`numeric_cell` parses an integral `NUMERIC` into an `i64` and errors rather than rounding),
  `sutura-exec-duckdb` answers `Value::Text` from its `HugeInt` arm, and
  `sutura-exec-datafusion`'s `sum` over an `Int64` column has nowhere wider to go at all. Three
  adapters, three different endings, so a corpus whose answer is written ONCE cannot hold that
  row - `total_wide_by_day` goes to the boundary and stops there. What the boundary still
  buys is in that case's own doc.
- **A fixed-point measure, and so the `Value::Text` arm all three adapters keep for one.**
  Unreachable from a CSV-backed corpus rather than omitted: no type inference on the load path
  produces a fixed-point column from a fractional literal - `read_csv_auto` and `DataFusion`'s
  inference both answer a 64-bit float, and `sutura-exec-postgres`'s own importer has no
  `NUMERIC` arm to reach. The fractional CLASS is exercised, as `Value::Real`
  (`total_rate_by_day`), and the day an inference answers a fixed-point type instead that
  cell reddens - which is the class comparison working rather than a case somebody has to write.
- **Files.** `docs/adr/0012`'s *the corpus is files, not code* is unbuilt: a case is a value in
  this module, so adding one is still a code change.

# Null placement in a group key: decided, and what the null row does and does NOT detect

**`ASC NULLS LAST`, everywhere, and it is not this corpus's choice to make.** `sutura_sql`'s
`ordered_nulls_last` states the placement in the AST for every dialect and
`sutura-exec-datafusion`, which renders no SQL at all, lands on the same placement because
`LogicalPlanBuilder::sort_by` is `Expr::sort(true, false)`.
`sutura_domain::plan::federated` calls it *the whole of the ordered-result contract*. So the
per-case opt-out this module used to say it owed is **not** owed: there is no case here whose
null order a conforming source may legitimately answer differently.

**What the null row does NOT detect, measured rather than assumed.** It is NOT a check on our
statement of the placement. The layer collapses `NULLS LAST` away for every target whose
default is already nulls-last, which is all three adapters bound to these packs - so deleting
`ordered_nulls_last`'s `nulls_first: Some(false)` leaves `DuckDB`'s and Postgres's rendered SQL
**byte-identical**, and the only dialect whose text changes is `BigQuery`, which has no binding
here. Their engines then order nulls last on their own. `crates/sutura-sql`'s
`every_order_by_states_nulls_last` is what holds our statement of it, and it holds the AST
rather than an answer.

**What it DOES buy, which is two things and neither is that one.** Through
`crate::Behaviour::Order` it pins the three engines' own default null ordering - most
usefully `datafusion`'s, whose `sort_by` supplies `nulls_first: false` from a default that a
version bump could change with no diff of ours - so it is a **dependency regression detector**.
Through `crate::Behaviour::Content` it is a claim about OUR code: a null key must be a GROUP
and not a row a join or a filter dropped, which is the failure class
`crates/sutura-app/tests/golden/data_systems.rs` names for a fact key.

### `struct Case`

```rust
pub struct Case
```

One question, and the answer to it.

Private fields with accessors, which is what a library crate here owes: a caller cannot assemble
a `Case` whose expected rows belong to a different plan.

#### Methods

```rust
pub const fn expected(&self) -> &RowSet
```

The rows every conforming adapter answers, in the order the plan's `ORDER BY` claims.

```rust
pub const fn name(&self) -> &'static str
```

The name a failure reports. Static, so a fault carries it without allocating.

```rust
pub const fn plan(&self) -> &QueryPlan
```

The plan to execute.

### `struct LegCase`

```rust
pub struct LegCase
```

One leg, and the answer to it.

Separate from `Case` because a leg is a different shape of executable rather than a different
question: it reads the same table over the same range and answers the same rows, which is what
makes it a conformance claim - an adapter that executes a leg must reach the answer a whole plan
reaches.

#### Methods

```rust
pub const fn expected(&self) -> &RowSet
```

```rust
pub const fn leg(&self) -> &LegPlan
```

```rust
pub const fn name(&self) -> &'static str
```

### `fn source`

```rust
pub fn source() -> sutura_domain::model::SourceName
```

The data system name every plan here resolves to.

An adapter opened under another name is handed a plan it does not own, and the packs do not
paper over that: it is the fixture's job to open the adapter as this source.

### `fn table`

```rust
pub fn table() -> sutura_domain::model::TableName
```

The table every case reads, as the data system knows it.

### `fn posture`

```rust
pub fn posture() -> sutura_domain::source::SourcePosture
```

The posture every adapter in this pack is opened with.

**Shared, and the packs cannot offer anything else today.** An adapter that CAN carry a
per-subject credential needs one minted for a real subject at a real source to be exercised as
such, which is leg 2 and is not built - so binding an impersonating adapter to these packs holds
it to the shared path only, and the pack says so rather than reporting a green that reads wider.
`docs/adr/0012` decides that impersonation gets no negative pack at all, and this is the same
boundary from the positive side.

### `fn presented`

```rust
pub fn presented() -> sutura_domain::identity::Presented
```

What a leg in these packs executes as.

Built from the same `declared` acknowledgement `posture` is, so the credential the pack
presents and the posture the fixture opened the adapter with cannot drift apart - which is
exactly the disagreement each adapter's exhaustive match on what it received exists to catch.
An adapter that compares the two witnesses fails here if a fixture opened it any other way.

### `fn csv`

```rust
pub const fn csv() -> &'static str
```

The corpus, as CSV, from the committed file under `corpus/`.

Served from the file rather than from a copied constant so a row edit in
`corpus/conformance_events.csv` is a change to the data and not to this module - which is the
whole of what "a case is a directory entry, not a function" asks. The bytes are embedded at
compile time by `include_str!`, so this stays `&'static str` and `const`.

The last row is outside every case's time range on purpose: a corpus whose filters exclude
nothing cannot tell an adapter that applied them from one that did not.

### `fn on_disk`

```rust
pub fn on_disk() -> std::path::PathBuf
```

The corpus on a filesystem, written once per process, as the path a fixture attaches.

**Inside THIS WORKTREE, and that is the whole of `telekom/sutura#405`'s first instance.** It
used to land on `<temp_dir>/sutura-conformance/<table>.csv` - a purpose and no key - and the
argument for the rename below was *the bytes are identical either side*, which is true per TREE
and not per machine. Reproduced on 2026-09-07 with two worktrees of this repository, each
running its own `on_disk`, one row differing: the `DuckDB` binding failed
`total-by-region-and-day` and `total-by-region-and-day-as-a-leg` as content faults naming this
corpus's own cases, while the run that overwrote the file was green. The window is wide because
`attach_csv` makes a VIEW over `read_csv_auto`, so the file is read at QUERY time; the Postgres
binding reads it seven times per binding at LOAD time.

A path under the worktree needs no key, because the worktree is the key - the same answer
`sutura_dev::scope::Scope::state_dir` gives, and `tests/bound.rs` pins the two spellings
together against that type rather than leaving a comment claiming they agree. This crate may not
reach `sutura-dev` through a normal dependency (`xtask/src/boundaries/harness.rs`), so the
SPELLING is duplicated and the AGREEMENT is mechanical.

Written under a per-process temporary name and RENAMED onto the shared one, which is still
needed and now means something narrower: `nextest` gives each test its own process, so several
processes of THIS worktree write this path at once, and a reader must not see a half-written
file. Those processes write identical bytes - which is the claim the old path could not make.

### `fn cases`

```rust
pub fn cases() -> Vec<Case>
```

Every question in the corpus.

A `Vec` rather than a constant, because a `QueryPlan` owns its strings and none of these types
is `const`-constructible. The count is what `crate::census` reports, so a corpus that lost its
cases is a failure rather than a fast green.

### `fn leg_case`

```rust
pub fn leg_case() -> LegCase
```

The one leg in the corpus.

### `constant TABLE`

The one table every case reads.

## Module `execute`

The execute pack: what every implementor of the execution port must do with a plan.

**Every body here is generic in the port and mentions no adapter.** That is the property
`docs/adr/0012` is built on: an assertion that appears twice will disagree with itself, and the
disagreement will be read as a difference between two data systems rather than as a difference
between two copies of a test. An adapter contributes a constructor, never an assertion.

Each function is one behaviour and returns `Conformed`, so there are exactly three outcomes and
a reader can tell them apart: the behaviour HELD, the adapter DECLINED it and said why, or a
typed `Fault` names what disagreed. `crate::execute_packs` gives each one a `#[test]` name
per adapter; nothing here panics, so a pack can also be called directly.

# What this pack does not reach

- **Ordering inside a leg.** A `sutura_domain::plan::LegPlan` carries no row limit and
  no statement that an order was promised, so `a_leg_is_executed` asserts content only -
  `sutura_domain::warehouse::agreement`'s own header says a leg comparing against a plan that
  claimed no order should not call the order assertion.
- **Impersonation, in either direction.** `docs/adr/0012` decides that a declared absence of
  impersonation gets no pack: the fallback a negative one would assert as correct is the one
  `docs/adr/0008` forbids, and the direction worth worrying about is not observable from this
  port at all. The mechanism is the boot refusal, tested over a composition root.
- **Which error an adapter refused with.** `Self::Error` is the adapter's own type, so a pack
  sees only that a call failed. `a_leg_is_refused` is written around that limit rather than
  through it - see its own doc.

### `fn labels_are_the_plans_own`

```rust
pub fn labels_are_the_plans_own<W>(warehouse: &W) -> crate::Conformed<<W as >::Error>
```

The answer's labels are the ones the plan projects, in the order it projects them.

A separate behaviour from the two below rather than a consequence of them, because it is a
separate diagnosis: `result_labels` is the domain's single statement of what a result carries, so
an adapter that renamed or reordered a column has a defect in its projection rather than a wrong
number. `agree_on_content` would also catch it and would report it as a content disagreement,
which is the less useful of the two readings.

### `fn content_agrees_with_the_reference`

```rust
pub fn content_agrees_with_the_reference<W>(warehouse: &W) -> crate::Conformed<<W as >::Error>
```

The rows are the reference's rows, as a multiset.

THE conformance claim: one plan, one answer, whatever executed it. A multiset rather than a set,
because a duplicated row is exactly what a fan-out defect produces.

### `fn order_agrees_with_the_reference`

```rust
pub fn order_agrees_with_the_reference<W>(warehouse: &W) -> crate::Conformed<<W as >::Error>
```

The rows are in the order the plan's `ORDER BY` claims.

Separate from `content_agrees_with_the_reference` and asked after it, which is the domain
policy's own instruction: the first symptom of a wrong number would otherwise be reported as a
sort order. Every plan in the corpus groups, so every one of them emits an order to claim.

### `fn one_plan_asked_twice_answers_the_same_way`

```rust
pub fn one_plan_asked_twice_answers_the_same_way<W>(warehouse: &W) -> crate::Conformed<<W as >::Error>
```

One plan, asked twice, answered the same way twice.

The weakest behaviour in the pack and the only one that needs no reference, which is why it is
kept: it is what the reference comparison degenerates to for an adapter whose rows are right and
whose plan is non-deterministic - an unstable tie order, a cached result that went stale, a
connection that reset the session between calls. Both halves are compared, so a stable answer in
an unstable order is still a failure.

### `fn a_preflight_that_accepts_is_followed_by_an_answer`

```rust
pub fn a_preflight_that_accepts_is_followed_by_an_answer<W>(warehouse: &W) -> crate::Conformed<<W as >::Error>
```

A pre-flight that accepted the plan is followed by an answer.

The port states that the check and the execution take the same
`Executable` *"so the two cannot disagree about what this adapter accepts"*, and this is that
sentence as an assertion. Two directions are faults: a pre-flight that refuses a plan the adapter
can execute, and one that accepts a plan the adapter then cannot answer.

**An adapter that answers `PreFlight::NotAsked` DECLINES this behaviour**, and the declination
is the honest reading rather than a pass: nothing was checked, so nothing about the check has
been established. The limit worth stating next to it - the declination is observed at run time
rather than read off a typed declaration, because the port has no capability constant for a
pre-flight the way it has one for a leg. Where that constant exists the pack would select on it
and a mismatched declaration would not build.

### `fn a_leg_is_executed`

```rust
pub fn a_leg_is_executed<W>(warehouse: &W) -> crate::Conformed<<W as >::Error>
```

A leg reaches the answer a whole plan reaches.

Selected for an adapter that declares `EXECUTES_LEGS`. The leg reads the same table over the same
range as one of the whole-plan cases and must answer that case's rows, which is what makes it a
conformance claim rather than a smoke test: an adapter with a leg-rendering path of its own has
to land on the number the whole-plan path lands on.

Content only. See this module's header for why a leg gets no order assertion.

### `fn a_leg_is_refused`

```rust
pub fn a_leg_is_refused<W>(warehouse: &W) -> crate::Conformed<<W as >::Error>
```

A leg is refused by an adapter that declares it does not execute one.

Selected for an adapter that leaves `EXECUTES_LEGS` at its default. This is the direction
`docs/adr/0012` calls *a declared absence with something to try*: a leg IS built and executed
on the shipped answer path now, so what this direction is worth is narrower and still real -
it is the only thing that exercises the guard of an adapter with no leg venue of its own -
and an adapter that quietly computed one instead would be surfacing half an answer under a
certified metric name.

**A whole plan is executed FIRST, and that is not a warm-up.** A pack cannot see which error an
adapter refused with - `Self::Error` is the adapter's own type - so on its own this behaviour
would be green for an adapter that failed for any reason at all, including one that cannot reach
its data system. Answering a whole plan immediately before is what makes the refusal evidence
about the leg. The residual limit: the refusal is still only *an* error, so an adapter that
refused a leg for the wrong reason passes.

### `fn a_leg_is_refused_over`

```rust
pub fn a_leg_is_refused_over<W>(warehouse: &W, cases: &[crate::corpus::Case]) -> crate::Conformed<<W as >::Error>
```

The same behaviour, over cases a caller supplies.

**A seam, and a narrow one, for the branch above it cannot otherwise reach.**
`crate::Fault::EmptyCorpus` is what stops this behaviour being green over nothing - the guard
`crate::census` provides for every other behaviour and the one place it is a `Fault` instead -
and with the corpus reached through `corpus::cases` alone no fake could empty it, so the
variant was unprovokable and the claim *every fault is provoked* was seven of eight.

It is the beginning of what a file-backed corpus needs anyway: a corpus the pack is handed
rather than one it calls. Every other behaviour still reads `corpus::cases` directly, so this
is one seam and not a parameter threaded through the pack.

## Module `venue`

Whether this environment can stand a fixture up, and what a DECLARED absence costs.

Split out of the crate root because that file reached the 1000-line ceiling `cargo xtask
max-lines` holds, and the rule this repository applies to a threshold lint applies to itself:
split the file rather than raise the number. The split is by TASK rather than by size - every
item here answers *is the thing this adapter needs even here*, and nothing here knows what a
behaviour is.

# The distinction the whole module exists for

`crate::Outcome::Declined` is a typed statement about the ADAPTER - *this adapter cannot do
that*. An absent tier is a statement about the VENUE. Collapsing the two would make a green run
over an absent Postgres indistinguishable from a green run against one, which is the failure
mode the packs were built against, so they are reported under different words and decided at
different levels: a declination comes out of a pack that RAN, and an absence stops the pack
running.

# Which venue may skip is not this crate's decision, and a DECLARED absence is not free

`sutura_dev::requirement` decides skip-or-fail once for every harness in this repository, from
`REQUIRE_TIER`, and only the thing that provisioned a tier sets it - so an honest fixture in
such a venue never reports an absence at all, because `sutura_dev::provisioned::here` has
already failed the run. What CAN reach here is a fixture that answered `Fixture::Absent`
without asking, and `absence_is_impossible` is what makes that cost something.

### `enum Fixture`

```rust
pub enum Fixture<W>
```

An adapter's fixture, or the reason this venue could not stand one up.

**The type every binding's `open` path returns, and it is the mechanism rather than a
convention.** `crate::execute_packs` used to call `open` for a `W`, so an adapter whose data system
may not be reachable here had exactly one option - panic in its fixture - and therefore could
not be bound at all: `sutura-exec-postgres` was registered in the golden matrix and carried the
one declared exemption in `cargo xtask check-conformance-bindings` for precisely that reason
(`telekom/sutura#348`).

**What the return type buys, stated exactly, because the sentence that stood here read wider
than the mechanism.** It forces a VALUE, not a question: `Fixture::standing(connect().unwrap())`
asks nothing and PANICS, which is loud and fail-closed; `Fixture::Absent(Missing::tier(s, &".."))`
asks nothing and is silent in the two venues named in this module's header. So what a binding
cannot do is leave the two cases unconsidered - a fixture returning `W` does not compile - and
what it can still do is answer either one dishonestly. That is one line, in a file whose whole
content is a fixture and a declaration, and the diff is where it is read.

# Why this is not an `crate::Outcome`, which is the distinction the design turns on

`crate::Outcome::Declined` is a statement about the ADAPTER - *this adapter cannot do that*, carrying
a typed `crate::Declination`. An absent tier is a statement about the ENVIRONMENT. Collapsing the two
would make a green run over an absent Postgres indistinguishable from a green run against one,
which is the failure mode the packs were built against. So the two are reported under different
words (`crate::hold` prints `DECLINED`, `crate::not_here` prints `NOT RUN`) and decided at different
levels: a declination comes out of a pack that RAN, and an absence stops the pack running.

# What it does NOT establish

See this module's header: nothing here can tell an absence that was DISCOVERED from one that was
merely declared, and the reason the harness cannot is a dependency rule that has its own gate.

#### Variants

- `Standing` - It stood up. What an in-process or in-memory adapter always answers.
- `Absent` - The environment this adapter needs is not here, so nothing was asked of it.

#### Methods

```rust
pub const fn missing(&self) -> Option<&Missing>
```

The reason it did not, where it did not.

```rust
pub const fn standing(warehouse: W) -> Self
```

It stood up.

Named rather than left to the variant, so an in-process binding's last line reads as the
answer it is and the two answers are spelled at the same length.

### `enum Missing`

```rust
pub enum Missing
```

Why this venue could not stand a fixture up. **About the environment, never about the adapter.**

Typed rather than a message, for the reason every refusal in this workspace is: a reader that
matched on the text would be depending on the text. One variant today - a second arrives with
the first adapter whose absence is not a tier, and cloud state a run cannot create is the shape
that asks for it. It arrives WITH that adapter rather than ahead of it, because a variant
nothing constructs is a claim nothing provokes, and this crate has paid for one of those already
(`crate::Fault::EmptyCorpus`, which needed a seam before it was reachable at all).

#### Variants

- `Tier` - A service this adapter reaches over a socket, which nothing has provisioned here.

#### Methods

```rust
pub fn tier(service: &str, diagnostic: &impl core::fmt::Display) -> Self
```

A tier this venue has not provisioned, carrying the provisioner's own diagnostic.

The diagnostic arrives as a `Display` rather than as a `String`, so what reaches a reader is
the sentence the provisioner wrote rather than one a binding composed beside it. That
remedy is derived per venue and three checks hold it; a binding restating it would be a
fourth copy with no mechanism.

#### Implements

`Clone`, `Debug`, `Display`, `Error`

### `fn declared_here`

```rust
pub fn declared_here() -> Option<String>
```

What this venue declared about tiers, read from the environment.

**The only environment read in this crate, and everything below it takes the VALUE.** That is
what makes the reporters testable at all, and it was measured rather than reasoned about: with
the read inside `not_here` and `census`, `just validate` refused two of THIS crate's own cells -
`checks.nextest` provisions the Postgres tier and sets the variable, and a fake absence in a
fake venue is indistinguishable from a fabricated one. `unsafe_code` is `forbid` across this
workspace and `std::env::set_var` is unsafe on Rust 2024, so no test can turn it off either. So
the macro reads it once per cell and hands it down, which is also the shape
`sutura_dev::requirement::decide` chose for the same reason.

### `fn a_tier_is_required`

```rust
pub fn a_tier_is_required(forced: Option<&str>) -> bool
```

Whether an absent tier is a failure here, decided over the VALUE rather than the environment.

Over the value for the reason `sutura_dev::requirement::decide` is: an environment read is not
testable across a threaded runner, and this is the half a test has to be able to compare.

**The falsy spellings are a COPY and the owner is `sutura_dev::requirement::NOT_REQUIRED`**,
because that crate cannot be reached from here through a normal dependency. The copy is not
held by the eye: `tests/bound.rs` iterates the owner's list, so a spelling added there fails
this crate's own cell until this line agrees. Review found the version before that - a fixed
array of eleven values chosen HERE - and named the scenario: add `"off"`, the obvious next
spelling for a variable people set by hand, and `SUTURA_DEV_REQUIRE_TIER=off` means *optional*
to `provisioned::here`, which skips, and *required* here, which then refuses the absence that
skip produced.

### `fn absence_is_impossible`

```rust
pub fn absence_is_impossible(missing: &Missing, forced: Option<&str>) -> bool
```

Whether a DECLARED absence is a defect here rather than a skip.

**Pure, over the value, because the alternative is not available and would be wrong anyway.**
`unsafe_code` is `forbid` across this workspace and `std::env::set_var` is `unsafe` on Rust
2024, so a test cannot manipulate the environment here at all - and
`sutura_dev::requirement`'s own tests refuse to do it for the second reason, which is that it
races across a threaded runner. So the decision is a value every caller passes down from
`declared_here`, which is what lets `tests/bound.rs` provoke the refusal end to end, message
included, in both endings and in either direction.

**An exhaustive `match` and not a `matches!`, and the difference is the whole of this claim.**
`REQUIRE_TIER` is a statement about TIERS, so the variant that arrives for cloud state a run
cannot create has to decide its own direction - and a `matches!` gave it one by omission:
`false`, silently, with `cargo check --all-features` exit 0. That is this branch's own hole
reopened one adapter later and inside the venue this crate says is closed - a fixture answering
`Absent(Cloud)` without asking anything, in `checks.nextest`, which sets the variable. Measured
with the refusal absent: `21 tests run: 21 passed`, the only tell printed lines nobody diffs.

With the `match` a new variant does not compile until somebody writes its arm, so the fail-open
direction cannot be chosen by not looking. **No test asserts that and none can** - a compile
error is not an outcome libtest has - so the evidence is the mutation, re-taken on 2026-09-06:
adding a `Missing::Cloud` variant made `just lint` fail with
`E0004` - a pattern for the new variant not covered - at this arm, where the same mutation
against the `matches!` version was exit 0.

### `constant REQUIRE_TIER`

The variable a provisioner sets when it has brought a tier up, spelled here as well.

**`sutura_dev::requirement::FORCE`'s name, duplicated, and the duplication is PINNED rather than
hoped about.** This crate may not take `sutura-dev` through a normal dependency -
`xtask/src/boundaries/harness.rs` holds it to `sutura-domain` alone - so the name and its
truthiness are spelled twice, and two statements about one fact can disagree.
`tests/bound.rs`'s `the_requirement_this_harness_reads_is_the_one_the_provisioner_writes` is the
mechanism that keeps them equal: it takes `sutura-dev` as a DEV-dependency, which that gate
permits by design (what may not happen is a pack BODY compiled against something, and a pack
body is `src/`), and compares both halves against `FORCE` and `requirement::decide`.

## Module `compile`

The compile pack: what a metadata catalog must do with a question, end to end.

`crate::execute` holds a data system to the answer it gives for a plan. This pack holds a
metadata catalog to the shape its own definitions give a question: load its
`PinnedDefinitions`, run the question through `sutura_semantic::compile`, and hold the outcome
to the oracle - or, for a catalog that declares it supplies part of the model, to its own
declaration. It is the compile half of issue telekom/sutura#349's conformance rack, and it is a
REAL module in exactly the sense `Behaviour` is one for the execute pack: a
deleted cell must redden rather than quietly shrink a green count.

# How it mirrors `crate::execute_packs`

The same four mechanisms, so a reader who knows one pack knows the other:

| Execute pack | This pack |
| --- | --- |
| `Behaviour` + `EVERY` + `index` + the const assert | `CompileBehaviour` + `EVERY` + `index` + the const assert |
| the `#[test]`s and `census`'s `bound` are ONE repetition inside `execute_packs!` | the `#[test]`s and `compile_census`'s `bound` are ONE repetition inside `compile_packs!` |
| `EXECUTES_LEGS` declaration, chcked by a `const` assert | `SemanticCatalog::KIND` declaration, checked by a `const` assert |

And the one difference is what makes this pack a pair rather than a copy: **the GOLDEN/DECLARING
split** `CatalogKind` and `docs/adr/0016` draw. A catalog is held to the oracle only if it
declares itself *golden* - it can produce the whole model - which is enforced by a marker trait
bound, not by review (see `GoldenCatalog`).

# What a green run does NOT establish

- **Federated rendering.** The corpus is one source, one metric, one mono plan; nothing
  federates, so `Compiled::Federated` is a panic here rather than a case.
- **A live source, or identity forwarding.** Issue #349's stated surviving limits: the fixture is
  a hand-built catalog in this module, and no credential is minted for anything.
- **Every dialect.** The corpus declares it renders `DuckDb` and `Postgres`, and what
  `generate` produces for them is what `statement_is_the_oracles_own`
  compares. `ClickHouse` and `BigQuery`
  are out of scope here, which is why their renderings are not pinned.
- **That the corpus is hard.** It is one aggregate over one metric plus four refusals - see
  `questions` below for why none of the execute corpus's harder shapes is repeated here.

### `enum CompileBehaviour`

```rust
pub enum CompileBehaviour
```

One behaviour of the compile pack: the unit a test name, a failure report and a CI filter key on.

An enum rather than a string, for the reason `crate::Behaviour` is one: the pack's own list and
the tests the macro emits are compared by the compiler at one end and by `compile_census` at
the other.

#### Variants

- `Plan` - The compiled mono plan's serialized form equals the oracle's.
- `Statement` - For each dialect this corpus declares it renders to, the statement equals the oracle's.
- `Params` - The bind parameters equal the oracle's.
- `Refusal` - The question this corpus expects to refuse reaches the variant it names, and not another.
- `Fidelity` - What the catalog declared is exactly what its bundle produced.
- `Repeat` - A second load produces the same digest.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The name a report carries.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `trait GoldenCatalog`

```rust
pub trait GoldenCatalog
```

The marker that separates a **golden** catalog from a declaring one in this pack.

The analogue of `crate::tests::golden::catalogs::GoldenCatalog` in miniature: the golden-only
cell functions are bound on this marker, so a cell that requires the whole model cannot be
expanded for a catalog that does not implement it - the call does not typecheck. It is the
ROUTING copy of `SemanticCatalog::KIND` (`macro_rules!` cannot read an associated constant, so
the same fact is stated here in the form a binding can be bound on) and `compile_packs!`'
`const` assert is where the two are torn unless they agree. The canonical declaration of the kind
is `SemanticCatalog::KIND` in the domain.

### `enum FixtureError`

```rust
pub enum FixtureError
```

A catalog that could not be read, or a fixture that could not be put together.

The fixture catalogs in this module are `SemanticCatalog`s whose `Error` this fills, and the
corpus builders return it too, so every `.parse`/`.assemble`/`.pin` in a fixture travels through
`?` and only the pack boundary turns it into a panic.

#### Variants

- `Names` - A name failed to parse as an identifier.
- `Values` - A dimension value or anchor value failed to parse.
- `Date` - A date failed to parse or did not exist.
- `Range` - A time range did not hold together.
- `Version` - A version label was not a version.
- `Definitions` - A definitions bundle was inconsistent at assembly.
- `Digest` - The bundle would not digest.
- `Knowledge` - The knowledge and its declaration disagreed.

#### Implements

`Debug`, `Display`, `Error`

### `struct OracleCatalog`

```rust
pub struct OracleCatalog
```

The catalog several registered catalogs read, stated a second time in Rust.

**The oracle, and it is deliberately a separate hand-built catalog from `GoldenSubject`**, for
the reason `sutura-app`'s `HandWrittenCatalog` is separate from the documents it transcribes:
two independent statements of one metric must produce the same plan, and the golden cells compare
the subject against this one so a mutation to either half reddens rather than passing as self-
agreement. It declares `Golden` only so its own declaration-fidelity cell holds.

#### Implements

`Clone`, `Copy`, `Debug`, `Default`, `GoldenCatalog`, `SemanticCatalog`

### `struct GoldenSubject`

```rust
pub struct GoldenSubject
```

The catalog under test for the golden arm of this pack.

A second, independent statement of the same one-metric corpus. `plan_is_the_oracles_own` and
its siblings compile a question through BOTH this and `OracleCatalog` and require them to
agree, which is what makes the golden cells differential rather than a copy of the oracle against
itself.

#### Implements

`Clone`, `Copy`, `Debug`, `Default`, `GoldenCatalog`, `SemanticCatalog`

### `struct DeclaringSubject`

```rust
pub struct DeclaringSubject
```

The catalog under test for the declaring arm of this pack.

Supplies **part** of the model - a model and a metric with a grain and nothing else - and says
so in its declaration, so it is measured by `fidelity_holds` against that declaration and by
`repeat_load_is_stable`, and by no golden cell: it implements no `GoldenCatalog`, which is
what makes the golden cells impossible to expand for it.

#### Implements

`Clone`, `Copy`, `Debug`, `Default`, `SemanticCatalog`

### `fn plan_is_the_oracles_own`

```rust
pub fn plan_is_the_oracles_own<C>()
```

`CompileBehaviour::Plan` - the compiled mono plan is the oracle's plan, in serialized form.

BOUND on `GoldenCatalog`, so this cell cannot expand for a catalog that does not declare itself
golden: only a golden catalog owns an oracle to be held to.

### `fn statement_is_the_oracles_own`

```rust
pub fn statement_is_the_oracles_own<C>()
```

`CompileBehaviour::Statement` - for each declared dialect, the rendered statement equals the
oracle's.

BOUND on `GoldenCatalog`, for the reason `plan_is_the_oracles_own` gives.

### `fn params_are_the_oracles_own`

```rust
pub fn params_are_the_oracles_own<C>()
```

`CompileBehaviour::Params` - the bind parameters equal the oracle's.

BOUND on `GoldenCatalog`, for the reason `plan_is_the_oracles_own` gives. The statement and
its parameters are folded in plan order by `sutura_sql::generate`, so the parameters are
compared over one dialect the way the statement is, and over the same one.

### `fn refusal_reaches_the_variant_the_oracle_names`

```rust
pub fn refusal_reaches_the_variant_the_oracle_names<C>()
```

`CompileBehaviour::Refusal` - a refused question reaches the variant the corpus names, not another.

BOUND on `GoldenCatalog`, because "the variant the corpus names" is a statement only a
corpus-backed adapter has an oracle for. The expected is a `Debug` prefix, so a question that
started refusing for a different reason is caught rather than passing as *some refusal*.

### `fn fidelity_holds`

```rust
pub fn fidelity_holds<C>()
```

`CompileBehaviour::Fidelity` - what the catalog declared is exactly what its bundle produced.

**UNIVERSAL: both a golden and a declaring catalog owe this**, and it is the assertion a
declaring adapter gets in place of the golden oracle - `docs/adr/0016`'s decision. It is two
directions in one `MetadataCapabilities::checked_against` call: everything declared was
produced, and nothing undeclared appears. A declaration widened beyond the bundle fails the
`Unprovided` direction and reddens here.

### `fn repeat_load_is_stable`

```rust
pub fn repeat_load_is_stable<C>()
```

`CompileBehaviour::Repeat` - a second load produces the same digest.

**UNIVERSAL.** It loads the same bytes twice and compares the two digests, so what it can see is
a catalog that answers differently on a second read - a hash-ordered collection, a timestamp, a
source of randomness. It deliberately claims only determinism, because *the digest is a function
of CONTENT* is a claim about two different inputs and a fresh load is one input read twice.

### `fn compile_census`

```rust
pub fn compile_census(kind: &'static str, bound: &[CompileBehaviour])
```

Asserts a binding emitted a test for exactly the behaviours its kind selects, and that the corpus
is not empty to be green over.

Three things mirror `crate::census`'s first three, and the missing two are the venue ones it
does not have: a compile catalog is a hand-built fixture with no socket to be absent, so there is
no floor to measure and no `NOT RUN` to print.

1. **`bound` is the array the single `#[test]`/census repetition inside
   `compile_packs!` generated**, one element per emitted test, compared
   against the behaviours the kind selects -
   `CompileBehaviour::EVERY` for `"golden"`, `CompileBehaviour::UNIVERSAL` for `"declaring"`.
   A deleted test is a deleted element and this reddens, which is the correction `crate::census`
   narrates for the execute pack.
2. the plan corpus is not empty, which is the state that would make `plan_is_the_oracles_own`
   and its two siblings vacuously green.
3. the refusal corpus is not empty, which is the state that would make
   `refusal_reaches_the_variant_the_oracle_names` a green no-op.

What it cannot do is the same thing `crate::census` cannot: know that a behaviour's BODY
asserts anything. A pack that returned without comparing would pass here and everywhere else; the
cells above compare, which is the evidence `tests/compile.rs` is for.
