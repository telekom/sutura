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
    fn open() -> DuckDbWarehouse { /* attach `corpus::on_disk()` */ }

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
3. **A behaviour an adapter cannot satisfy is never a silent pass.** Two mechanisms, because the
   two cases are different - see below.

# How an unsupported capability is kept out of the green count

| Case | Mechanism |
| --- | --- |
| A capability with a **typed declaration** on the port (`EXECUTES_LEGS`) | The binding names the declaration, a `const` assertion tears a binding that disagrees with it, and the two directions get DIFFERENT test names - so which one ran is in the report rather than in a skip nobody reads |
| A capability with **no constant to declare** (a pre-flight) | The pack returns `Outcome::Declined` carrying a typed `Declination`, which `hold` prints as `DECLINED`. Stated limit: this is observed at run time, so it is weaker than the row above and would become that row the day the port carries the constant |
| The corpus being empty, which would make every behaviour vacuously green | `census` fails on a corpus with no cases, and prints the case count beside the behaviour count so a run's ratio is read rather than assumed |

# What a green conformance run does NOT establish

- **Ordering within a leg, and impersonation in any form.** `execute`'s header says which and
  why; `docs/adr/0012` decides the impersonation one.
- **That the corpus is hard.** It is two questions over one table - `corpus` lists by name the
  cases `docs/adr/0012` says nothing else finds, none of which is here yet.
- **That every adapter is held.** A pack is bound where an adapter's own crate binds it, so which
  adapters conform is a question about which crates carry a `tests/conformance.rs`, and the
  answer is in those crates rather than here.

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
pub fn hold<E>(adapter: &str, behaviour: Behaviour, conformed: Conformed<E>)
```

Reports one behaviour, and fails the test if it did not hold.

The only place in this crate that ends a test, so what a failure prints is decided once: the
adapter, the behaviour, and the whole cause chain. `Display` on a `thiserror` enum prints the
outermost message and stops, and the outermost message here is the pack's - what tells a rejected
statement from an outage is one and two levels down.

## `fn census`

```rust
pub fn census<W>(adapter: &str, bound: &[Behaviour])
```

What a binding actually covered, asserted and printed.

Three things, and the third is why this exists rather than a comment:

1. the universal behaviours the binding emitted tests for are the ones `execute` defines, so a
   behaviour that lost its test reddens here;
2. the corpus is not empty, which is the state that would make every behaviour above vacuously
   green;
3. the counts are PRINTED - behaviours, cases, and which direction the leg declaration selected -
   because a suite that reports a ratio it has not earned is the failure this repository has
   already met twice.

What it cannot do: know that a behaviour's body asserts anything. That is what a red-against-base
run is for.

## `use None`

The domain, re-exported so `execute_packs` can name the port without the consuming crate
having to depend on `sutura-domain` under that spelling.

A `macro_rules!` body resolves item paths at the EXPANSION site, so a bare `sutura_domain::` in
the expansion would compile only for a consumer that happens to have that dependency under that
name. `$crate::sutura_domain` always resolves.

## `type_alias Conformed`

What one behaviour of one pack answers.

## `macro execute_packs`

Binds the execute pack to one adapter, as one named `#[test]` per behaviour.

```ignore
#[cfg(test)]
mod conformance {
    fn open() -> DuckDbWarehouse { /* attach `corpus::on_disk()` */ }

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
- `open` is a **path** to an `fn() -> W`, called once per test so no state crosses between them.
  A path rather than a closure because a `macro_rules!` body resolves items at the expansion
  site: a closure naming a type the caller imported at file scope would not resolve inside the
  generated module, and a `crate::`-rooted path always does. The fixture lives in a
  `#[cfg(test)]` module because the strict lints exempt what is inside one, which is why the
  path in the example names that module.
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
workspace. **Nothing enforces the file name or the wrapper module**; they are a convention, and
an adapter that ignores them is still run by `just test` under a name a filter has to spell
differently.

## Module `corpus`

The corpus the execute packs run: one table, two questions, and the answer written ONCE.

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
- **A null in a group key.** Null placement in `ORDER BY` differs per data system and is not
  stated by the plan, so a null key would make `crate::Behaviour::Order` a claim about the
  source's collation. `docs/adr/0012` decides that the packs re-sort rather than assert that;
  this corpus avoids the question instead, which is weaker and is why it is written here.
- **A wide integer or a decimal.** The type-mapping disagreements
  `sutura_domain::warehouse::agreement`'s header lists are all reachable only past an `i64`,
  and nothing here goes near one.
- **Files.** `docs/adr/0012`'s *the corpus is files, not code* is unbuilt: a case is a value in
  this module, so adding one is still a code change.

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

The corpus, as CSV.

### `fn on_disk`

```rust
pub fn on_disk() -> std::path::PathBuf
```

The corpus on a filesystem, written once per process, as the path a fixture attaches.

Written under a per-process temporary name and RENAMED onto the shared one, so two adapters'
fixtures running in one process cannot read a half-written file. The bytes are identical either
side of the rename, so a reader that saw the old file saw the same corpus.

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

- **Ordering inside a leg.** A [`LegPlan`](sutura_domain::plan::LegPlan) carries no row limit and
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
`docs/adr/0012` calls *a declared absence with something to try*: the value of it is that nothing
upstream builds a leg today, so this is the only thing that exercises the adapter's own guard -
and an adapter that quietly computed one instead would be surfacing half an answer under a
certified metric name.

**A whole plan is executed FIRST, and that is not a warm-up.** A pack cannot see which error an
adapter refused with - `Self::Error` is the adapter's own type - so on its own this behaviour
would be green for an adapter that failed for any reason at all, including one that cannot reach
its data system. Answering a whole plan immediately before is what makes the refusal evidence
about the leg. The residual limit: the refusal is still only *an* error, so an adapter that
refused a leg for the wrong reason passes.
