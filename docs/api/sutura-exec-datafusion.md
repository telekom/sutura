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
- `Attach`
- `Build` - A logical plan could not be assembled from the query plan.
- `Analyze` - The engine refused the plan: an unknown table, an unknown column, a type mismatch.
- `Execute`
- `UnsupportedType` - A column came back as a type this adapter does not map.
- `Downcast` - The schema said one Arrow type and the array was another.
- `NotADate` - A day number came back that is not a date this build can represent.
- `Shape`
- `SchemaMismatch` - The result schema is not the one the plan's labels describe.
- `MissingParam` - A predicate named a parameter index the plan does not have.
- `NoPredicate` - A plan with no predicate at all.

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
pub fn new(source: SourceName) -> Result<Self, DataFusionError>
```

Builds an adapter with nothing registered.

There is no file to open, which is the difference from the `DuckDB` adapter: the engine is
this process, and a table exists once it has been attached.

### Implements

`Debug`, `Warehouse`
