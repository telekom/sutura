<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-exec-bigquery

The public API of `sutura-exec-bigquery`, rendered from rustdoc JSON.

A `Warehouse` adapter over `BigQuery`: render the plan, push it down, map the rows back.

`BigQuery` is the first source in this repository that is a **network service with its own
authorization**, which is what makes it the interesting one: a file engine has nobody else to be,
and a dataset has grants that belong to somebody. Everything about that is declared here rather
than assumed - see *identity*, below.

# What is built, and what is NOT

**This crate does not contain an HTTP client, and that is a decision rather than an omission.**
What it contains is everything this adapter DECIDES:

- the credential match and the posture agreement, so a leg cannot execute as an identity nobody
  declared;
- the rendering, through `sutura-sql` in `Dialect::BigQuery`, so no second set of quoting and
  placeholder decisions exists here;
- the refusal of a federated leg, because there is no combiner above it;
- the value mapping, which is where a wrong number would come from.

**A limit of that mapping, stated because it decides what a time column on this source is:**
`transport::FieldType` reads `DATE` and refuses `TIMESTAMP` and `DATETIME` - a timestamp arrives
as epoch-seconds text the `Date` arm cannot parse, so either comes back `Unmapped` and fails the
answer, which is the correct and loud outcome. A time column therefore has to be a `DATE` here.

What it does not contain is the **wire**: `transport::JobTransport` is the seam, and no
implementor of it ships. Two reasons, and the second is the one that decides it:

1. An outbound HTTP stack plus a credential library is a large dependency addition to a workspace
   that cross-compiles to musl and holds an exact licence allowlist.
2. **Nothing in this repository can verify it.** There is no `BigQuery` in a container, and
   `docs/adr/0017` records that the acceptance leg runs on a developer's own project or nowhere.
   An unverified network client that looks like the feature is worse than a seam that says it is
   one - which is this repository's own rule about an overstated claim, applied to itself.

So this crate is in AGENTS.md's *Built And Not Wired* section, and nothing here may be cited as an
invariant. `sutura-serve` links no `BigQuery` adapter and refuses `kind: bigquery` by name.

# Identity

`BigQueryWarehouse::IMPERSONATION` is `NoPlaceForASubject`, and **that is honest for today
rather than permanent.** A shared service account reaching the dataset for everybody who asks is
the `SharedServiceUser` posture, and it is the posture a developer's own application-default
credential provides - which is the whole of what the login task in this repository serves.
Per-subject execution needs a credential minted per leg through a token exchange, and the
`docs/implementation-plan-bigquery.md` step that builds it is where this constant changes.

Declaring it the other way round to "leave room" would be the exact failure the port's own
documentation warns about: an adapter that accepted subject material it cannot use would report a
leg as impersonated that ran shared.

# Two things this adapter deliberately does not offer

**No arbitrary SQL entry point.** `BigQueryWarehouse::execute` takes an `Executable` and
renders the statement itself; `transport::JobRequest::new` is `pub(crate)`, so there is no way to
hand a statement to a transport from outside this crate.

**No result caching.** Under row-level security a query-keyed cache is a cross-user leak, and this
is the first adapter where there would be row-level security to leak through.

## `enum BigQueryError`

```rust
pub enum BigQueryError<E>
```

Why this data system could not answer.

Generic in the transport's own error, for the reason `sutura_app::ServiceError` is generic in the
adapter's: erasing it here would mean a caller that knows which transport is installed could no
longer tell a refused credential from a dropped connection. The chain still walks - the cause is
an owned `#[source]`.

### Variants

- `Endpoint` - The endpoint did not answer.
- `Render` - The plan would not render.
- `LegWithoutCombiner` - A federated leg arrived, and there is nothing above it to combine legs.
- `NoPlaceForASubject` - Credential material this adapter has nowhere to put.
- `PresentedDisagreesWithPosture` - The leg's credential and this source's declared posture do not agree.
- `UnmappedType` - A column came back as a type this adapter does not map.
- `NotAnInteger` - A cell declared `INT64` did not parse as one.
- `NotADouble` - A cell declared `FLOAT64` did not parse as one.
- `NotABool` - A cell declared `BOOL` was neither `true` nor `false`.
- `NotFinite` - A double came back non-finite.
- `NotADate` - A cell declared as a date did not parse as one.
- `RowWidth` - A row had more or fewer cells than the schema had columns.
- `Incomplete` - The endpoint delivered a page whose row count is not what it reported as total.
- `Shape` - The result set could not be built.

### Implements

`Debug`, `Display`, `Error`

## `struct BigQueryWarehouse`

```rust
pub struct BigQueryWarehouse<T>
```

A `BigQuery` dataset, behind the `Warehouse` port.

Generic in its transport rather than holding a boxed one: there is one per process, it is chosen at
composition, and a generic keeps the transport's own error type visible in `BigQueryError`.

### Methods

```rust
pub const fn new(source: SourceName, posture: SourcePosture, billing_project: ProjectId, default_dataset: DatasetId, transport: T) -> Self
```

Opens a dataset.

**Every argument is required and none has a default**, which is the shape the port asks for and
the reason is different for each: a defaulted posture would be a claim about who a query runs
as that nobody made, and a defaulted billing project would be a project somebody else pays
for. The billing project is the caller's to supply because there is nothing to infer it from -
it is a path segment of the request that submits a job, and a federated identity has no project
of its own.

### Implements

`Debug`, `Warehouse`

## Module `transport`

The one thing this adapter needs from a `BigQuery` endpoint, as a port.

**Why there is a port here at all, when the adapter is already behind one.** `Warehouse` is the
domain's port and this crate implements it; this is a second, much narrower one *inside* the
adapter, and it buys two things that matter more than the indirection costs:

- **Everything this adapter decides becomes testable without a network.** The credential match,
  the posture agreement, the leg refusal, the rendering and the whole value mapping are exercised
  against a fake that returns rows, which is what *ports get fakes, not mocked HTTP* asks for.
- **The dependency decision is isolated to one implementor.** An outbound HTTP stack plus a
  credential library is a large addition to a workspace that cross-compiles to musl and gates
  licences exactly, and it belongs in the change that can first verify it against a real endpoint.
  Nothing in this repository can do that - see the crate documentation.

**What is deliberately NOT here: a method that takes a string.** The request carries a statement
this crate rendered from a plan, and there is no entry point a caller could hand SQL to.

### `enum ParameterMode`

```rust
pub enum ParameterMode
```

How a request writes its bind parameters.

One variant, and it is a variant rather than an absence because the endpoint's own request body
carries this as a field with two values: a query may use positional parameters or named ones and
**not both**, so a transport has to state which it is sending rather than infer it from the text.

Positional is the one this adapter uses, and the decision is recorded on
`sutura_sql::Dialect::placeholder_style`: a rendered statement carries `?` and a
`sutura_sql::GeneratedQuery` carries an ORDERED list of values with no names, because a
parameter's identity in a plan IS its position. Named parameters would need a name invented per
parameter, with nothing in the domain to invent it from.

#### Variants

- `Positional` - `?` in the statement, an ordered array of values carrying no names beside it.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct JobRequest`

```rust
pub struct JobRequest<'job>
```

One query job, as this adapter asks for it.

Borrowed rather than owned throughout: it is built per call, handed to one transport, and dropped.
A clone here would copy the statement for no reason.

#### Methods

```rust
pub const fn billing_project(&self) -> &ProjectId
```

The project this job is billed to.

```rust
pub const fn default_dataset(&self) -> &DatasetId
```

The dataset the statement's unqualified table names resolve in.

**This is why the generated statement needs no qualifying**, and it is the reason the fourth
dialect changed nothing about how a table is rendered: the endpoint's request carries a default
dataset beside the SQL, so a bare backticked table name resolves there. The generator emits the
same shape it emits for every other dialect.

```rust
pub const fn params(&self) -> &[ParamValue]
```

The values, in the order the statement's placeholders take them.

**The order is the contract**, because `ParameterMode::Positional` means the endpoint pairs
the nth value with the nth `?`. A transport that reordered this would send a different query.

```rust
pub const fn statement(&self) -> &str
```

The statement, with its values still absent from it.

#### Implements

`Debug`

### `struct ProjectId`

```rust
pub struct ProjectId
```

The project a job is billed to, as this adapter holds it.

**A wrapper for the reason `sutura_exec_datafusion::WorkingSet` is one:** a project id and a
dataset id are both text, and a call taking two `&str` in the wrong order compiles and is wrong.

**Where the format is checked, and why it is checked twice.** `sutura_config::BillingProject`
refuses an unusable value when the settings tree is READ, so a deployment fails at startup rather
than on its first question - that is a diagnostic job. This type refuses it again because THIS is
the crate whose transport interpolates it into a request path, and a check belongs where the risk
is. The two are not one copy of one rule: an adapter may not depend on the settings tree, so
sharing the type would be an adapter reaching into another adapter.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

The id, for building a request.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnusableResourceName>
```

Parses a project id.

The accepted set is `[a-z0-9-]`, which is what keeps the value inside one URL path segment: no
`/`, no `?`, no `#`, no `%`, no whitespace, nothing non-ASCII. Length is NOT bounded here and is
bounded where the value is declared - this crate's job is that the value cannot escape a
request, and a too-short id is a diagnostic the settings tree already gives.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `struct DatasetId`

```rust
pub struct DatasetId
```

The dataset unqualified table names resolve in, as this adapter holds it.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

The id, for building a request.

```rust
pub fn parse(raw: impl AsRef<str>) -> Result<Self, UnusableResourceName>
```

Parses a dataset id.

`[A-Za-z0-9_]`, case PRESERVED - a dataset id is case-sensitive, so folding it here would turn
a working declaration into a dataset that does not exist.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `enum UnusableResourceName`

```rust
pub enum UnusableResourceName
```

Why a resource name this adapter was handed is not usable.

#### Variants

- `Empty` - Nothing was written, or only whitespace was.
- `Character` - A character that could leave the part of a request this value is written into.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `enum FieldType`

```rust
pub enum FieldType
```

What the endpoint said a column is.

**A closed set plus one named escape**, rather than a passthrough of every type the endpoint can
return. Each variant here is a claim that this adapter maps that type to a domain value and has a
test saying so; `Self::Unmapped` carries the endpoint's own spelling so a type nobody mapped
produces an error NAMING it rather than a null.

#### Variants

- `Int64` - A 64-bit integer.
- `Float64` - A double. Mapped through `Real`, which refuses a non-finite value.
- `Numeric` - An exact decimal - `NUMERIC` or `BIGNUMERIC`. Mapped to TEXT rather than to a double, so an exact total stays exact; `sutura-exec-duckdb` maps its own `Decimal` the same way and for the same sentence.
- `Bool` - A boolean.
- `String` - Text.
- `Date` - A calendar date, as ISO text.
- `Unmapped` - A type this adapter does not map, under the name the endpoint used for it.

#### Methods

```rust
pub fn parse(name: &str) -> Self
```

Decodes a type name the endpoint sends, into the closed vocabulary this adapter maps.

A query response spells the types the legacy way - `INTEGER`/`FLOAT`/`BOOLEAN` - while the
variants here are named after their modern spellings. The transport that reads an answer's
schema calls this, so which spellings become `Int64` is decided HERE, where the value mapping
lives, and not in the unbuilt transport. A name nobody maps becomes `Self::Unmapped` under
the endpoint's own spelling, so an answer is refused NAMING it rather than answered as null.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `struct Field`

```rust
pub struct Field
```

One column, as the endpoint described it.

#### Methods

```rust
pub const fn kind(&self) -> &FieldType
```

What the endpoint said this column is.

```rust
pub fn name(&self) -> &str
```

The label a result column carries.

```rust
pub const fn of(name: String, kind: FieldType) -> Self
```

Names one column.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `enum Cell`

```rust
pub enum Cell
```

One cell, as the endpoint sent it.

**Text or nothing, and that is the endpoint's shape rather than a simplification.** A value in a
query response is a JSON string whatever its declared type is - an integer arrives as `"250"` - so
the mapping from text to a typed domain value is this adapter's work, and `Field::kind` is what
decides it. Modelling it as already-typed here would move that work into the transport, where the
fake and the real implementor would each have to do it and could disagree.

#### Variants

- `Null` - JSON `null`.
- `Text` - A value, as the endpoint spelled it.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `struct JobRows`

```rust
pub struct JobRows
```

A job's result: what the columns are, the rows under them, and how many the job produced.

**The count is part of the result, and that is what makes a partial answer not a result.** The
endpoint's `jobs.query` answers one page - "as many results as can be contained within the
maximum permitted reply size" - and `totalRows` "can be more than the number of rows in this
single page". A first page, or an incomplete job's empty `rows`, is *under the cap, not
truncated*, and this adapter's `rows` refuses a delivered count that does not equal what the
endpoint reported as total - see `super::BigQueryError::Incomplete`.

#### Methods

```rust
pub fn fields(&self) -> &[Field]
```

The columns, in the order the statement projected them.

```rust
pub const fn of(fields: Vec<Field>, rows: Vec<Vec<Cell>>, total_rows: usize) -> Self
```

Assembles a result.

`total_rows` is what the endpoint reported as `totalRows`, which is present only when a job is
complete - so an incomplete job has no value to fill it with, and the transport has to error.

```rust
pub fn rows(&self) -> &[Vec<Cell>]
```

The rows on this page.

```rust
pub const fn total_rows(&self) -> usize
```

What the endpoint said the job's total is, which a delivered page is compared against.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `trait JobTransport`

```rust
pub trait JobTransport
```

A `BigQuery` endpoint, as narrow as this adapter's needs.

Two methods, because the port above it has two questions with different costs: running a job reads
data and is billed, and validating one does neither. The endpoint really does distinguish them -
its request body carries a dry-run flag, and a dry run uses no slots and is not charged - which is
what makes `Warehouse::dry_run` able to answer `PreFlight::Accepted` honestly here rather than
inheriting the port's `NotAsked` default.
