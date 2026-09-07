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
- `NotFinite` - A floating-point column came back as a value that is not a number.
- `NotADate` - A day number came back that is not a date this build can represent.
- `Shape`
- `KeyCounts` - A key probe's result was not the pair of counts its statement projects.
- `NoSchema` - The driver handed back a result set with no statement behind it, so there are no column labels to read.
- `Render` - The plan could not be rendered as SQL.
- `Attach`
- `NoPlaceForASubject` - One leg of a federated answer, rendered here and assembled above by the combiner.
- `PresentedDisagreesWithPosture` - The broker presented a leg that does not agree with how this source was DECLARED.

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
adapter usually grows and what would make every check upstream of here optional. The table
name is a `TableName`, so it cannot carry a quote; the path is a string, so it is escaped
the only way a SQL string literal can be, by doubling every quote.

`read_csv_auto` and not a bind parameter, because a table function's argument is part of the
statement's shape rather than a value and `DuckDB` will not bind one there.

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
