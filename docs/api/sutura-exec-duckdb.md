<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-exec-duckdb

The public API of `sutura-exec-duckdb`, rendered from rustdoc JSON.

A `Warehouse` adapter over `DuckDB`, for local development and single-file work.

`DuckDB` is the case where the data is a file and there is no server to authenticate against, so
the single-player credential is the process's own access. That is stated rather than hidden: this
adapter is not a stand-in for a data system with grants, and it is the one place in the design
where "run as the calling subject" is trivially satisfied because there is nobody else to be.

Two things this adapter deliberately does not offer:

**No arbitrary SQL entry point.** `DuckDbWarehouse::execute` takes an `Executable` and renders
the statement itself, into a `GeneratedQuery` that carries its parameters separately. There is
no method that takes a string. A development affordance that ran a statement somebody typed would
be the shortest path around every check upstream of here.

**No result caching.** Under row-level security a query-keyed cache is a cross-user leak, and
although this adapter has no row-level security to leak through, adding a cache here would be the
place the habit started.

## `enum DuckDbError`

```rust
pub enum DuckDbError
```

Why this data system could not answer.

### Variants

- `Open`
- `Prepare`
- `Execute`
- `UnsupportedType` - A column came back as a type this adapter does not map.

  An error rather than a stringified fallback. A `LIST` or a `STRUCT` rendered with `Debug`
  would flow into an answer looking like data, and an anchor comparison against it would pass
  or fail for reasons nobody could read.

  The column is named by its LABEL rather than by its position, which is also what the engine
  does. The two adapters answer one plan, so an error from either has to be readable against the
  same projection, and "column 1" is a fact about a result set nobody has in front of them.
- `NotFinite` - A floating-point column came back as a value that is not a number.

  **What `zero_denominator: fails` actually produces.** The generator emits that ratio's
  division unguarded and casts the numerator to `DOUBLE` first, so the division is IEEE float
  division: `CAST(3 AS DOUBLE) / 0` is `inf` here rather than an error, and `0 / 0` is `NaN`.
  `Real` refuses all three, so the word `fails` is true of the metric that chose it instead of
  answering the string `inf` under a certified name.

  The cause names which of the three it was; this variant names the column.
- `NotADate` - A day number came back that is not a date this build can represent.

  The cause is kept rather than discarded: "not a date" and "a date in the year 40 000" send a
  reader to different places.
- `Shape`
- `KeyCounts` - A key probe's result was not the pair of counts its statement projects.

  A defect in the rendering or in this adapter's value mapping, never anything about the data:
  the probe projects two aggregates over no group, so one row of two integers is the only shape
  it can have. It travels as an `Err` from the port, which the boot path reads as *this
  declaration went unchecked* rather than as a violated one.
- `NoSchema` - The driver handed back a result set with no statement behind it, so there are no column labels to read.

  **An error rather than an empty projection, and the empty projection was the bug.** This was
  `unwrap_or_default()`, which turns a missing schema into a zero-column result - and a
  `RowSet` with no columns and N rows is a shape `RowSet::new` ACCEPTS, because every row
  then has no cells either and the thing is rectangular. So a question would have been answered
  with a result set that had silently lost its projection, under a certified name and with
  provenance attached. Refusal beats degradation on a shape check: nothing downstream can tell
  "this metric has no columns" from "this driver told us nothing".

  No `#[source]`, because there is nothing to preserve: the handle is an `Option` and the
  absent case carries no cause. That is the whole of what the driver said.
- `Render` - The plan could not be rendered as SQL.

  This adapter speaks SQL, so it asks the compiler to render the plan for its own dialect. An
  adapter that executes a plan directly - the in-process engine - never reaches this.
- `FixtureRead` - The fixture CSV could not be read to name its column types.

  `attach_fixture_csv` reads the bytes to type the
  columns before the query; a file that cannot be read is a fixture defect, not a number to
  answer.
- `FixtureSchema` - The fixture CSV did not satisfy the shared schema boundary.
- `Attach`
- `NoPlaceForASubject` - One leg of a federated answer, rendered here and assembled above by the combiner.

  **Not a refusal and not a default body.** `Warehouse::execute` takes an
  `Executable`, so this adapter's match over what it can be handed is exhaustive. A
  `LegPlan` renders through `generate_leg` and runs like any
  other statement; it carries no row cap, because a leg is not an answer - the combiner above
  it applies `MAX_ROWS`.

  The credential broker handed this adapter subject material it has nowhere to put.

  **An `Err` and never a refusal.** Nothing about the question was wrong: it is a wiring defect
  between the broker and the source declaration, and a refusal would invite a client to retry a
  deployment bug. `docs/adr/0008` part 4 is the decision, and the same variant exists on the
  engine adapter for the same reason - two implementors of one port, each answering for what it
  was handed, because neither may reach into the other for a shared check.

  One process holding one connection under one operating-system identity, which is what
  `Warehouse::IMPERSONATION` declares here, so the only shape this can be handed is the
  deployment's own identity for that source.
- `PresentedDisagreesWithPosture` - The broker presented a leg that does not agree with how this source was DECLARED.

  **A different question from the variant above, and a review found that only the first was
  being asked.** `NoPlaceForASubject` compares what arrived against what this CODE can carry -
  the `Warehouse::IMPERSONATION` constant - and reads `posture` not at all. So a shared leg
  carrying a *different* operator acknowledgement matched the variant this adapter accepts and
  was executed, while provenance, which is read off `posture`, reported this adapter's own
  declaration instead.

  The two values compared are genuinely independent: the broker reads the settings tree and this
  adapter holds what the composition root handed it. An `Err` rather than a refusal, for the
  reason the variant above is one. The same variant exists on the engine adapter, because neither
  implementor of this port may reach into the other for a shared check.

### Implements

`Debug`, `Display`, `Error`

## `struct DuckDbWarehouse`

```rust
pub struct DuckDbWarehouse
```

A `DuckDB` database, behind the `Warehouse` port.

### Methods

```rust
pub fn attach_csv(&self, table: &TableName, path: &Path) -> Result<(), DuckDbError>
```

Exposes a CSV file as a table.

A narrow, typed affordance instead of a general "run this SQL" method, which is what a local
adapter usually grows and would make every check upstream of here optional. The table name is
a `TableName`, so it cannot carry a quote; the path is a string, so it is escaped the only
way a SQL string literal can be, by doubling every quote.

```rust
pub fn attach_fixture_csv(&self, table: &TableName, path: &Path) -> Result<(), DuckDbError>
```

Exposes a conformance fixture CSV with shared column typing.

The columns are typed before the read, and the typing comes from
`sutura_domain::warehouse::csv` - the one classification every adapter shares. Naming the
complete map keeps decimals exact and prevents `DuckDB`'s boolean and wide-integer inference
from drifting from the other fixture adapters. Use `Self::attach_csv` for CSVs outside the
deliberately simple fixture format. Available only with the default-off `fixtures` feature.

```rust
pub fn in_memory(source: sutura_domain::model::SourceName, posture: sutura_domain::source::SourcePosture) -> Result<Self, DuckDbError>
```

Opens a database that exists only for this process.

What the golden suite uses: a fixture that is built from a committed CSV every run cannot
drift from the CSV, and a database file in the repository would be a binary nobody reviews.
**The posture is a parameter and has no default**, for the reason the port gives: a defaulted
posture would be a claim about who a query runs as that nobody made.

```rust
pub fn open(source: sutura_domain::model::SourceName, posture: sutura_domain::source::SourcePosture, path: &Path) -> Result<Self, DuckDbError>
```

Opens a database file.

### Implements

`Debug`, `Warehouse`
