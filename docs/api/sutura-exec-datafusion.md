<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-exec-datafusion

The public API of `sutura-exec-datafusion`, rendered from rustdoc JSON.

A `Warehouse` adapter that executes a plan in process, generating no SQL at all.

`DataFusion` is the reason the port takes a `QueryPlan` rather than a rendered statement. It
has no dialect: a plan becomes a `LogicalPlan` over Arrow, the engine type-checks it against the
registered tables and runs it in this process. Every bug class that lived in rendering is
therefore absent here rather than fixed - there is no identifier to quote, no alias to emit, no
`GROUP BY` to hand the wrong expression to, and no placeholder whose position could disagree
with the parameter list.

Values are handed over as typed literals, which is the strongest form of parameterisation
available and not a weakening of one. See the `literal` function below.

Three things this adapter deliberately does not offer:

**No arbitrary SQL entry point.** Not merely absent: the `sql` feature is off, so this engine's
parser is not compiled into the binary. There is nothing here that turns text into a plan, which
is a stronger statement than not calling it.

**No entry point that takes a statement.** The only way in is a `QueryPlan`, which carries its
parameters as a typed list. A development affordance that ran something somebody typed would be
the shortest path around every check upstream of here.

**No result caching.** Under row-level security a query-keyed cache is a cross-user leak, and
although an in-process engine over a local file has no row-level security to leak through,
adding a cache here would be the place the habit started.

**The engine's memory is bounded, and the bound is not this process's memory.** Every session here
is built with a `RuntimeEnv` carrying a fixed-size pool, because the alternative is the engine's
unbounded one - and under `panic = "abort"` a large enough hash join is then process death for
every concurrent caller rather than an error for the one who asked. A refused reservation leaves as
`RefusalReason::ResourcesExhausted`. What the pool counts is operator reservations and **nothing
else**: not what a driver buffers, not `collect()` materialising every batch, not the row set built
in the conversion loop below. See `pool`, which states the gap rather than implying it is closed.

`translate`, `collect`, `fixture` and `pool` own narrow seams; this owns session and execution.

**No production gauge reads the `DataFusion` pool.** Measurement-only children can opt into a
separate recorder; ordinary adapter construction exports no live reservation reading. The
`check-guidance` absence rule rejects a production `.memory_pool()` call.

## `enum DataFusionError`

```rust
pub enum DataFusionError
```

Why this data system could not answer.

One variant per failure mode rather than one wrapper around the engine's error, because "the
plan would not build", "the plan would not resolve" and "a column came back as a type we do not
map" send a reader to three different places.

### Variants

- `Runtime` - The runtime this adapter executes on could not be built.

  One runtime is built per adapter and kept, so this happens once at construction or not at
  all. The engine is async all the way down and `futures::executor::block_on` panics at
  collect time with "no reactor running", so there is no runtime-free path to fall back to.
- `Environment` - The execution environment - the bounded memory pool, and nowhere to spill - could not be built.

  Separate from `Self::Runtime`, which is the *tokio* runtime: one is a thread pool and one is
  the memory bound, and a deployment that cannot start needs to know which. Like `Runtime` it
  happens once at construction or not at all.
- `UnknownCodec` - A path's outermost extension spells a compression codec this build cannot read.

  **A parse rather than a fallback, and the failure it replaces is silent.** Handing a
  compressed file to the engine as plain text does not fail: the schema is inferred from the
  codec's own header bytes, and the table resolves to columns nobody declared. So an extension
  that certainly names a codec and is not one this build compiled is refused by name.

  `crate::attach`'s `NEAR_MISSES` carries why this is a closed list rather than "anything
  unrecognised": `orders.txt` is a CSV and must keep reading.
- `UnknownFormat` - A path's extension names no format this engine reads.

  Raised by `attach_file`'s dispatch and by nothing else, so a composition root that offered a
  candidate name outside `crate::attach::candidates` gets a refusal rather than a CSV read of
  a file that is not one.
- `Attach`
- `QualifiedTableUnreachable` - A model names a table this engine has nowhere to look for.

  **This engine registers one file per model in its own table registry - there is no catalog and
  no schema above it - so a `dataset.table` or a `project.dataset.table` path names nothing it
  holds.** Refused by name rather than by dropping the qualifier and reading the table of that
  name from the registry, which is the wrong-number failure issue #83 reports: a plausible answer
  under a certified metric, off a table nobody asked for.

  A typed error and not a `RefusalReason`, because no question a caller could ask produces one:
  a table path comes from a catalog document. `sutura serve` refuses the same thing at BOOT, so a
  deployment reaches this only if a model arrived after the engine was opened.

  `sutura_sql::Dialect::qualification` declares the same limit for the rendering side, where
  `DuckDb` is `TableOnly` for exactly this reason.
- `Build` - A logical plan could not be assembled from the query plan.

  A bug here or upstream rather than a refusal: a caller cannot ask anything that causes one.
- `Analyze` - The engine refused the plan: an unknown table, an unknown column, a type mismatch.
- `Execute`
- `Unreadable` - A result column could not be read as a domain value.

  **Wrapped rather than restated, and that is `docs/adr/0039`'s point.** The mapping from an
  Arrow array to a `Value` is `sutura_domain::warehouse::arrow`'s, shared with every adapter
  whose driver speaks Arrow, so the five variants this replaces - an unmapped type, a failed
  downcast, a non-finite double, a day count that is not a date, a result that is not
  rectangular - are one cause with one set of messages instead of one copy per adapter.

  The `#[source]` chain is what keeps the detail reachable: `sutura-app`'s bounds suite matches
  `UnreadableCell::NotFinite`'s own column through it, which is what
  `zero_denominator: fails` actually produces.
- `Unannounced` - A result batch did not carry the fields the schema it arrived under announced.

  Unreachable through this engine - it produces its own batches from its own plan - and kept
  because the check is the shared one: `Accumulating` is the same guard a foreign ADBC driver's
  stream goes through, and one path through it is what stops the engine's own collection being
  the lenient copy.
- `SchemaMismatch` - The result schema is not the one the plan's labels describe.

  Checked rather than papered over. `QueryPlan::result_labels` is the one definition both
  adapters build from, so a disagreement here means the projection is not what we think it is,
  and answering from it would return a number from a column nobody chose.
- `KeyCounts` - A key probe's result was not the pair of counts its aggregate projects.

  A defect in this crate's aliasing or in its value mapping rather than anything about the
  data - two aggregates over no group produce one row of two integers - and it travels as an
  `Err` from the port, which the boot path reads as *this declaration went unchecked*.
- `MissingParam` - A predicate named a parameter index the plan does not have.

  Predicates are resolved by their recorded index rather than by position, so this is what a
  plan built with a stale index looks like instead of a silently wrong comparison.
- `NoPredicate` - A plan with no predicate at all.

  Unreachable: a plan always carries the two bounds of its `TimeRange`, which cannot be
  unbounded. Written as a branch rather than an assertion because the SQL path refuses the
  same shape, and an adapter that quietly ran it unfiltered would disagree with the other one
  about an unbounded scan.
- `NoPlaceForASubject` - The credential broker handed this adapter subject material it has nowhere to put.

  **An `Err` and never a refusal, and the direction is the point.** Nothing about the question
  was wrong: it is a wiring defect between the broker and the source declaration, and offering
  it as a refusal would invite a client to retry a deployment bug until something works.
  `docs/adr/0008` part 4 states both directions and says which one is silent - an adapter that
  quietly *accepted* material it cannot use would report a leg as impersonated that ran shared.

  This adapter is one process reading local files under one operating-system identity, which is
  what `Warehouse::IMPERSONATION` declares, so the only shape it can be handed is the
  deployment's own identity for that source. A configuration that asked for anything else does
  not boot - `SourcePosture::deliverable_by` refuses it in the composition root - so reaching
  this arm in production means the broker ignored the declaration it reads.
- `PresentedDisagreesWithPosture` - The broker presented a leg that does not agree with how this source was DECLARED.

  **The check above answers a different question, and a review found the gap.**
  `NoPlaceForASubject` compares what arrived against what this CODE can carry - the
  `Warehouse::IMPERSONATION` constant - and says nothing about the posture the composition
  root handed this adapter. So a shared leg carrying a *different* operator acknowledgement
  matched the variant and was accepted, and provenance - which is read off `posture` - then
  reported this adapter's own declaration rather than the acknowledgement the broker actually
  presented.

  The comparison is a real one rather than a value against itself: the broker reads the settings
  tree and this adapter holds what the root handed it. An `Err` for the reason the variant above
  is one - a wiring defect between the broker and the source declaration, which no caller may
  retry into an answer. The typed cause carries which disagreement it was.
- `DeadlineExceeded` - The deadline ran out before a call or around the whole `rows` future. Dropping that future requests abort of spawned asynchronous tasks; `DataFusion` wraps non-cooperative plan leaves so they yield. Already-running blocking work cannot be aborted and may outlive this error.

### Implements

`Debug`, `Display`, `Error`

## `struct DataFusionWarehouse`

```rust
pub struct DataFusionWarehouse
```

An in-process engine, behind the `Warehouse` port.

### Methods

```rust
pub fn attach_csv(&self, table: &TableName, path: &Path) -> Result<(), DataFusionError>
```

Exposes a CSV file as a table, compressed or not. `DataFusion` handles inference.

The codec comes from the path - see `Codec::of_path` - so `orders.csv` and `orders.csv.gz`
are one call.

```rust
pub fn attach_file(&self, table: &TableName, path: &Path) -> Result<(), DataFusionError>
```

Exposes whichever format a path's extensions name, compressed or not.

The one entry point a composition root needs: it pairs with `candidates`, so the set of
names a deployment looks for and the set this dispatch accepts are the same list. An
extension naming no format this engine reads is `DataFusionError::UnknownFormat` rather
than a guess at CSV - inferring a schema from a file of the wrong shape is how a table
resolves to columns nobody declared.

```rust
pub fn attach_fixture_csv(&self, table: &TableName, path: &Path) -> Result<(), DataFusionError>
```

Exposes a deliberately simple conformance fixture CSV with exact shared types.

Available only with the default-off `fixtures` feature.

```rust
pub fn attach_json(&self, table: &TableName, path: &Path) -> Result<(), DataFusionError>
```

Exposes a newline-delimited JSON file as a table, compressed or not.

The CSV affordance's twin for the other plain-text format, and the second half of what
`datafusion/compression` buys. NDJSON rather than a JSON array: the engine reads one record
per line, which is what streams and what a schema can be inferred from without holding the
document.

```rust
pub fn attach_parquet(&self, table: &TableName, path: &Path) -> Result<(), DataFusionError>
```

Exposes a Parquet file as a table.

**No codec argument, and that is the point rather than an omission.** A Parquet file records
its own compression per column chunk and the `parquet` feature's codecs read it, so there is
nothing for `Codec` to decide - and an outer `.gz` around a Parquet file is
`DataFusionError::UnknownCodec` through `Codec::of_path`, which is the honest answer to a
file that should not have been written that way. The module header carries the distinction.

```rust
pub fn new(source: SourceName, posture: SourcePosture, working_set: WorkingSet) -> Result<Self, DataFusionError>
```

Builds an adapter with nothing registered.

There is no file to open, which is the difference from the `DuckDB` adapter: the engine is
this process, and a table exists once it has been attached.

**It takes a ceiling, and that is not optional.** `SessionContext::new()` installs the
engine's unbounded pool, which under `panic = "abort"` makes a large enough join process death
rather than a refusal - so a constructor that let a caller skip the bound would be the one
place the whole control could be forgotten. `sutura_config::WorkingSetCeiling::DEFAULT_BYTES`
is what a caller with no settings to read uses.
**It also takes the posture, and that is not optional either**, for the reason the ceiling is
not: a defaulted posture would be a claim about who a query runs as that nobody made.

```rust
pub fn with_worker_threads(source: SourceName, posture: SourcePosture, workers: core::num::NonZeroUsize, working_set: WorkingSet) -> Result<Self, DataFusionError>
```

The same adapter, `workers` threads wide.

**What `Self::new` costs when several threads call it at once, measured rather than
assumed.** Twenty questions per caller over a million rows, in the `dev` profile - where
every dependency is already at `opt-level = 3`, so the aggregation kernels are the shipped
ones - on a sixteen-way host. Throughput, normalised to one caller on the current-thread
runtime:

| callers | `new` | `with_worker_threads(callers)` | one `new` per caller |
| --- | --- | --- | --- |
| 1 | 1.00x | 1.07x | 0.98x |
| 2 | 1.02x | 2.08x | 1.82x |
| 4 | 1.03x | 3.94x | 3.14x |
| 8 | 1.01x | 6.06x | 4.87x |

The first column is the finding: it is *flat*. A shared current-thread runtime does not scale
with callers at all on this workload, because every `block_on` drives the same single-threaded
core and the work is inside it. The third column is the other candidate - a runtime per
calling thread - and it is consistently worse than one wide runtime while also needing
per-thread state, so it was not taken.

**The partition count follows the width**, which a bare `worker_threads` call would not do.
`DataFusion` defaults `target_partitions` to `available_parallelism` - the number this key
exists to override, since a CPU quota does not change it - so a two-worker runtime would
otherwise build sixteen-way plans and execute them two at a time. Pinning it is why the small
widths above are *ahead* of the baseline rather than level with it.

`Self::new` is deliberately left alone: the command-line tool answers one question and
exits, and it is also the caller with no settings to read a width from.

```rust
pub const fn working_set(&self) -> WorkingSet
```

The configured pool ceiling; `MemoryPool::memory_limit` can report `Unknown` instead.

### Implements

`Debug`, `Drop`, `Warehouse`

## `use Codec`

The outer codec a text file is wrapped in.

A closed enum over what `datafusion/compression` compiles rather than a re-export of
`FileCompressionType`: that type also has variants for codecs this build does not have, so
matching on it would mean an arm nothing can produce. Converted at the one call site.

## `use candidates`

Every file name one model's table could arrive under, in the order to prefer them.

**The engine owns this list, not a composition root**, and that is what stops a deployment
offering a candidate `DataFusionWarehouse::attach_file` then refuses - or missing one it
reads. Parquet first, because it carries its own schema and its own codecs; then the two
plain-text formats, each uncompressed and then once per codec.

The names are relative: a caller joins each onto its data directory and takes the first that is
a file. `sutura_cli`'s two file-source searches are the callers.

## `use WorkingSet`

How many bytes the engine's operators may reserve at once.

**A newtype for the unit rather than for a range**, and that is the whole of its job:
`DataFusionWarehouse::with_worker_threads`
already takes a `NonZeroUsize` for a thread count, so a second bare `NonZeroUsize` beside it would
be two arguments of one type whose meanings are a width and a quantity of memory. Swapping them
compiles and installs a three-byte pool. Wrapped, the swap does not build.

It parses nothing beyond non-zero, which the inner type already carries - the range that matters is
parsed once, in `sutura_config::WorkingSetCeiling`, against the memory the process can actually
reach. This crate does not depend on that one and must not: an adapter does not call another
adapter, so the composition root converts.

## `use CombineError`

Why a combine could not be assembled.

Split the way the port's two predicates read it: the four caller-facing arms are deterministic
refusals about the DATA the legs returned, and the rest are this workspace's own wiring or the
engine's. Every arm carries an Arrow type or a label where it carries anything at all - a
driver's metadata and a label the splitter assigned - and never a cell.

## `use DataFusionCombiner`

The combiner: a tokio runtime, and a bounded session built per combine.

**It holds no session, and that is the whole reason the ceiling is a real bound.** A
`GreedyMemoryPool` is installed on a `RuntimeEnv` and a `RuntimeEnv` is installed on a
`SessionContext`, so a combiner that kept one session would have to fix the ceiling at
construction - and the ceiling is what a deployment configures per question. One session per
combine also means a combine's registered tables cannot outlive it, which is what keeps one
caller's leg results out of another's session.

**What it does NOT hold is an identity, and that is not an omission.** `crate::pool`'s process
is one operating-system identity, so a combine runs as the deployment. What the combiner carries
per subject instead is a `ComputeContext` - see
`Self::for_subject`.

## Module `measurement`

Opt-in peak recording for measurement-only children, excluded from default builds.
Opt-in measurement construction for bounded DataFusion execution.

`MeasuredWarehouse` changes no ordinary construction
path. It keeps a bounded recording pool beside a fresh adapter so a child can read only the engine
operators' reservation peak. It does not measure driver buffering, collected batches, domain-row
conversion, or the process resident set.

### `struct MeasuredWarehouse`

```rust
pub struct MeasuredWarehouse
```

A measured warehouse and its persistent operator-reservation observer.

It delegates the execution port unchanged. Ordinary `DataFusionWarehouse` construction retains
its direct `GreedyMemoryPool`, so recording
costs nothing outside an explicit measurement child.

#### Methods

```rust
pub fn attach_csv(&self, table: &TableName, path: &Path) -> Result<(), DataFusionError>
```

Attaches one CSV table to the measured child.

```rust
pub fn new(source: SourceName, posture: SourcePosture, working_set: WorkingSet) -> Result<Self, DataFusionError>
```

```rust
pub fn pool_peak(&self) -> usize
```

The persistent operator reservation peak in bytes.

```rust
pub fn pool_reserved(&self) -> usize
```

The reservation at the end of the observation window.

```rust
pub fn reset_peak(&self)
```

Starts a new pool observation window.

#### Implements

`Warehouse`

## Module `pool`

The working-set ceiling.

The pool, the never-spill policy, and how a refused reservation is recognised. Its own file
because it is a third seam, and because `lib.rs` is at the length gate.
The working-set ceiling.

The pool the engine's operators reserve against, and how a refused reservation is recognised on
the way back out.

# Why this file exists at all

**There was no memory pool.** Nothing in the workspace constructed a `RuntimeEnv`, so
`DataFusion` installed its `UnboundedMemoryPool` at both `SessionContext` construction sites -
and shipped profiles compile `panic = "abort"`, so a hash join or an aggregate wide enough to
outgrow the machine was not an error for the caller who asked. It was the process ending for every
caller in flight. A bounded pool turns that into a reservation that fails, which
`sutura_app::answer` turns into `RefusalReason::ResourcesExhausted`.

# What the pool counts, and what it does not

It counts what the engine's own operators reserve: a hash-join build side, aggregate state, a
sort. **It counts nothing else.** Not what a driver buffers, not `collect()` materialising every
batch into memory at once, not the `Vec<Vec<Value>>` built while a result is converted into domain
rows - all three of which are on the path a question takes through this crate. So this is not a
bound on the process's memory and must not be alerted on as one: a question large enough to end
the process on one of those paths still ends it. `docs/adr/0009` puts the bound that reaches them
- a byte budget applied as rows are converted - with the execution boundary rather than here, and
says so rather than letting this one be read as wider than it is.

# Greedy, and never spilling

`GreedyMemoryPool` rather than
`FairSpillPool`: first come, first served, and a reservation over the ceiling fails immediately.
`docs/adr/0009` Decision 3 decides the policy and the second of its two reasons is what settles
it - spilling writes the **asking subject's rows** to the pod's local disk, a data-at-rest
surface nothing in this design governs, on the one path whose whole purpose is that a query
executes as the person who asked. A bound that protects memory by making an ungoverned copy of
the data has not protected anything.

So temporary files are **disabled** rather than left at the engine's default of an OS temporary
directory. That is belt and braces on purpose: the pool alone would still let a spilling operator
react to a refused reservation by writing, and `DiskManagerMode::Disabled` is what makes there be
nowhere to write to. No spill directory, no disk sizing, and a refusal that does not depend on
disk state.

Not wrapped in `TrackConsumersPool` either, though it would improve the engine's own message: what
reaches a caller is `sutura_domain::query::RefusalReason::ResourcesExhausted`,
which carries the configured ceiling and deliberately nothing about what the question demanded.

**No production gauge reads the `DataFusion` pool.** ADR 0015 specifies that absence because an
operator-reservation reading is narrower than process memory. The opt-in measurement feature is
a gauge whose absence it currently specifies for production: it observes a separate recording
pool in fresh test children and exposes no accessor on the ordinary adapter.

### `struct WorkingSet`

```rust
pub struct WorkingSet
```

How many bytes the engine's operators may reserve at once.

**A newtype for the unit rather than for a range**, and that is the whole of its job:
`DataFusionWarehouse::with_worker_threads`
already takes a `NonZeroUsize` for a thread count, so a second bare `NonZeroUsize` beside it would
be two arguments of one type whose meanings are a width and a quantity of memory. Swapping them
compiles and installs a three-byte pool. Wrapped, the swap does not build.

It parses nothing beyond non-zero, which the inner type already carries - the range that matters is
parsed once, in `sutura_config::WorkingSetCeiling`, against the memory the process can actually
reach. This crate does not depend on that one and must not: an adapter does not call another
adapter, so the composition root converts.

#### Methods

```rust
pub const fn bytes(self) -> usize
```

The ceiling, for whatever sizes the pool.

```rust
pub const fn of_bytes(bytes: core::num::NonZeroUsize) -> Self
```

The ceiling, in bytes.

The one constructor, named for the unit so a call site reads as bytes at the point of the call
rather than at the declaration it came from.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`
