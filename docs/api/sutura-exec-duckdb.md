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

**A raw statement runs only on a database `DuckDbWarehouse::open` opened** (`docs/adr/0013`).
`DuckDbWarehouse::execute` takes an `Executable` and renders the statement itself; the one door
a caller's text reaches is `Warehouse::execute_raw`, and it answers `None` on a database
`DuckDbWarehouse::in_memory` opened. A file is opened with `READ_ONLY` and then
`THEN_LOCKED`: no write, no file or network outside the database, no extension, and the
configuration locked so no statement can turn any of that back. **Those settings, not a parse of
the text, are the barrier**: the driver runs every statement of a string but the last while
preparing it, before anything here could look.

**No result caching.** Under row-level security a query-keyed cache is a cross-user leak, and
although this adapter has no row-level security to leak through, adding a cache here would be the
place the habit started.

## Limits

- **The deadline is a watchdog per `execute`** (`docs/adr/0029`, sixth amendment): a thread
  calls the driver's `ConnectionCancel` when the budget is spent, the engine answers `Interrupt`,
  and that is read as the deadline. Granularity is the engine's own interrupt check, so a
  statement is stopped at its next one rather than the instant; `dry_run` only prepares and
  carries the deadline, and so does the raw path, where a statement runs until it ends.
- **The driver runs every statement of a string but the last at `set_sql_query`**, and prepares
  the last (the pinned `StatementSetSqlQuery`). A raw statement may be several; `READ_ONLY` and
  `THEN_LOCKED` are what each of them runs under.
- **What those leave a raw statement**: reading the declared database's tables, and
  objects that live and die with its own connection (a `TEMP` table, a session setting).
  `DuckDB`'s own `memory_limit` is its default, not `runtime.working_set_max_bytes`, and spilling
  is unmeasured: no local file opens after the database does, so a statement too large for
  memory is expected to fail rather than spill.
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
- `DeadlineExceeded` - The deadline ran out: spent before the statement started, or the watchdog interrupted it.
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

Opens a database that exists only for this process, WRITABLE - so it accepts no raw statement.

What the conformance packs and the differential use, attaching views over committed CSVs: a
fixture built from a committed CSV every run cannot drift from the CSV, and a database file
in the repository would be a binary nobody reviews. The golden row opens `Self::open`
instead, over a file `write_database` wrote.
**The posture is a parameter and has no default**, for the reason the port gives: a defaulted
posture would be a claim about who a query runs as that nobody made.

# Errors

As `Self::open`.

```rust
pub fn open(source: sutura_domain::model::SourceName, posture: sutura_domain::source::SourcePosture, path: &Path, result_budget: sutura_domain::warehouse::ResultBudget) -> Result<Self, DuckDbError>
```

Opens a database file with `READ_ONLY`, then runs `THEN_LOCKED` on it: the one constructor
a raw statement can run on.

# Errors

`DuckDbError::NoDriver` or `DuckDbError::Load` where there is no driver, and
`DuckDbError::Open` where the database does not open - a file that is not there included,
since a read-only open creates nothing, a path that is not UTF-8, and a `THEN_LOCKED`
statement the driver refused.

### Implements

`Debug`, `Warehouse`

## `fn write_database`

```rust
pub fn write_database(path: &std::path::Path, tables: &[(sutura_domain::model::TableName, std::path::PathBuf)]) -> Result<(), DuckDbError>
```

Writes a database file holding one table per CSV, for a fixture to `DuckDbWarehouse::open`.

Typed the way `DuckDbWarehouse::attach_csv` types a view. Available only with the default-off
`fixtures` feature.

# Errors

`DuckDbError::Open` where the file cannot be created, and `DuckDbError::Attach` where a CSV
cannot be read.

## `constant MOUNTED_DRIVER`

The variable a host that links no driver names a mounted `libduckdb` with.

**Not a settings key**, for `sutura-adbc-postgres`'s `MOUNTED_DRIVER` reason: which driver file a
host carries is a property of the host, and a build that links one never reads this.

## `constant READ_ONLY`

The options `DuckDbWarehouse::open` hands the driver with a database file's path.

Each is measured against the pinned driver by `tests/raw.rs`, with a cell there that is red
without it. `access_mode` refuses a write or DDL. `enable_external_access` refuses every file and
network read or write outside the database and every extension install or load (`ATTACH`,
`COPY ... TO`, `read_csv`, `glob`, `INSTALL`, `LOAD`) - **and so does `THEN_LOCKED`'s disabled
local file system, first**, so on the pinned driver no refusal is external access's alone and its
cell asserts the setting rather than an effect. It stays for the file system that is not local:
a network one a driver build links, which the pinned one does not. `DuckDB` itself refuses
turning either option back while the database is open, locked or not.

## `constant THEN_LOCKED`

What `DuckDbWarehouse::open` runs on the database once it is open, in order, before the
handle exists for anyone to call.

`disabled_filesystems` refuses a read of the database's OWN file as bytes, which external access
alone leaves open, and `lock_configuration` - last, so it locks what came before - a `SET` of an
instance-wide setting, which would otherwise outlive the statement for every later call. Each
has a cell in `tests/raw.rs` that is red without it. **Statements, not open options**: the pinned
driver refuses `disabled_filesystems` as an option ("Failed to set configuration option" -
`DuckDB` sets it only on a running database), and a lock handed over at open would refuse the
`SET` after it. So a lock moved first fails the open, rather than leaving the file system on.
