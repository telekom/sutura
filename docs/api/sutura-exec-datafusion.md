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

What it does offer is two narrow, typed attach affordances, `DataFusionWarehouse::attach_csv`
and `DataFusionWarehouse::attach_parquet`, which is what a golden fixture needs.

**The engine's memory is bounded, and the bound is not this process's memory.** Every session here
is built with a `RuntimeEnv` carrying a fixed-size pool, because the alternative is the engine's
unbounded one - and under `panic = "abort"` a large enough hash join is then process death for
every concurrent caller rather than an error for the one who asked. A refused reservation leaves as
`RefusalReason::ResourcesExhausted`. What the pool counts is operator reservations and **nothing
else**: not what a driver buffers, not `collect()` materialising every batch, not the row set built
in the conversion loop below. See `pool`, which states the gap rather than implying it is closed.

# Four files, along three seams

`translate.rs` turns a plan into expressions and never reads a result; `collect.rs` turns a result
into domain rows and never reads a plan except for its labels; `pool.rs` is the working-set ceiling
and reads neither. What is left here is what none of them is about: the session, the runtime,
attaching a file, and executing.

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
- `Environment` - The execution environment - the bounded memory pool, and nowhere to spill - could not be built.
- `Attach`
- `QualifiedTableUnreachable` - A model names a table this engine has nowhere to look for.
- `Build` - A logical plan could not be assembled from the query plan.
- `Analyze` - The engine refused the plan: an unknown table, an unknown column, a type mismatch.
- `Execute`
- `UnsupportedType` - A column came back as a type this adapter does not map.
- `Downcast` - The schema said one Arrow type and the array was another.
- `NotFinite` - A floating-point column came back as a value that is not a number.
- `NotADate` - A day number came back that is not a date this build can represent.
- `Shape`
- `SchemaMismatch` - The result schema is not the one the plan's labels describe.
- `KeyCounts` - A key probe's result was not the pair of counts its aggregate projects.
- `MissingParam` - A predicate named a parameter index the plan does not have.
- `NoPredicate` - A plan with no predicate at all.
- `NoPlaceForASubject` - The credential broker handed this adapter subject material it has nowhere to put.
- `PresentedDisagreesWithPosture` - The broker presented a leg that does not agree with how this source was DECLARED.

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

Exposes a CSV file as a table.

A narrow, typed affordance instead of a general "run this" method, which is what a local
adapter usually grows and what would make every check upstream of here optional. Nothing is
escaped and nothing is quoted, because nothing is rendered: the name and the path are
arguments to a registration call, so a path with a quote in it is a path. `has_header` is the
read options' default.

```rust
pub fn attach_parquet(&self, table: &TableName, path: &Path) -> Result<(), DataFusionError>
```

Exposes a Parquet file as a table.

The CSV affordance's twin, and why the `parquet` feature is on. Neither `compression` nor
`avro` is: each reintroduces a licence the supply-chain gate does not allow, and the
manifest's comment records which.

```rust
pub fn memory_pool(&self) -> &Arc<dyn MemoryPool>
```

The pool every operator in this session reserves against.

**An accessor because the fields are private and stay private**, and because
`docs/adr/0015` needs `MemoryPool::reserved` for a gauge whose absence it currently specifies:
a gauge reading zero while no pool exists is a lie an operator builds an alert on.

**State the limit with the reading.** What comes back counts operator reservations - a
hash-join build side, aggregate state, a sort - and nothing else. It is not this process's
memory, and it must not be alerted on as though it were: `collect()` materialising every batch
and the row set built during conversion are both outside it, on the same request path.

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

The ceiling this adapter's pool was built with.

From the configured value rather than from `MemoryPool::memory_limit`, which defaults to
`Unknown`: a pool that does not override it reports no ceiling, and then the reserved-against-
ceiling ratio an operator actually wants cannot be computed.

### Implements

`Debug`, `Drop`, `Warehouse`

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
