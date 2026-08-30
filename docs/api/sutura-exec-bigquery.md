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

The **wire** - one `transport::JobTransport` that speaks to the endpoint - is `wire`, behind
the default-off `wire` feature. `docs/adr/0018` is the decision that produced it and prices what
it costs; the two reasons it was absent are answered rather than repealed:

1. The dependency addition turned out to be **zero new packages in `Cargo.lock`**, measured:
   `ureq` at the resolved version and features is already in the graph under `libduckdb-sys`. The
   feature is default-off anyway, so which side of the build its TLS stack is compiled on stays a
   decision a composition root makes in a manifest line.
2. **Nothing in this repository can verify it, and that is still true.** There is no `BigQuery` in
   a container, `docs/adr/0017` refuses an emulator on principle, and the acceptance leg -
   `tests/acceptance.rs` - is `#[ignore]`d, needs a project a developer names in their own
   environment, and **has not been run.** So what `wire` claims is that it builds the request it
   says it builds and reads the answer it says it reads; acceptance is not claimed anywhere.

So this crate is still in AGENTS.md's *Built And Not Wired* section, and nothing here may be cited
as an invariant. `sutura-serve` links no `BigQuery` adapter and refuses `kind: bigquery` by name,
and the `data_systems:` axis of the golden matrix still gains no entry - a cell that has never
executed reads as coverage.

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
  credential source is a real addition to a workspace that cross-compiles to musl and gates
  licences exactly, and it arrives in exactly one place: `crate::wire`, behind the crate's
  default-off `wire` feature. `docs/adr/0018` prices it. What is still true is the sentence that
  kept the seam empty for a release: **nothing in this repository can verify a network client**,
  so the acceptance leg is opt-in, needs a developer's own project, and is unexecuted.

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

## Module `wire`

The WIRE: one `JobTransport` that speaks to a `BigQuery` endpoint over HTTP.

**This is the seam `docs/adr/0017` left open, filled in by the decision `docs/adr/0018` records.**
Behind the crate's default-off `wire` feature, because what arrives with it is an outbound TLS
stack and two of the four release triples are musl; that manifest argument is on the `ureq` entry
in the workspace root and is not repeated here.

# What this module claims, and what it does not

**It has never been run against a real project by anybody in this repository, and nothing here
pretends otherwise.** There is no `BigQuery` in a container - `docs/adr/0017` refuses an emulator
on principle rather than on cost, because it is the option that produces the most
confident-looking green - so what the suite beside this module proves is that *this code builds
the request it says it builds and reads the answer it says it reads*, over documents that are not
the service's. That is a real property and it is smaller than acceptance. The acceptance leg is
`crates/sutura-exec-bigquery/tests/acceptance.rs`, it is `#[ignore]`d, it needs a project a
developer names in their own environment, and **it is unexecuted.**

So: still *Built And Not Wired* in AGENTS.md, one line further along. What moved is that a
developer with a project can now try it; what did not move is that `sutura-serve` links no
`BigQuery` adapter and refuses `kind: bigquery` by name, and the `data_systems:` axis of the
golden matrix still gains no entry - a cell that has never executed reads as coverage.

# The four things this module decides

- **One page or a refusal.** `jobs.query` answers one page, and completeness is stated as
  `totalRows` beside the rows rather than by the rows alone. A `pageToken`, an incomplete job or a
  delivered count short of the reported total is refused here - see `WireError::MoreThanOnePage`
  and `WireError::NotComplete` - because to `answer()` a first page would read as *under the
  cap, not truncated*, which is the exact row the row-cap invariant exists to hold.
- **The service's own result cache is turned OFF.** Not for cost: an anchor that reproduces from a
  cache has reproduced the cache, which is `differential.rs`'s own argument. And a cached answer
  under a *shared* identity is shared across every asker, so leaving it on would put the
  cross-user leak this crate refuses one layer below the code the per-subject step has to change.
- **The bearer never travels to a second host.** `max_redirects` is `0`, so there is no redirect
  for a credential to follow. That is stronger than the client's own default, which is to strip
  the header on redirect: here there is nothing to strip.
- **Every foreign string that reaches an error is bounded and filtered.** The endpoint's
  `reason` is kept and its free-text `message` is not, because a reason is a fixed vocabulary an
  operator can act on and a message is unbounded text from another service heading for a log.

# What is deliberately absent

- **Paging.** A result bigger than one page is refused rather than assembled. `getQueryResults`
  needs the job's `location` for a dataset outside the two multi-regions, and
  `SourcePlacement::BigQuery` declares no `location` - `docs/adr/0017` says why that field is not
  in this repository yet and that the change adding the wire is the one that decides it. **This
  change decides it by not needing it**, and the cost is the refusal above.
- **A `location` on the request.** Same reason, one size smaller.
- **Retries.** A refused job comes back as `WireError` and reaches a caller as
  `BigQueryError::Endpoint`, whose transport-facing status is a `503`. Retrying inside an adapter
  would spend a caller's request timeout on a decision the caller cannot see.

### `enum WireError`

```rust
pub enum WireError<C>
```

Why the endpoint did not answer with rows.

Generic in the credential source's own error, for the reason `crate::BigQueryError` is generic
in this one: a caller that knows which credential source is installed can still tell a missing
file from a refused refresh, and erasing it here would be the information this whole chain of
generics exists to keep.

**`ureq::Error` appears as a `#[source]` and never as a variant this type re-exports**, which is
the shape *Structured Errors* asks for at a boundary: the variant is ours, the chain still walks,
and a caller who knows the transport can downcast. It is boxed because it is much larger than
every other variant and `clippy::result_large_err` is on.

#### Variants

- `Credential` - No token could be produced, so nothing was sent.
- `Expired` - A token was produced and its deadline had already passed.
- `NoClock` - This process could not read a wall clock.
- `RequestNotSerializable` - The request could not be serialized.
- `Unreachable` - The endpoint was not reached.
- `Unreadable` - The endpoint answered and the answer could not be read.
- `Refused` - The endpoint refused.
- `NotADocument` - The answer was not the document a query response is.
- `Failed` - The endpoint accepted the job and reported errors against it.
- `NotComplete` - The job had not finished when the endpoint answered.
- `MoreThanOnePage` - The answer is one page of more than one.
- `NoTotal` - A complete job that stated no total.
- `NotATotal` - The total was not a number.
- `NoSchema` - A complete job with rows and no schema to read them against.
- `NotAScalar` - A cell that is neither a string nor a null.

#### Implements

`Debug`, `Display`, `Error`

### `struct BigQueryWire`

```rust
pub struct BigQueryWire<C>
```

A `BigQuery` endpoint, reached over HTTP.

Generic in its credential source rather than holding a boxed one, for the reason
`crate::BigQueryWarehouse` is generic in its transport: there is one per process, it is chosen
at composition, and a generic keeps the source's own error type visible in `WireError`.

#### Methods

```rust
pub const fn new(agent: ureq::Agent, credentials: C) -> Self
```

Opens a transport.

The `agent` is a parameter rather than something built here so it can be the same one the
credential source refreshes through. `agent` is the only function that builds one.

#### Implements

`Debug`, `JobTransport`

### `fn agent`

```rust
pub fn agent() -> ureq::Agent
```

The client every request in this crate goes through.

**Public, and handed to both the transport and the credential source**, so the two share one
connection pool and one set of the decisions below - which is what makes *the redirect policy is
set once* true rather than true twice.

Every non-default setting is a decision:

Plain backticks below rather than intra-doc links, because this function is public and the two
items it names are private - `api-docs` reports that pairing as a warning, and the safe form for
it is the same one `AGENTS.md` prescribes for a cross-crate reference.

- `http_status_as_error(false)`, because the client's default turns a `4xx` into an error and
  discards the body - and the body is where the endpoint says *which* refusal this is. Status is
  read explicitly instead, in `refusal`.
- `https_only(true)`, so a bearer token cannot leave over plaintext even if a URL somewhere loses
  its scheme. `HOST` is already `https`; this is the second lock.
- `max_redirects(0)`, so the credential has no second host to reach. See the module header.
- `timeout_global`, so a job cannot hold a pool thread open indefinitely.
- `max_response_header_size`, because headers are read before the body's own limit applies.

### Module `credential`

Where the bearer token a job is submitted with comes from.

**A second narrow port, for the reason [`JobTransport`](crate::transport::JobTransport) is one.**
The wire needs one thing from a credential - a token that is usable right now, and the instant it
stops being usable - and everything else about how a deployment authenticates is somebody else's
decision. So `AccessTokens` is that one thing, and the transport is generic in it.

**This is also the seam per-subject execution arrives at**, which is why it is a port on the first
day rather than a `String` field. `docs/implementation-plan-bigquery.md`'s second `BigQuery` step
mints a token *per leg, for the subject who asked*; under a `String` that step would have to change
the transport, and under a port it adds an implementor. Nothing here anticipates it further than
that: `Bearer` carries the deadline because a minted token has one, and
`crate::BigQueryWarehouse`'s `IMPERSONATION` still says `NoPlaceForASubject` because nothing
mints one. (Plain backticks on the constant, because it is a `Warehouse` trait item rather than an
inherent one and an intra-doc link to it does not resolve - `api-docs` reported that as a broken
link, which is the one class of rustdoc warning `AGENTS.md` says can reach `mkdocs --strict`.)

# What ships, and what does not

`ApplicationDefault` reads the file `gcloud auth application-default login` writes and exchanges
its refresh token for an access token. That is exactly the fixture
[`docs/adr/0017`](https://github.com/telekom/sutura/blob/main/docs/adr/0017-what-a-bigquery-test-runs-against.md)
decided - a real project reached from a developer's own machine, under the `SharedServiceUser`
posture - and `just gcloud-login` is what produces it.

**Three other credential shapes exist and none of them is built**, each refused by name rather
than mishandled:

- a **service-account key**, which needs an `RS256` assertion signed with a private key. The
  signing is the dependency decision, not the flow: `jsonwebtoken` is already in this workspace
  but with `use_pem` off, so a key in PEM form has nothing to parse it. `docs/adr/0018` prices it.
- the **metadata server**, which is how a deployment on the provider's own compute gets a token
  with no key at all. It is a plain unauthenticated `GET` and would cost nothing in dependencies;
  what it costs is that **nothing in this repository can verify it**, because it exists only
  inside that provider's network.
- **workload or workforce identity federation**, which is the per-subject step and an
  architecture decision with an owner outside this repository.

# Nothing is cached, and that is a decision

A token is minted for every job. The obvious alternative - hold the last one until it expires -
is refused for three reasons, in ascending order of how much they matter:

1. `clippy.toml` disallows `std::sync::Mutex` in this workspace, so the cache would arrive with a
   new dependency for the primitive to hold it.
2. *"A credential is not reused past its expiry"* is one of the assertions the per-subject step
   owes, and the shape that cannot get it wrong is the one with nothing to reuse.
3. **It is the credential-shaped version of the cache this crate already refuses.** `lib.rs` says
   a query-keyed result cache is a cross-user leak under row-level security; a *token* cache keyed
   by nothing is the same defect one layer down, and it would be sitting in the code the
   per-subject step has to change. A cache that is correct for one identity and wrong for many is
   worse than no cache, because it works until the day it matters.

**The cost, stated rather than waved at:** one extra `HTTPS` round trip per job, against a query
that costs seconds and money. When that is measured to matter, what arrives is a cache keyed by
whose credential it is - which is a thing only the per-subject step can key.

#### `struct Bearer`

```rust
pub struct Bearer
```

A token usable now, and when it stops being usable.

**The deadline travels with the token rather than beside it**, so a caller cannot present one and
forget the other. `Expiry` is the domain's own vocabulary for this, which matters because the
per-subject step reports the same value through `sutura_domain::audit::CallRecord`.

##### Methods

```rust
pub const fn not_after(&self) -> Expiry
```

When it stops being usable.

```rust
pub const fn of(token: Secret, not_after: Expiry) -> Self
```

Names a token and its deadline.

```rust
pub const fn token(&self) -> &Secret
```

The token, still opaque. A caller has to reach `Secret::expose` to write it into a header, and
that call is greppable.

##### Implements

`Clone`, `Debug`

#### `trait AccessTokens`

```rust
pub trait AccessTokens
```

Where the token a job is submitted with comes from.

One method, and it takes the clock as an argument. **The clock is a parameter for the reason
`Expiry::passed_by` takes one:** an implementor that reads the wall clock itself cannot be tested
against a deadline, and the expiry logic is the half of a credential source most likely to be
wrong in the direction nobody notices. Who reads the real clock is the transport, once.

#### `struct CredentialFile`

```rust
pub struct CredentialFile
```

The file an application-default credential lives in.

A newtype rather than a `PathBuf` argument, because `Self::well_known` and `Self::at` are two
different claims - *wherever this machine keeps it* and *this exact file* - and a function taking a
path cannot tell which one it was handed.

##### Methods

```rust
pub fn at(path: impl Into<PathBuf>) -> Self
```

This exact file.

```rust
pub fn path(&self) -> &Path
```

The path, for reading it and for a message that says which file was wrong.

```rust
pub fn well_known() -> Result<Self, NoWellKnownLocation>
```

Wherever this machine keeps it.

Three places, in the order the tooling itself uses: the explicit variable, then the relocated
configuration directory, then the default under the user's home. **`HOME` is read and no
fallback is invented** - a process with no home directory has no well-known location, and
guessing one would be reading a credential from a path nobody chose.

##### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

#### `struct NoWellKnownLocation`

```rust
pub struct NoWellKnownLocation
```

Why the well-known location could not be worked out.

One variant, and it carries no path: the refusal is that this machine named no home directory, and
the fix is to say where the file is with `CredentialFile::at`.

##### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

#### `enum UnusableCredential`

```rust
pub enum UnusableCredential
```

Why the file could not become a credential.

**Every variant names what is wrong and none of them quotes the file's contents.** A path is
carried where the fix is *which file*, and a field NAME is carried where the fix is *what is
missing*; the value never is, because in this file every value is either a secret or a project
identifier.

##### Variants

- `Unreadable` - The file could not be opened or read.
- `TooLarge` - The file is larger than any credential document is.
- `NotADocument` - The file is not the JSON document this expects.
- `NotAUserCredential` - The file is a credential shape this build does not implement.
- `Incomplete` - An `authorized_user` document missing one of the three fields a refresh needs.
- `AnotherUniverse` - The credential was minted against a different service universe than the one this build talks to.

##### Implements

`Debug`, `Display`, `Error`

#### `enum RefreshFailed`

```rust
pub enum RefreshFailed
```

Why a refresh did not produce a token.

##### Variants

- `Unreachable` - The token endpoint did not answer.
- `Unreadable` - The token endpoint answered, and the answer could not be read.
- `Refused` - The token endpoint refused.
- `NotADocument` - The answer was not the JSON document a token response is.
- `NoToken` - The answer carried no token.
- `AlreadyExpired` - The answer's own deadline had already passed when it arrived.

##### Implements

`Debug`, `Display`, `Error`

#### `struct ApplicationDefault`

```rust
pub struct ApplicationDefault
```

An access-token source built from the file `gcloud auth application-default login` writes.

**One identity for everybody who asks**, which is `SharedServiceUser` and is the posture
`crate::BigQueryWarehouse` declares. On a developer's machine that identity is the developer.

##### Methods

```rust
pub fn read(file: &CredentialFile, agent: ureq::Agent) -> Result<Self, UnusableCredential>
```

Reads a credential file.

The `agent` is handed in rather than built here, so the refresh and the job share one
connection pool and one `crate::wire::agent` configuration - which is what makes "the
redirect policy is set once" true rather than true twice.

##### Implements

`AccessTokens`, `Debug`
