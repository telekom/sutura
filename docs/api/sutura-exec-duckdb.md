<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-exec-duckdb

The public API of `sutura-exec-duckdb`, rendered from rustdoc JSON.

A `Warehouse` adapter over `DuckDB`, through its ADBC driver, for local development and
single-file work.

`DuckDB` is the case where the data is a file and there is no server to authenticate against, so
the single-player credential is the process's own access. That is stated rather than hidden: this
adapter is not a stand-in for a data system with grants, and it is the one place in the design
where "run as the calling subject" is trivially satisfied because there is nobody else to be.

**ADBC is the only transport** (`telekom/sutura#913`). `DuckDB` is its own ADBC driver - the
engine library defines `duckdb_adbc_init` - so the driver is the archive this artefact links
(`nix/duckdb-adbc.nix`, both musl triples) or the `libduckdb` `MOUNTED_DRIVER` names. A result
arrives as Arrow batches and is handed on as the driver typed it: which types answer is decided
once, by the domain's reader (`ResultBatches::to_rows`), for every Arrow adapter alike.

Two things this adapter deliberately does not offer:

**No arbitrary SQL entry point.** `DuckDbWarehouse::execute` takes an `Executable` and renders
the statement itself, into a `GeneratedQuery` that carries its parameters separately. There is
no method that takes a string. A development affordance that ran a statement somebody typed would
be the shortest path around every check upstream of here.

**No result caching.** Under row-level security a query-keyed cache is a cross-user leak, and
although this adapter has no row-level security to leak through, adding a cache here would be the
place the habit started.

## Limits

- **The deadline is carried, not enforced** (`docs/adr/0029`): the driver's `ConnectionCancel`
  interrupts a running statement, and honouring the deadline needs a watchdog per call
  (`telekom/sutura#1236`).
- **The driver runs every statement of a string but the last at `set_sql_query`**, and prepares
  the last (the pinned `StatementSetSqlQuery`). Only rendered statements and this crate's own
  `attach_*` views reach it, each one statement.
- **A certified answer is handed on unread**, so a `REAL`, a non-finite `DOUBLE` or an unmapped
  type is refused by the domain's reader downstream rather than as a `DuckDbError`.

## `enum DuckDbError`

```rust
pub enum DuckDbError
```

Why this data system could not answer.

### Variants

- `NoDriver`
- `Load` - The driver did not load or initialise, by either route - a relative mounted path included.
- `Open`
- `Connect`
- `Prepare`
- `Execute`
- `Batch`
- `Parameters`
- `Unannounced`
- `OverBudget` - The collected result would cost more than this adapter's materialisation budget to hold.

  The byte budget the port's
  `result_did_not_fit` reads: a
  result refused for crossing it is *the result did not fit*, never a data-system failure,
  so a caller is refused rather than told to retry.
- `Unreadable` - A boot-path result the domain's reader refused - the type, or the value, names the column.
- `KeyCounts` - A key probe's result was not the pair of counts its statement projects.

  A defect in the rendering or in the value mapping, never anything about the data: the probe
  projects two aggregates over no group, so one row of two integers is the only shape it can
  have. It travels as an `Err` from the port, which the boot path reads as *this declaration
  went unchecked* rather than as a violated one.
- `Render` - The plan could not be rendered as SQL.
- `FixtureRead` - The fixture CSV could not be read to name its column types.
- `FixtureSchema` - The fixture CSV did not satisfy the shared schema boundary.
- `Attach`
- `NoPlaceForASubject` - The credential broker handed this adapter subject material it has nowhere to put.

  **An `Err` and never a refusal.** Nothing about the question was wrong: it is a wiring defect
  between the broker and the source declaration, and a refusal would invite a client to retry a
  deployment bug. `docs/adr/0008` part 4 is the decision, and the same variant exists on the
  engine adapter for the same reason - two implementors of one port, each answering for what it
  was handed, because neither may reach into the other for a shared check.

  One process holding one database under one operating-system identity, which is what
  `Warehouse::IMPERSONATION` declares here, so the only shape this can be handed is the
  deployment's own identity for that source.
- `PresentedDisagreesWithPosture` - The broker presented a leg that does not agree with how this source was DECLARED.

  **A different question from the variant above.** `NoPlaceForASubject` compares what arrived
  against what this CODE can carry - the `Warehouse::IMPERSONATION` constant - and reads
  `posture` not at all. So a shared leg carrying a *different* operator acknowledgement matched
  the variant this adapter accepts, while provenance, which is read off `posture`, reported this
  adapter's own declaration instead. An `Err` rather than a refusal, for the reason above.

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

# Errors

`DuckDbError::Attach` where the driver refused the view, a CSV it cannot read included.

```rust
pub fn attach_fixture_csv(&self, table: &TableName, path: &Path) -> Result<(), DuckDbError>
```

Exposes a conformance fixture CSV with shared column typing.

The columns are typed before the read, and the typing comes from
`sutura_domain::warehouse::csv` - the one classification every adapter shares. Naming the
complete map keeps decimals exact and prevents `DuckDB`'s boolean and wide-integer inference
from drifting from the other fixture adapters. Available only with the default-off `fixtures`
feature.

# Errors

`DuckDbError::FixtureRead`, `DuckDbError::FixtureSchema`, or `DuckDbError::Attach`.

```rust
pub fn in_memory(source: sutura_domain::model::SourceName, posture: sutura_domain::source::SourcePosture, result_budget: sutura_domain::warehouse::ResultBudget) -> Result<Self, DuckDbError>
```

Opens a database that exists only for this process.

What the golden suite uses: a fixture that is built from a committed CSV every run cannot
drift from the CSV, and a database file in the repository would be a binary nobody reviews.
**The posture is a parameter and has no default**, for the reason the port gives: a defaulted
posture would be a claim about who a query runs as that nobody made.

# Errors

As `Self::open`.

```rust
pub fn open(source: sutura_domain::model::SourceName, posture: sutura_domain::source::SourcePosture, path: &Path, result_budget: sutura_domain::warehouse::ResultBudget) -> Result<Self, DuckDbError>
```

Opens a database file.

# Errors

`DuckDbError::NoDriver` or `DuckDbError::Load` where there is no driver, and
`DuckDbError::Open` where the database does not open - a path that is not UTF-8 included,
refused rather than converted lossily into a path that names another file.

### Implements

`Debug`, `Warehouse`

## `constant MOUNTED_DRIVER`

The variable a host that links no driver names a mounted `libduckdb` with.

**Not a settings key**, for `sutura-adbc-postgres`'s `MOUNTED_DRIVER` reason: which driver file a
host carries is a property of the host, and a build that links one never reads this.
