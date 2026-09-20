---
title: DataFusion is the combiner, and the federation boundary is Arrow
description: Reverses ADR 0007's second amendment on owner instruction - the combiner is DataFusion, and datafusion-federation is the mechanism, so that a later multi-node move does not redevelop it. Decides which of the two routes into that crate is taken and why the SQL one is refused (its unparser inlines a caller's value into statement text and it needs a second SQL parser in the closure), what Arrow may mean inside the hexagon measured rather than argued (two Arrow majors coexist in one lockfile today, so the interior names no array type and the batches stay adapter-side), the dependency cost measured both as published and with one upstream line changed, and the one mechanism the whole adoption rests on: a per-caller compute context carrying an opaque subject digest, because provider equality is what the optimizer fuses scans on.
---

# DataFusion is the combiner, and the federation boundary is Arrow

Status: **accepted, and almost nothing here is built.** What exists in this tree is the one
mechanism the rest cannot be built without - `sutura_domain::identity::ComputeContext` and its
cells. Everything else is decided and scheduled; *The order this lands in* below says which step
proves what. A record whose code does not exist is a plan, and this one says so rather than being
read later as a description.

[Federating across different data systems](0007-federating-across-different-data-systems.md) decided
*the combiner is DataFusion*, then amended itself twice and landed a pure domain function instead.
Its *Second amendment* states the reversal: **"the combiner is NOT DataFusion"**. This record
reverses that amendment, on an owner instruction with a reason 0007 could not weigh at the time: a
later move to more than one node must not have to redevelop federation, and `datafusion-federation`
is where that work already is. 0007's *Third amendment* carries the pointer; this record carries the
argument, the measurements and the mechanism.

## The decision, in five parts

1. **The combiner is DataFusion, above the port**, and the mechanism is the
   `datafusion-federation` crate rather than a combiner of ours over the same engine. The shape 0007
   decided survives untouched: per-source legs, each a whole mono-source plan, executed in its own
   adapter under its own credential, joined and re-aggregated above the port. Only the identity of
   the thing above the port moves, and it moves to a crate rather than to code of ours.
2. **The federation boundary is Arrow-native and carries no rows.** What crosses it is the engine's
   own `SendableRecordBatchStream`. Nothing on that seam converts a batch into a
   `sutura_domain::warehouse::RowSet` and nothing converts back.
3. **The hexagon's interior names no Arrow array type, and that is a measurement rather than a
   preference** - *What Arrow may mean inside the hexagon* below. The batches are constructed and
   consumed by adapters; the interior never holds a cell of one.
4. **The `FederationProvider`/`FederationPlanner` route is taken and the `SQLExecutor` route is
   refused** - *Which of the two routes* below. That is what keeps `sutura-sql` in the rendering
   seat, keeps a caller's value out of statement text, and keeps a second SQL parser out of the
   closure.
5. **A federated leg's provider identity carries its caller, as an opaque digest.** This is the part
   that is not an optimisation question - *The mechanism this rests on* below. It is built; the rest
   is not.

## The mechanism this rests on, and it is a comparison

Read the upstream source before the argument, because the argument is a consequence of four lines of
it. Quotations are from `datafusion-federation` 0.5.6 and `datafusion-sql` 55.1.0, the versions the
workspace's `datafusion = "55.0.0"` pin resolves against.

| Where                                | What it says                                                                                                                                  |
| ------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/lib.rs:79`                      | `fn eq(&self, other: &dyn FederationProvider) -> bool { self.name() == other.name() && self.compute_context() == other.compute_context() }`   |
| `src/lib.rs:86`                      | `Hash` hashes the same two values and nothing else                                                                                            |
| `src/optimizer/scan_result.rs:21-25` | two `Distinct` providers become `Ambiguous` **only** `if provider != other_provider`                                                          |
| `src/optimizer/mod.rs:213`           | *"The plan is ambiguous; any input that is not yet optimized and has a sole provider represents a largest sub-plan and should be federated."* |
| `src/sql/executor.rs:27`             | *"returning None here may cause incorrect federation with other providers of the same name that also have a compute_context of None"*         |

So a provider *is* the pair `(name, compute_context)`, and the optimizer collapses every scan whose
provider compares equal into one federated node, executed through one of them. Two consequences, and
they are the same rule read twice:

- **The good case needs nothing.** One caller holding credentials for two data systems yields two
  providers with different names, so `ScanResult` becomes `Ambiguous`, and each maximal
  single-source sub-plan is federated to its own source under its own credential while the engine
  joins and re-aggregates above them over Arrow batches. That is the whole of what 0007's track 2
  wanted, and it is the default path rather than something to build.
- **The bad case is exactly one shape.** Two providers for the **same** source on behalf of
  **different** callers, comparing **equal**, stay `Distinct` - so one caller's scan is executed
  through the other caller's provider, on the other caller's credential. That is a cross-caller read
  produced by an optimisation, with no bug on either caller's own path.

**Putting the subject in the compute context turns the second case into the first.** Unequal
providers are the `Ambiguous` path, which federates each caller separately. The fix and the defect
are the same comparison.

### Why the context is a digest and not the subject

The context is interpolated into plan text. `src/sql/mod.rs:309-310` writes
`" compute_context={ctx}"` into `VirtualExecutionPlan`'s `DisplayAs`; `src/lib.rs:72` and
`src/sql/executor.rs:79,85` render it from the `Display` and `Debug` of the trait objects. So
whatever goes there reaches `EXPLAIN` output and anything that renders or logs a plan. A subject
identifier in plan text is a disclosure in an identity-aware product, and this repository's own
rules refuse a person's identifier in world-readable text on top of that.

The two requirements therefore pull opposite ways - discriminate the caller, disclose nothing about
them - and a digest is what satisfies both while staying **stable**, which is the third requirement
nobody states until push-down disappears: one caller's two scans of one source must still compare
equal, or a single source's answer arrives as several federated fragments.

`sutura_domain::identity::ComputeContext` is that value, and the reason it is a type rather than a
convention is this repository's own rule that an invariant is held by a type, a lint, a hook or a
gate and never by recall:

- **One constructor, and it takes the subject.** `ComputeContext::of(&SourceName, &Subject)` is the
  only door - private field, no `Default`, no `From`, no `parse`. A context with no subject in it is
  not a value this type has, so a future `compute_context()` that returns one is subject-bearing by
  construction, and one returning `None` or a constant does not type-check against it.
- **The full subject is never returned to the constructor.** `SubjectKey::feed` contributes the
  unmasked value to the hasher and hands nothing back, which makes it write-only by shape: it cannot
  become the path by which a raw subject reaches a formatter. It is `pub(super)`, so no other module
  of the domain crate can reach the full value at all.
- **Every field contributes its own fixed-width digest**, which is what makes the whole digest
  injective over its fields rather than over their concatenation. Two `(source, subject)` pairs that
  hash alike are two callers the optimizer may fuse, so a boundary that can slide is the same defect
  one layer down. A numeric length prefix would have done the same job and needed a byte order;
  `clippy::big_endian_bytes` and its siblings are all on here, and the sibling that matters is the
  host-endian one, where two hosts disagree about one subject's context and isolation becomes a
  property of the architecture. A per-field digest makes the choice not arise.

  **And this one is defence in depth rather than load-bearing, which a mutation established against
  a first attempt that claimed otherwise.** Replacing the per-field digest with the raw bytes leaves
  the suite green: the constructor's field order puts a non-empty constant between the two
  caller-influenced fields, so the slide is not reachable through the only door. Nothing gates that
  order, so the construction is what would hold the property the day it changes - and the cell that
  pins it does so at `feed_field` rather than end to end, because end to end it cannot be seen.
- **A scheme tag is the first field.** `DefinitionDigest` is also SHA-256 over domain values, and
  two digests that can collide across schemes are two things a later comparison could confuse.

The hash is the domain's own for the reason `ALLOWED_IN_DOMAIN`'s `sha2` entry already gives: a
digest computed outside the type that pairs it with its content is not a guarantee.

### What the digest does not buy, stated next to what it does

It is a plain SHA-256 over low-entropy inputs. It keeps a subject from appearing **verbatim** in
plan text and it gives the optimizer an unequal comparison. It is **not** secret against anyone who
can guess or enumerate subjects: they can hash their guesses and compare. Closing that needs a
per-process random salt - stable within a process, which is the whole span the optimizer's
comparison covers - and a salt needs an entropy source the interior does not have and may not
acquire without the argument that `ALLOWED_IN_DOMAIN` exists to host. It is the named upgrade path,
not a limitation this record leaves implied.

### The two further controls, and neither is built yet

The type holds the value. Two things about its *use* are decided here and are the later steps' to
build, so this record does not imply the whole of (d):

- **A per-request provider, bound to that request's own per-source credential.** No
  process-lifetime provider shared across callers.
- **A per-request session registry holding only that caller's providers**, so another caller's
  provider is not reachable to be fused with. Defence in depth behind the two above.

## Which of the two routes into the crate is taken

`datafusion-federation` offers two seams, and the difference is not a matter of taste.

**`SQLExecutor` (behind the crate's own default-off `sql` feature) is refused.** Its
`execute(&self, query: &str, schema: SchemaRef, filters: &[Arc<dyn PhysicalExpr>])` receives a
statement DataFusion has already unparsed from the sub-plan. Two measured consequences, each of
which reverses a named invariant of 0007:

- **A caller's value reaches the statement as text.** `datafusion-sql`'s unparser renders a literal
  inline - `src/unparser/expr.rs:266` is `Expr::Literal(value, _) => Ok(self.scalar_to_sql(value)?)`.
  0007's own sentence is *"a value from a question never reaches a statement as text, because every
  leg is a `GeneratedQuery` with statement and parameters in separate fields"*. The unparser's route
  goes through the engine's AST and `sqlparser`'s own escaping rather than through string
  concatenation, so this is a change of guarantee rather than an injection - from *parameterised,
  never text* to *escaped by a third-party unparser*. It is still a change of guarantee, and it is
  not one to make as a side effect of picking a seam.
- **A second SQL parser enters the closure**, which 0007 declined by name, and with it an unparser
  in the seat `sutura-sql` occupies.

**`FederationProvider` + `FederationPlanner` + `FederatedTableSource` is taken.** They are outside
the crate's `sql` feature. The planner returns an `ExecutionPlan` of ours that produces Arrow
batches, which means: no unparsing, no second parser, no literal inlined into text, `sutura-sql`
keeps the rendering seat and its goldens do not move, and the engine still joins and re-aggregates
above the boundary over Arrow batches - which is the whole of what parts 1 and 2 of the decision
ask for. `compute_context` is declared on `FederationProvider` (`src/lib.rs:63`) as well as on
`SQLExecutor`, so the mechanism above applies identically on this route.

### The unparser measurement, recorded because it prices the route NOT taken

Taken rather than assumed, and it **refutes the framing this change was scoped under**, which read
the `SQLExecutor::dialect` doc comment - *"currently supports 'sqlite', 'postgres', 'flight'"* - as
the unparser's dialect set. It is not; it describes that crate's own example executors.
`datafusion-sql` 55.1.0's `src/unparser/dialect.rs` ships seven concrete dialects -
`DefaultDialect`, `PostgreSqlDialect`, `DuckDBDialect`, `MySqlDialect`, `SqliteDialect`,
`BigQueryDialect`, `SnowflakeDialect` - plus `CustomDialect` and a `CustomDialectBuilder` with
twenty-two knobs.

Against `sutura_sql::Dialect`'s five: DuckDB, Postgres and BigQuery have a first-class dialect;
**ClickHouse and Oracle have none**, and `CustomDialectBuilder` is what would describe them. One
gap is worth naming because it is a correctness one rather than a coverage one: this repository
**forces** quoting on every identifier and alias, and of the three first-class dialects only
`DuckDBDialect` and `BigQueryDialect` return a quote character unconditionally -
`PostgreSqlDialect` does not override `identifier_quote_style` and inherits `DefaultDialect`'s
conditional rule, which quotes a keyword or an upper-case identifier and leaves the rest bare.
`CustomDialectBuilder::with_identifier_quote_style` closes it, because `CustomDialect` returns its
configured character unconditionally.

None of that is load-bearing for the route taken, and it is written down anyway: it is the price of
revisiting the decision, and a later reader who only has the doc comment would price it wrongly in
the other direction.

## What Arrow may mean inside the hexagon

The instruction this change was scoped under was to make `arrow` domain vocabulary, with the
measured transitive cost. **The measurement refuses it, and the refusal is better than the
instruction was.**

`xtask/src/boundaries/edges.rs`'s `ALLOWED_IN_DOMAIN` is an allowlist walked over the whole
transitive resolve graph, and the line it draws in its own words is *"None is a framework - no
runtime, no client, no engine."* Arrow is a memory format and an array library, so it is inside that
line where `datafusion` - an engine - is outside it. That asymmetry is real and it is not what
decides this.

**What decides it is that two Arrow majors coexist in this workspace's lockfile today, and that this
repository already has a gate whose reasoning settles the question.** `duckdb` 1.10505.0 resolves
`arrow` 58.4.0 and `datafusion` 55.1.0 resolves `arrow` 59.2.0, both present and both carrying a
dated row in `devco/arrow-majors-allow`, which `cargo xtask check-arrow` reads in both directions.

That file states the test this decision turns on, in its own words: *"a DUPLICATE costs build time
and supply-chain surface, while a TYPE BOUNDARY - first-party code holding a value from one major
and handing it to something expecting the other - does not compile, or is undefined behaviour if
forced across the C data interface. The test is whether any first-party crate NAMES the type."* And
it records the current answer: the split is a duplicate rather than a boundary **because**
`sutura-exec-duckdb` *"declares no arrow dependency, names no Arrow type and converts to a neutral
row type"*.

**So naming an Arrow array type in the interior is precisely the move that converts the tolerated
duplicate into a type boundary.** If the interior's vocabulary is `arrow` 59, then the adapter on 58
has to name an Arrow type to produce it, and by that file's own test the exception it currently
enjoys evaporates: the routes between two Arrow majors in one process are IPC bytes, which copies
every buffer, or the C data interface, which `unsafe_code = "forbid"` puts out of reach. Naming an
array type in the interior would therefore not make the interior Arrow-native - it would make one
adapter unable to speak the interior's vocabulary at all, which is the opposite of what was asked
for. 0007 recorded the split as a timing constraint with an expiry on the `duckdb` crate's own
release schedule, and that allowlist row says the upstream change has merged and is unreleased. It
has not expired.

**The narrower limit, because the allowlist states it and an overstated one is the defect:** that
adapter is a dev-dependency, so the two majors coexist in the test build and never in a shipped
artefact. A type boundary in the test build still does not compile, so this does not soften the
conclusion - it does mean the cost is a red test build rather than a shipped hazard, and a reader
who needs that distinction should have it.

The cost, measured against `cargo tree -p sutura-domain --all-features` rather than estimated, to
the standard `ALLOWED_IN_DOMAIN`'s `serde_json`/`sha2` entry set:

| Candidate          | New crates in the domain's tree                                                                                                           | What is in them                                                                                                                                                                   |
| ------------------ | ----------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `arrow-schema`     | **6**, of which **5 are already allowlisted** (`allocator-api2`, `equivalent`, `foldhash`, `hashbrown`, `indexmap`) - so **one new name** | no runtime, no client, no engine                                                                                                                                                  |
| `arrow-array`      | **28**                                                                                                                                    | brings `chrono`, `chrono-tz`, `iana-time-zone`, `core-foundation-sys`, `getrandom`, `zerocopy`, `half`, the `num-*` stack - a time-zone database and an entropy source among them |
| `arrow` (umbrella) | **72**                                                                                                                                    | adds `crossterm` and `comfy-table` (a terminal renderer), `csv`, `regex`, `flatbuffers`, `rustix`, `zstd-sys` and `lz4_flex`                                                      |

**So: the interior names no Arrow array type.** The umbrella is refused outright - a terminal
renderer in the hexagon's interior needs no further argument. `arrow-schema` is the one candidate
whose cost is a single name, and this record does not spend it either, because a schema with no
arrays buys nothing at the seam that matters; it is pre-approved in the sense that a later step
needing a schema in a domain signature has the number already, and a diff adding it has this
paragraph to argue from.

**The boundary is still Arrow-native**, and the reason that is not a contradiction is the same
reason 0007 gave for the port carrying a plan: what crosses is the engine's own batch stream,
constructed and consumed by adapters, and the interior holds none of it. That is the shape the owner
asked for read literally - *the domain never touching a row* - rather than the shape that names a
crate in a manifest.

**ADBC is why this is cheaper than it looks and is not work this record schedules.** An ADBC driver
returns Arrow batches natively, so an Arrow-native federation boundary removes a conversion from
such an adapter instead of adding one. That is an argument for the direction, not a dependency of
it.

## What adopting the crate costs, measured both ways

Two throwaway resolves outside this tree, each against the workspace's own `datafusion` pin and
feature set, diffed against this workspace's `Cargo.lock` package list:

| Adoption shape                                                                                                           | New lockfile entries |
| ------------------------------------------------------------------------------------------------------------------------ | -------------------- |
| `datafusion-federation = "0.5.6"` as published, with `default-features = false`                                          | **22**               |
| the same crate with one line changed in its own manifest - `default-features = false` on its own `datafusion` dependency | **3**                |

**The lockfile grows either way, and the difference is one line upstream.** The three are
`datafusion-federation` itself plus `async-stream` and `async-stream-impl`, which it declares
directly. The other nineteen are `arrayvec`, `async-compression`, `bigdecimal`, `blake2`, `blake3`,
`bzip2`, `compression-codecs`, `compression-core`, `constant_time_eq`,
`datafusion-functions-nested`, `datafusion-sql`, `generic-array`, `libbz2-rs-sys`, `liblzma`,
`liblzma-sys`, `recursive`, `recursive-proc-macro-impl`, `sqlparser` and `sqlparser_derive` - and
none of them is reachable from the route this record takes. They arrive because that crate's
`datafusion` dependency does not set `default-features = false`, and cargo unifies features across
the workspace, so datafusion's default set - which includes `sql`, `compression`,
`crypto_expressions`, `nested_expressions` and `recursive_protection` - is enabled for every
consumer of the one `datafusion` in the graph.

Three of those nineteen are the reason this is a decision and not a footnote:
**`datafusion-sql`, `sqlparser` and `sqlparser_derive` put a second SQL parser and an unparser into
the closure of every shipped binary**, which is exactly what 0007 declined and what
`sutura-exec-datafusion`'s own module documentation claims is absent: *"the `sql` feature is off, so
this engine's parser is not compiled into the binary."* And **`liblzma-sys` builds C**, on four
published triples including two musl ones, which is the class of dependency this repository's crate
map says breaks a cross build rather than slowing one.

**So the action is upstream, per this repository's own rule of engaging the ecosystem rather than
monkeypatching**: propose `default-features = false` on that crate's `datafusion` dependency, which
is correct for it independently of us - its non-`sql` surface uses none of the default set. Vendoring
to move first is the fallback and is recorded in `VENDOR.md` if taken. Until one of the two lands,
adoption costs 22 entries and falsifies the sentence quoted above, and *The order this lands in*
puts that step where it has to be resolved rather than discovered.

## The refusal surface, reconciled

`RefusalReason::MeasureDoesNotFederate` is live, raised at two sites in
`crates/sutura-semantic/src/plan.rs`, and 0007's own correction insists it was never superseded.
This record does not retire it and does not narrow it, and the reason is worth stating because the
opposite reading is available: DataFusion doing the combine changes **who** re-aggregates, not
**whether** an aggregate re-aggregates. An exact distinct count over two legs is not recoverable by
any combiner from two per-leg counts - the identity is arithmetic, not an implementation gap - so a
measure that does not decompose still does not decompose above a DataFusion join.

**What does change is the alternative to refusing.** 0009's Decision 2 already says a
non-descending aggregate is answered by transporting finer-grained rows and computing above, and a
federated engine makes that pull-up cheaper and wider than a hand-written combine made it. So the
expected direction is *fewer questions reaching the refusal because more of them descend or pull up*

- and that is a change to the splitter, measurable as a refusal that stops being raised for a
  question that used to raise it. A refusal that silently stops refusing is a change of guarantee, so
  any such move arrives with the cell that shows which question moved, and not as a side effect of
  adopting a crate.

## The order this lands in, and what each step proves

The whole of this record is not one reviewable change. The steps, in the order they must land,
because each one's evidence depends on the one before it:

1. **The mechanism, and it is what this record lands with.** `ComputeContext` plus its cells: two
   subjects on one source do not share a context, one subject on one source reproduces its context,
   and the rendered form carries no subject identifier. Cuts nothing, adds no dependency, and is the
   value every later step routes through. Its limit is stated plainly: **nothing calls it yet**, so
   what is proven is the value's properties and not a path.
2. **The dependency, resolved rather than absorbed.** Add `datafusion-federation`, with the upstream
   `default-features = false` question closed one way or the other first, and re-measure the four
   cross builds - which `just validate` does not reach and `just gates` does not either, so that
   step's evidence is a CI cross build and nothing local.
3. **A `FederationProvider` for one source**, per request, bound to that request's own per-source
   credential, with `compute_context` returning `ComputeContext::as_str` and the per-request
   registry. Proves isolation on a path rather than on a value: the cell is two callers on one
   source not fusing, and the mutation that drops the subject from the context reddens it.
4. **The second source, and the answer compared.** The existing federated goldens and differential
   cells answer identically through the new path while the old one still runs. Any answer that moves
   is the finding, not a snapshot to accept.
5. **The cut.** `FederatedPlan::combine` and `sutura_semantic::federated_plan` go, with no shim and
   no second path, once step 4 has shown the two agree.

`FederatedPlan`'s `SameSource` refusal is explicitly not relaxed anywhere in this sequence: the
per-leg credential and posture path in `crates/sutura-app/src/federated.rs` is built on the two
sources being distinct, which makes it identity-adjacent and not a cleanup.

## What is explicitly not decided

- **Whether `sutura-sql` renders a federated leg forever.** It keeps the seat under the route taken.
  If the `SQLExecutor` route is ever revisited, the unparser measurement above is the price.
- **The Arrow major split.** It closes on `duckdb-rs`'s own schedule, and nothing here needs it to.
- **Whether the interior ever names `arrow-schema`.** The number is measured (one new name); the
  spend is not taken.
- **Multi-node.** It is the owner's reason for this record and not a thing this record builds. What
  it buys is that the mechanism a multi-node move needs is a crate we depend on rather than code we
  wrote.
