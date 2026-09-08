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
- the value mapping, which is where a wrong number would come from;
- the boot pre-flight, which asks each dataset once - not once per model - whether it holds the
  tables the bundle names, so a mistyped table name costs a boot refusal here as it already does
  on a `files` deployment rather than a failed answer for whoever asks first.

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
2. **Nothing in CI can verify it; a developer's own project now has.** On 2026-08-30 the three
   `#[ignore]`d tests in `tests/acceptance.rs` passed against a real dataset under a
   service-account key - the first statement this repository generated to be accepted by
   `BigQuery`. **What that one is, exactly:** one hand-built `SUM` over a two-column fixture, so it
   says nothing about a join, `COUNT(DISTINCT`, `CASE WHEN`, a `NULLIF` ratio or `ISOWEEK` - and
   the last is one of the two constructs `docs/adr/0017` measured the parse check to be blind
   about. **The corpus-wide leg that record specifies is `tests/corpus.rs`, beside it**, behind the
   default-off `fixtures` feature: it loads the example fixtures into four tables through
   `BigQueryWarehouse::load_fixture`, runs the corpus questions, and compares its rows with the
   engine's for the same plan. That is where the join, the ratio and `ISOWEEK` are reached.

So this crate is still built and not wired - see `.agents/skills/sutura/query-surface` - and nothing here may be cited
as an invariant. `sutura-serve` links no `BigQuery` adapter and refuses `kind: bigquery` by name,
and the `data_systems:` axis of the golden matrix still gains no entry - **and the reason for that
last one has changed rather than gone away.** It was *a cell that has never executed reads as
coverage*; the corpus leg executes, so what keeps the entry out now is that a cell in that registry
runs inside `just test` and this one cannot: the nix sandbox has no network, so acceptance is a
`nix run` app and not a `checks.*` output.

# Identity

`BigQueryWarehouse::IMPERSONATION` is `PerSubjectCredential`, which is what makes a source
executed as the asking subject representable here: the credential a broker mints for the asker is
carried as a `Presented::SubjectToken` and sent as this job's bearer, so the dataset evaluates
the statement under whoever that token is. The `wire`'s own credential source stays for the
shared posture. Per-subject execution still needs a broker that mints a per-leg credential through
a token exchange - this crate performs no exchange, it presents one - and that broker lives beside
the composition root that links this adapter, which is the half `docs/implementation-plan-bigquery.md`
describes as not wired.

**ONE of the two subject shapes, and the other is refused rather than degraded.** The domain's
`Presented::SubjectPrincipal` is a principal the data system switches to on a connection the
DEPLOYMENT authenticated, and `BigQuery` has no such mechanism; it is the same POSTURE as a
subject token, so `Presented::agrees_with` passes it and only this adapter can say it has
nowhere to put it. `BigQueryError::NoPrincipalSwitch` is that refusal, and the reason it is a
refusal is the reason the whole-shape `NoPlaceForASubject` it replaced existed: a leg accepted
here would be submitted under the transport's own credential while provenance, read off this
source's posture, reported the answer as impersonated.

**What no version of this is:** a deployment where a served source executes as its asker.
`sutura-serve` refuses an `impersonation-at-source` `bigquery` entry by name, because no broker
that exchanges is attached to a served source yet - see that crate's `build_bigquery`.

# Two things this adapter deliberately does not offer

**No arbitrary SQL entry point.** `BigQueryWarehouse::execute` takes an `Executable` and
renders the statement itself; `transport::JobRequest::new` is `pub(crate)`, so there is no way to
hand a statement to a transport from outside this crate. **The `fixtures` feature does not open
one:** `load_fixture` takes a table name and a path, and `crate::importer` renders the statement
from names that parsed and cells that parsed - refusing, rather than escaping, a cell that could
close a literal.

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
- `NoPrincipalSwitch` - The leg presents a principal for the data system to switch to, and there is no such mechanism here.
- `PresentedDisagreesWithPosture` - The leg's credential and this source's declared posture do not agree.
- `UnmappedType` - A column came back as a type this adapter does not map.
- `NotAnInteger` - A cell declared `INT64` did not parse as one.
- `NotADouble` - A cell declared `FLOAT64` did not parse as one.
- `NotABool` - A cell declared `BOOL` was neither `true` nor `false`.
- `NotFinite` - A double came back non-finite.
- `NotADate` - A cell declared as a date did not parse as one.
- `RowWidth` - A row had more or fewer cells than the schema had columns.
- `Incomplete` - The endpoint delivered a page whose row count is not what it reported as total.
- `NoIdentityInTheAnswer` - The identity read came back as something other than one row of one text cell.
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
pub fn drop_table(&self, table: &TableName) -> Dropped<<T as >::Error>
```

Removes one table from the connection's dataset.

**The tidy half of per-run fixture cleanup.** A run names its tables with a per-run suffix
(see `tests/corpus.rs`), so what it removes is its OWN tables and never a colleague's. The
`crate::importer` header states the guarantee half - every `CREATE` also carries a 24-hour
expiration, because `panic = "abort"` means a cancelled runner never reaches this method and
the expiration is what still cleans up after it.

It takes a table name and never a statement, for the same reason `load_fixture` does: the
statement is rendered from a name that parsed, and *no arbitrary SQL entry point* stays true.

Behind the same `fixtures` feature and in the same impl block, for the same two reasons.

```rust
pub fn load_fixture(&self, table: &TableName, csv: &std::path::Path) -> Loaded<<T as >::Error>
```

Replaces one table in the connection's dataset with the rows of a committed fixture CSV.

**The mirror of #78's `PostgresWarehouse::load_csv`, and it exists for the reason that one
does: a relational data system has to be GIVEN tables before a corpus can be run against it,
and the example models are files.** The differences from the Postgres shape are in
`crate::importer`'s header - there is no `COPY`, so the rows travel inside the statement and
every cell is re-rendered from a parsed value.

**Behind the `fixtures` feature, so no shipped build holds it.** `Cargo.toml` carries that
argument. What it buys over a `#[cfg(test)]` helper is that the acceptance leg is an
INTEGRATION target - a separate crate - which cannot reach a test-gated item here.

It takes a table name and a path and never a statement, which is what keeps *no arbitrary SQL
entry point* true of this crate: the statement is rendered from names that parsed and cells
that parsed.

**In THIS impl block rather than in the module that renders the statement**, because
`clippy::multiple_inherent_impl` is denied here and it is right to be: a type whose inherent
methods are spread over files is one whose surface nobody can read in one place.

Returns how many data rows the fixture carried, so a caller can assert the load moved what the
file holds rather than trusting a green.

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

```rust
pub fn session_user(&self, presented: &Presented) -> Result<String, BigQueryError<<T as >::Error>>
```

Who this data system says the leg presenting `presented` is executing AS.

**The observable for the claim this adapter's `IMPERSONATION` constant makes.** A
`Presented::SubjectToken` rides as this job's own bearer, so what the endpoint resolves
that bearer to IS the identity the source executed under - and asking the source rather than
asserting it is the difference between evidence and a comment. `docs/adr/0008` names
`SESSION_USER()` as the primitive; `SESSION_USER` is the only statement this can issue.

It goes through `Self::deliverable` like every other credential-taking method, so a leg
whose credential disagrees with the source's posture is refused here too rather than being
answered by a read that looks harmless.

**Not part of the `Warehouse` port, and that is a decision rather than an omission.** No
other adapter can answer it - `sutura-exec-datafusion` and `sutura-exec-duckdb` execute in
process under one identity, so a defaulted method would answer *the process* and read as
though it had asked. An inherent method is reachable by the one venue that needs it and by
nothing that federates.

# Errors

`BigQueryError::Endpoint` where the endpoint did not answer,
`BigQueryError::Incomplete` where the page and the reported total disagree, and
`BigQueryError::NoIdentityInTheAnswer` where the answer is not one row of one text cell.
Nothing here quotes what came back: see that variant.

### Implements

`Debug`, `Warehouse`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

## `use None`

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
  default-off `wire` feature. `docs/adr/0018` prices it. The sentence that kept this seam empty for
  a release - *nothing in this repository can verify a network client* - is now half spent: nothing
  in CI can, and a developer's own project has. Three tests passed against a real dataset on
  2026-08-30, over one hand-built `SUM` rather than the corpus.

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

```rust
pub const fn subject_bearer(&self) -> Option<&Secret>
```

The asking subject's own credential, where the leg carried one.

**This is the half that makes a `BigQuery` source execute as the asker.** A
`Presented::SubjectToken` carries the
credential a broker minted for the asking subject - an exchanged Google access token scoped
to that subject - and the transport sends it as its bearer for THIS job, so the endpoint
evaluates the statement under whoever the token says. `None` for the shared posture, whose
leg runs under the identity the transport itself already holds.

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

**`Ord` is derived so the pre-flight can group by it**, and the ordering it derives is the inner
string's: `parse` neither trims into a different value nor folds case, so the wrapper compares
exactly as the text it holds does and there is no invariant for the derive to disagree with.

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

`Clone`, `Debug`, `Eq`, `Ord`, `PartialEq`, `PartialOrd`

### `struct DatasetId`

```rust
pub struct DatasetId
```

The dataset unqualified table names resolve in, as this adapter holds it.

`Ord` for the reason `ProjectId`'s is derived, plus one of its own: this type PRESERVES case, so
the derived ordering and the derived equality are the case-sensitive comparison a dataset id
really wants.

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

`Clone`, `Debug`, `Eq`, `Ord`, `PartialEq`, `PartialOrd`

### `struct DatasetAddress`

```rust
pub struct DatasetAddress
```

One dataset, addressed the way a metadata read needs it: who pays, where it lives, and its id.

**A named struct rather than loose arguments**, for the reason `ProjectId` is a wrapper at all:
a call taking ids of the same shape in the wrong order compiles and is wrong. It is also the
grouping key the pre-flight uses, which is what the derived `Ord` is for.

**THREE fields and not two, and the third one is a review finding rather than symmetry.** The
first shape of this type carried the dataset's project only, and the listing then sent that as
the quota project - so a source declared `billing_project: acme-analytics` reading a model at
`partner-data.shared.dim_region` attributed the listing to `partner-data`, which the caller holds
no `serviceusage.services.use` on. It would have 403'd and become a permanent warning, while a
QUERY against the same table attributed to `acme-analytics` and worked. `docs/adr/0018` states
the rule as *carrying the source's declared billing project*.

**What that says about the newtype, written down because it is the interesting half:** a wrapper
per id prevents an argument-ORDER mistake and permits a ROLE mistake - *where the dataset lives*
against *who pays* - and the role mistake is the one that happened. Two accessors named for their
roles is the fix that a single `project` field could not be.

#### Methods

```rust
pub const fn billed_to(&self) -> &ProjectId
```

The project whose quota and billing this read is attributed to: the SOURCE's, always.

Read into `x-goog-user-project` where the credential requires that header, which is the one
place the distinction from `Self::project` bites - see this type's own documentation.

```rust
pub const fn dataset(&self) -> &DatasetId
```

The dataset's own id.

```rust
pub const fn of(billed_to: ProjectId, project: ProjectId, dataset: DatasetId) -> Self
```

Addresses a dataset: the source's billing project, the dataset's own project, and its id.

The first two are equal for an unqualified model and differ for a cross-project one, which is
exactly the case the role distinction exists for.

```rust
pub const fn project(&self) -> &ProjectId
```

The project the dataset LIVES in, which is the one written into the request path.

Not the one the read is attributed to - `Self::billed_to` is.

#### Implements

`Clone`, `Debug`, `Eq`, `Ord`, `PartialEq`, `PartialOrd`

### `struct HeldTables`

```rust
pub struct HeldTables
```

Every table one dataset holds, by the id it knows each under - and what the listing said about
how many there were supposed to be.

**A struct rather than the `BTreeSet<String>` alias it was, and the second field is the whole
reason.** The set alone cannot tell an EMPTY dataset from a document whose shape the service
changed: both arrive as no ids at all, and the pre-flight reads no ids as *every table is
absent*. `ListingTotal` is what the two can be told apart by.

**And exactly that far, which is the limit next to the claim.** It reaches a shape change the
same document still reports a readable count beside: a service that re-spelled `totalItems` as
well leaves `ListingTotal::Unreported` or `ListingTotal::Unreadable`, and those say *nothing
to compare* rather than *empty dataset*. Nor does it reach a dataset every one of whose ids this
crate drops - that is `ListingTotal::Accounted` beside no ids, deliberately, because it is an
ordinary dataset no model in the bundle could have named anyway.

**Why it travels on the answer rather than being decided here, which is not the same as *it
could not be*:** `JobTransport::listing_was_refused` is proof that this port can hold a
decision on the layer above's behalf. So the layer is a CHOICE, and the reason it is this one is
that the choice is not settled - `docs/adr/0018` states why refusing is not obviously the safe
direction - and a transport that turned the value into a verdict would have taken it.

The set is still what the pre-flight asks with, and `Self::holds` is its only question;
`Self::named` is for a diagnostic and for a test, not for a count anything concludes from.

#### Methods

```rust
pub fn holds(&self, id: &str) -> bool
```

Whether the dataset holds a table under exactly this id.

Case-SENSITIVE, because `GoogleSQL` does not fold a table name - `BigQueryWarehouse::preflight`
carries the argument, and this is the call it makes.

```rust
pub const fn named(&self) -> &BTreeSet<String>
```

Every id the listing named.

```rust
pub const fn of(named: BTreeSet<String>, total: ListingTotal) -> Self
```

The ids a listing named, and what its own total said about them.

```rust
pub const fn total(&self) -> ListingTotal
```

What the listing's own reported total said about the ids the same document carried.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

### `enum ListingTotal`

```rust
pub enum ListingTotal
```

What a listing's own reported total said, against the entries of the same document whose table
id it could read.

**Four variants rather than an `Option<u64>`, because each says something different about what a
caller may conclude** - and the two that mean *nothing to compare* are the ones a boolean would
have merged with the answer. A reader has to name the case, for the reason
`sutura_domain::source::AnchorIdentity` names `NoneDeclared` rather than answering `None`.

**The comparison is against the entries that carried a table id this crate could READ - neither
the ids the listing NAMED nor the entries it merely counted**, and each half of that is a wrong
claim avoided. An id outside `usable_table_id`'s accepted set is DROPPED from the named set, and
`BigQuery` permits such an id, so comparing against the named set would report an ordinary
dataset as short of its own total. The entry count is the mistake the other way, and it was this
type's first shape: a document whose `tableReference` the service renamed or nested carries
entries and no readable id, which read `Self::Accounted` over no ids at all - the pre-flight
reporting every table absent while the cross-check read clean. An entry with no readable id is
the shape signal; an id `usable_table_id` rejected is the legitimate drop, and it still counts.

The same cross-check one document over is `crate::BigQueryError::Incomplete`, which compares
`delivered` against `total` on a query answer and REFUSES. Two vocabularies for one shape, named
here so a reader who greps one finds the other: that one refuses because a short result set is a
wrong number, and this one cannot, because a short listing is a boot warning.

#### Variants

- `Unreported` - The document carried no total at all, so an empty listing and an empty dataset are one value.
- `Unreadable` - It carried a total this crate could not read as a count.
- `Accounted` - It reported a total, and carried a readable table id for every table the total claims.
- `Short` - It reported MORE tables than the same document carried readable table ids for.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `enum NamedResource`

```rust
pub enum NamedResource
```

Which of this adapter's two resource names a refusal is about.

**A variant rather than the `&'static str` this field used to be**, because *Structured Errors*
says the variant is the contract and the message is not: a test can assert `Self::Project`
and a rename is then a compile error at the assertion, where a string compare kept passing while
asserting the old spelling. The `core::fmt::Display` impl is the one place the operator-facing
wording lives, so `UnusableResourceName`'s sentences read exactly as they did.

#### Variants

- `Project` - The project a job is billed to - `ProjectId`.
- `Dataset` - The dataset unqualified table names resolve in - `DatasetId`.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `PartialEq`

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

Two methods PUT A QUESTION TO THE ENDPOINT, because the port above it has two questions with
different costs: running a job reads data and is billed, and validating one does neither. The
endpoint really does distinguish them - its request body carries a dry-run flag, and a dry run
uses no slots and is not charged - which is what makes `Warehouse::dry_run` able to answer
`PreFlight::Accepted` honestly here rather than inheriting the port's `NotAsked` default.

**Three more members are not that, and the count is spelled out because it has been wrong
twice.** `result_did_not_fit` asks the implementor about a failure it already has and sends
nothing; `list_tables` sends a metadata read rather than a statement, which is what makes it
cheap enough for a boot check; and `apply`, behind the `fixtures` feature, is the second
statement-issuing method - present only in a build that loads fixtures, so no deployment can
reach it.

## Module `wire`

The WIRE: one `JobTransport` that speaks to a `BigQuery` endpoint over HTTP.

**This is the seam `docs/adr/0017` left open, filled in by the decision `docs/adr/0018` records.**
Behind the crate's default-off `wire` feature, because what arrives with it is an outbound TLS
stack and two of the four release triples are musl; that manifest argument is on the `ureq` entry
in the workspace root and is not repeated here.

# What this module claims, and what it does not

**This HAS now been run against a real project, and that is new.** On 2026-08-30 the three tests
in `crates/sutura-exec-bigquery/tests/acceptance.rs` passed against a real dataset from a
developer's machine, under a service-account key: the endpoint accepted a statement this repository
generated, answered it as one complete page, and the numbers were the fixture's. **It is the first
time anything here has had a statement accepted by `BigQuery`.**

**What that does NOT establish, stated first because a green run invites the larger reading.** It
is ONE hand-built `SUM` over a two-column fixture - no join, no `COUNT(DISTINCT`, no `CASE WHEN`,
no `NULLIF` ratio, no `CAST(... AS FLOAT64)`, no `ISOWEEK` - and `ISOWEEK` plus `DATE_TRUNC`'s
argument order are precisely the two things `docs/adr/0017` MEASURED the parse check to be blind
about. The leg that record specifies is the corpus compared against the engine, and it is not
built. So: *one statement accepted*, not *the corpus accepted*.

What the suite beside this module proves is separate and unchanged: that *this code builds the
request it says it builds and reads the answer it says it reads*, over documents that are not the
service's.

So: still built and not wired - `.agents/skills/sutura/query-surface` - two lines further along. `sutura-serve` links no
`BigQuery` adapter and refuses `kind: bigquery` by name, and the `data_systems:` axis of the
golden matrix still gains no entry - one live statement is not a registered data system.

# What this module decides, and every one of them is pinned by a TYPE or by a test

- **A job is bounded in TIME and in MONEY, and neither bound is a constant here.** `JobBounds`
  carries both, `WireAgent` carries the `JobBounds`, and `BigQueryWire` can only be built from
  a `WireAgent` - so there is no way to submit a job this deployment did not bound. `jobTimeoutMs`
  is what cancels a job at the service (`timeoutMs` alone does NOT: it bounds how long the client
  waits, and an expired one leaves the job running and billing), and `maximumBytesBilled` is what
  stops a question scanning a petabyte - neither the row cap nor the one-page refusal bounds bytes
  scanned.
- **The time bound is ONE ABSOLUTE DEADLINE PER CALL, not a timeout per HTTP operation, and this
  bullet exists because the earlier shape was the second thing while claiming the first.** A single
  call does a token exchange and then a job; `timeout_global` on the agent gave each of them a full
  budget of its own, so a review measured one ANSWER - `dry_run` then `execute`, two exchanges and
  two jobs - at four independent budgets against a transport whose own request timeout is thirty
  seconds. `CallDeadline` is opened once in `submit` and every operation below it gets only what
  is LEFT: the exchange's socket, the job's socket, and the `timeoutMs`/`jobTimeoutMs` the request
  carries. A budget spent before the job is `WireError::DeadlineSpent` rather than a send.
  **The limit, because it is the half a type here cannot reach:** neither `Warehouse` nor
  `JobTransport` takes a deadline, so the two calls one answer makes cannot share one - an
  answer's worst case is `QueryDeadline::CALLS_PER_ANSWER` budgets. That arithmetic is done once,
  in `QueryDeadline::within_request_timeout`, so a composition root gets a deadline that already
  fits inside the request timeout instead of a number it has to divide correctly.
- **One page or a refusal.** `jobs.query` answers one page, and completeness is stated as
  `totalRows` beside the rows rather than by the rows alone. The wire refuses a `pageToken`
  (`WireError::MoreThanOnePage`) and a job that did not finish (`WireError::NotComplete`); the
  delivered count that is not the reported total is refused one port further out, in the adapter's
  `BigQueryWarehouse::rows` as `BigQueryError::Incomplete` - `complete` here compares nothing, it
  hands the rows and the total to the adapter - because to `answer()` a first page would read as
  *under the cap, not truncated*, which is the exact row the row-cap invariant exists to hold.
  **And a wide
  result now leaves as a REFUSAL rather than as a `503`, which is a correction to what this header
  used to say was the cost.** It used to reach a caller as `BigQueryError::Endpoint`, which both
  transports answer as the status a dead endpoint produces - inviting a retry that returns the same
  page. `ResultTooLarge` is what it means, and the port can now say it: `result_did_not_fit` on
  `crate::transport::JobTransport` answers it for `WireError::MoreThanOnePage`, the adapter
  passes it up through `Warehouse::result_did_not_fit`, and a caller gets `413 result_too_large`
  carrying `ResultBound::Volume` - a bound with no number, because the reply cap is the service's
  and it reports neither that nor the size of the reply that hit it. `NotComplete` deliberately
  answers `false`: a job that ran out of time may finish on a retry.
- **The service's own result cache is turned OFF.** Not for cost: an anchor that reproduces from a
  cache has reproduced the cache, which is `differential.rs`'s own argument. And a cached answer
  under a *shared* identity is shared across every asker, so leaving it on would put the
  cross-user leak this crate refuses one layer below the code the per-subject step has to change.
- **The bearer's DESTINATION is a compile-time constant; its ROUTE is not, and the difference is
  worth stating precisely** because an earlier version of this header overstated it.
  `HOST` cannot be configured, `https_only` is on and `max_redirects` is `0`, so nothing a
  deployment writes can change *which service* receives the credential. What a deployment CAN
  change is the path: `ureq`'s default config is `Proxy::try_from_env()`, so `HTTPS_PROXY` routes
  these requests through an egress proxy. That is left ON deliberately - an egress proxy is a real
  deployment shape here, `docs/enterprise-mirrors.md` is the generic form of it - and it is safe
  because the tunnel is still TLS to `HOST` verified against a compiled-in root set, so a proxy
  sees a hostname and no bytes. It is written out in `WireAgent::pinned` rather than inherited,
  so it is a
  decision a reviewer can disagree with.
- **Failure is derived from the RESULT SHAPE and never from `errors` being non-empty.** The
  endpoint documents that array as *"the first errors or warnings encountered"* and says entries
  *"do not necessarily mean that the job has completed or was unsuccessful"* - so refusing on it
  would decline successful queries that merely warned. What refuses is `jobComplete`, a
  `pageToken`, an absent `totalRows`, and a delivered count that is not the reported total - the
  last of those in the adapter (`BigQueryWarehouse::rows`, `BigQueryError::Incomplete`), not here;
  the reported
  `reason` is folded into whichever of those fires, because it is the best diagnostic
  available at that point. See `complete`, and the limit stated there.
- **Refusal text is bounded and filtered, not discarded.** `credential::bounded` handles the
  `reason`; `EndpointMessage` retains the free-text `message` and redacts it under `Debug`
  only. `Display` and cause-chain logging can still render the message.

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
- **Surfacing a warning on a result that IS complete.** There is nowhere to put it: `RowSet` has
  no field for it and this crate has no logging dependency, so adding one for a line nobody has
  ever seen is a dependency decision this change does not take. Stated because a dropped warning
  is exactly the kind of absence that reads as "there were none".

### `struct WireAgent`

```rust
pub struct WireAgent
```

The client every request in this crate goes through, with the four settings that matter PINNED BY
THE TYPE rather than by a call site.

**This newtype is the whole mechanism, and it exists because the previous shape was a convention.**
The settings below used to live in a free function returning a bare `ureq::Agent`, and both
`BigQueryWire::new` and `credential::ApplicationDefault::read` accepted any agent - so a
composition root writing `ureq::Agent::new_with_defaults()` got redirects on, plaintext allowed
and no timeout, while every test passed because the tests all called the right function. A private
field with one constructor is what *a newtype parses rather than validates* asks for: if an
instance of this exists, the pins hold.

It also carries the `JobBounds`, so the deadline that shapes the socket timeout and the deadline
that goes into the request body are **the same value**. Two arguments could have disagreed.

#### Methods

```rust
pub const fn bounds(&self) -> JobBounds
```

What every job through this client is bounded by.

```rust
pub fn pinned(bounds: JobBounds) -> Self
```

The one constructor, and every non-default setting below is a decision:

- `http_status_as_error(false)`, because the client's default turns a `4xx` into an error and
  discards the body - and the body is where the endpoint says *which* refusal this is. Status is
  read explicitly instead, in `refusal`.
- `https_only(true)`, so a bearer token cannot leave over plaintext even if a URL somewhere
  loses its scheme. `HOST` is already `https`; this is the second lock.
- `max_redirects(0)`, so the credential has no second host to reach. `ureq-proto` also strips
  `authorization` on a redirect, which was verified rather than assumed - so this is belt and
  braces, and the belt is ours.
- `timeout_global`, at the job's deadline plus `CONNECT_MARGIN`, so the socket cannot outlive
  the job it is waiting for by more than connection setup.
- `max_response_header_size`, because headers are read before the body's own limit applies.
- `proxy(Proxy::try_from_env())`, which is the client's own default WRITTEN OUT rather than
  inherited. An egress proxy is a legitimate deployment shape and the tunnel is still TLS to
  `HOST` against a compiled-in root set, so what the environment chooses is the route and not
  the destination. The module header states that distinction, because a previous version of it
  claimed the stronger thing.

#### Implements

`Clone`, `Debug`

### `struct EndpointMessage`

```rust
pub struct EndpointMessage
```

The endpoint's own message on a refusal: free text, and the one field here that can name an
account.

**A type rather than a `String`, because the rule it carries is about RENDERING and a rule about
rendering cannot be held at call sites.** `Display` is the message; `Debug` is redacted. That is
the whole mechanism, and it is here because the alternative was asking fourteen acceptance legs
to remember which formatter they used.

**Measured, which is why this exists.** A leg ending `.expect("the endpoint answered")` formats
its error with `Debug`, and `Debug` walks the struct: on a real refusal that printed
`Access Denied: ... permission: <an account>` into a public workflow log. Ten of the fourteen
legs `nix run .#bigquery-acceptance` invokes were in exactly that shape, and the job's
`::add-mask::` step covers the project, the dataset and the table - **not an account**.
`Display` keeps the message because a `400` with only a reason code is undiagnosable, which is
what `docs/adr/0018` prices.

**What this does NOT do, and the earlier wording here claimed otherwise.** It said a caller
"has to ask for the sentence by name". It does not: `WireError::Refused`'s own `Display`
interpolates `detail`, so anything that walks a cause chain and `to_string()`s each link renders
it. `sutura_app::surface::cause_chain` does exactly that, and its output reaches
`tracing::error!` in the HTTP and agent transports - reachable from a `sutura-serve --features
bigquery` deployment. That path is **pre-existing and deliberate**: this workspace flattens a
cause chain at the sink, and a deployment's own log is not the public workflow log this
redaction targets. So the scope of the control is exactly one thing - **`Debug`**, which is what
a panicking test leg prints into a world-readable CI log - and it is not a general answer to
where the endpoint's message may travel.

It is already bounded and stripped on the way in - see `Self::bounded`.

#### Methods

```rust
pub fn as_str(&self) -> &str
```

The message itself, for a caller that has decided it may render it.

Named rather than reached through `Deref`, which `cargo xtask check-newtype-leaks` refuses:
a wrapper you can forget you are holding is not a wrapper.

```rust
pub fn bounded(message: Option<String>) -> Self
```

The endpoint's message, capped and stripped of anything that could forge a log line.

Infallible: an absent message is an empty one, which is honest - the status is what is
guaranteed.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `PartialEq`

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
- `DeadlineSpent` - This call's budget was gone before the job could be submitted.
- `RequestNotSerializable` - The request could not be serialized.
- `Unreachable` - The endpoint was not reached.
- `Unreadable` - The endpoint answered and the answer could not be read.
- `Refused` - The endpoint refused.
- `NotADocument` - The answer was not the document a query response is.
- `NotComplete` - The job had not finished when the endpoint answered.
- `MoreThanOnePage` - The answer is one page of more than one.
- `NoTotal` - A complete job that stated no total.
- `NotATotal` - The total was not a number.
- `NoSchema` - A complete job with rows and no schema to read them against.
- `NotAScalar` - A cell that is neither a string nor a null.
- `NotAListing` - The answer to a table listing was not one. Distinct from `Self::NotADocument`, the same failure for a query answer: two documents, two shapes, and one message per request.
- `UnusablePageToken` - The service handed back a page token this transport will not write into a URL. **Refused rather than filtered**, and `tables::usable_token` carries the argument; the token travels through `bounded`, which keeps a foreign string out of a log unbounded.
- `ListingDidNotFinish` - A dataset that did not finish listing inside the page bound. **A failure rather than a short listing**: this feeds *these tables are absent*, so a cut-off listing reports a table that is there as missing.

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

It holds a `WireAgent` and not a `ureq::Agent`, which is what makes the module header's claims
properties of this type rather than of whichever function a composition root happened to call.

#### Methods

```rust
pub const fn new(agent: WireAgent, credentials: C) -> Self
```

Opens a transport.

The `WireAgent` is a parameter rather than something built here so it can be the same one
the credential source refreshes through - one connection pool, one set of pins, and one
`JobBounds` shared by the socket timeout and the request body.

#### Implements

`Debug`, `JobTransport`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### `use None`

### Module `bounds`

What one job may spend: the TIME bound, the MONEY bound, and the per-call budget they are
charged against.

**Split out of `wire.rs` when that file reached the thousand-line limit `cargo xtask max-lines`
enforces and cannot exempt**, at the cut `document.rs` already made once. The seam is the first
bullet of `wire.rs`'s own header - *a job is bounded in TIME and in MONEY, and neither bound is a
constant here* - and everything in this file is one of those two bounds, the arithmetic that
keeps them inside the transport's own request timeout, or the refusal a deployment gets when the
number it wrote cannot be used. Nothing here opens a socket, holds a credential or builds a
document, which is what makes it the half a test reaches without a project.

Why each bound is shaped the way it is stays with the type; `wire.rs`'s header is the one place
they are argued together.

#### `struct QueryDeadline`

```rust
pub struct QueryDeadline
```

How long a job may run, and how long the client waits for its answer.

**A newtype rather than a constant, because the value belongs to the deployment.** The setting that
decides it is the one the transport in front of this service already uses -
`server.request_timeout_seconds`, which ships as 30 - and a constant in this file would be a second
copy of it that drifts the day somebody changes the first.

**It is a SHARE of that setting rather than the setting itself**, which review had to point out:
one answer makes `Self::CALLS_PER_ANSWER` calls and each pays `CONNECT_MARGIN` on top of its
own budget, so filling this with 30 gives a caller who waits 30 seconds a query that may still be
running. `Self::within_request_timeout` is the constructor that does the division, and it is the
one a composition root should reach for; `Self::parse` stays for a deployment stating a budget
outright.

##### Methods

```rust
pub const fn budget(self) -> Duration
```

The whole budget, as a duration.

```rust
pub const fn milliseconds(self) -> u64
```

The deadline in milliseconds, which is the unit both request fields take.

`saturating_mul` rather than `*`, and it cannot saturate: `Self::MAX_SECONDS` times a
thousand is far inside `u64`. Written that way because a bound that could wrap is a bound that
could become zero, and zero is the one value `Self::parse` refuses.

```rust
pub const fn parse(seconds: u64) -> Result<Self, UnusableBound>
```

Parses a deadline in whole seconds.

```rust
pub const fn socket(self) -> Duration
```

How long a socket may stay open for a call that has spent none of its budget yet.

**The backstop on the agent rather than the bound that holds.** What a single operation is
really allowed is `CallDeadline::socket(left)` over what is LEFT of the call's budget - see
`CallDeadline`, and see the module header for why a per-operation timeout was not enough. This
value is what the agent is configured with, so an operation that somehow reached the client
without an override is still bounded.

```rust
pub const fn within_request_timeout(request_timeout_seconds: u64) -> Result<Self, UnusableBound>
```

The largest deadline that keeps one ANSWER inside a transport's own request timeout.

**The arithmetic a composition root would otherwise have to remember, and get wrong.** The
number to fill this from is `server.request_timeout_seconds`, which ships as thirty; what a
caller wants is not that number but the share of it one call may spend, because an answer makes
`Self::CALLS_PER_ANSWER` calls and each pays `CONNECT_MARGIN` on top of its own budget. So
`within_request_timeout(30)` is ten seconds, and two calls of ten plus five is the thirty a
caller was promised.

A request timeout too short to leave anything is `UnusableBound::NoBudget` rather than a
silently clamped value, because a deployment whose timeout cannot fit a query wants to be told
so at startup.

##### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

#### `struct BytesBilledCeiling`

```rust
pub struct BytesBilledCeiling
```

The most a single job may be billed for scanning.

**Sent as `maximumBytesBilled`, which is enforced at the service and is what makes it worth
more than a client-side check.** A job that would exceed it FAILS and is not charged. Nothing else
in this repository bounds bytes scanned: `LIMIT 10001` bounds rows RETURNED, the one-page refusal
bounds a page, and `MAX_ANSWER_BYTES` bounds what is read into memory - a question can satisfy
all three and still scan a partitioned table end to end.

##### Methods

```rust
pub fn as_text(self) -> String
```

The ceiling, as the request body writes it.

**Text, because the endpoint writes and reads 64-bit integers as JSON strings.** A number here
would be silently truncated to a double by a strict reader at the far end.

```rust
pub const fn parse(bytes: u64) -> Result<Self, UnusableBound>
```

Parses a ceiling in bytes.

##### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

#### `enum UnusableBound`

```rust
pub enum UnusableBound
```

Why a bound this adapter was handed is not usable.

##### Variants

- `Zero` - Zero, which would refuse every question rather than bounding one.
- `TooLarge` - Above what the endpoint accepts, or above what a bound is for.
- `NoBudget` - A transport's request timeout too short to leave a job any budget at all.

##### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

#### `struct CallDeadline`

```rust
pub struct CallDeadline
```

The instant one call into this transport has to be finished by.

**One absolute deadline for the whole of one call, rather than a timeout per HTTP operation - and
that distinction is the correction this type exists to carry.** The previous shape put
`timeout_global` on the agent, so EVERY request through it got the full budget independently: a
single `crate::transport::JobTransport::run` does a token exchange and then a job, and both were allowed
`deadline + CONNECT_MARGIN` of their own. A review measured the consequence at the answer level -
four HTTP operations, each with its own budget, against a transport whose own request timeout is
thirty seconds - and the five-second overrun this module claimed was false.

So the budget is opened once per call and every operation gets only what is LEFT of it: the token
exchange, the socket the job waits on, and the `timeoutMs` and `jobTimeoutMs` the request carries -
which is what keeps the service cancelling at the instant the client stops waiting even when the
exchange spent half the budget first. When nothing is left, the refusal comes before the send.

**A monotonic `std::time::Instant` and not a wall clock**, because a wall clock can step and a
stepped deadline is either a job abandoned early or one that outlives its caller.

**The limit, and it is the half this type cannot reach:** one ANSWER calls the port twice -
`Warehouse::dry_run` and then `Warehouse::execute` - and neither `Warehouse` nor `crate::transport::JobTransport`
takes a deadline, so the two calls cannot share one. An answer's worst case is therefore
`CALLS_PER_ANSWER` budgets rather than one, which is exactly why
`QueryDeadline::within_request_timeout` exists: it does that arithmetic once so a composition root
cannot get it wrong. Carrying one deadline across the port is an architecture decision, not a
signature tweak.

##### Methods

```rust
pub fn opened(deadline: QueryDeadline) -> Self
```

Opens a budget now.

```rust
pub const fn opened_at(started: std::time::Instant, deadline: QueryDeadline) -> Self
```

Opens a budget that started at a named instant.

**The canonical constructor, with `Self::opened` delegating to it**, and it is public for one
reason: a caller cannot otherwise construct a budget that is already spent, so the refusal at
the end of one could not be reached from a test without sleeping through a real one.

```rust
pub fn remaining(self) -> Option<Duration>
```

What is left of the budget, or `None` when it is spent.

`None` rather than a zero duration, because zero means *no timeout* to the client underneath -
so handing it on would turn a spent budget into an unbounded wait, which is the opposite of what
this type is for.

```rust
pub const fn socket(left: Duration) -> Duration
```

How long a socket may stay open for an operation with `left` of the budget remaining: that,
plus connection setup.

##### Implements

`Clone`, `Copy`, `Debug`

#### `struct JobBounds`

```rust
pub struct JobBounds
```

What every job this adapter submits is bounded by.

Two bounds in one value, because they are one decision: *how much of a deployment's time and money
may one question spend*. A struct rather than two arguments so a call site cannot supply one and
forget the other, and so `super::WireAgent` can carry them both.

##### Methods

```rust
pub const fn deadline(self) -> QueryDeadline
```

How long a job may run.

```rust
pub const fn max_bytes_billed(self) -> BytesBilledCeiling
```

The most one job may be billed for scanning.

```rust
pub const fn of(deadline: QueryDeadline, max_bytes_billed: BytesBilledCeiling) -> Self
```

Names both bounds. Neither has a default, for the reason `BigQueryWarehouse::new` gives about
its own arguments: a defaulted deadline is a promise nobody made, and a defaulted ceiling is
money somebody else pays.

##### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### Module `credential`

Where the bearer token a job is submitted with comes from.

**A second narrow port, for the reason `JobTransport` is one.**
The wire needs two things from a credential - a token that is usable right now, and whether a
request carrying it has to name a quota project - and everything else about how a deployment
authenticates is somebody else's decision. So `AccessTokens` is those two things, and the
transport is generic in it.

It is a port on the first day rather than a `String` field, so *which* credential shape a
deployment holds is a choice of implementor. `Bearer` carries the deadline because a minted token
has one, and whether this adapter has anywhere for a subject's own credential to arrive is
declared by `crate::BigQueryWarehouse`'s `IMPERSONATION` rather than by anything this port
decides - and that declaration is deliberately the only copy of the value. This sentence used to
carry a second copy and stated the OPPOSITE of it for three commits, on a published page;
`check-guidance` refuses the shape now, so the correction is to stop encoding the value rather
than to keep two copies in step.

**What this is NOT, and the correction is review's rather than a hedge:** this port is not yet the
seam at which per-subject execution arrives as *merely another implementor*. Three signatures say
so - `Warehouse::execute` takes a `&Presented` and `BigQueryWarehouse` reads it only to call
`deliverable`; `JobTransport::run` takes a `JobRequest` and nothing else; and `AccessTokens::bearer`
takes a clock and a budget. So an implementation behind this port **cannot select a credential for
the presented subject and cannot tell two concurrent subjects apart.** The step that builds
per-subject execution has to carry the leg's subject or its credential context through one of those
three interfaces, and which one is part of that change rather than something anticipated here.
`docs/adr/0018` records it in the same words.

# Two credential kinds, as one closed shape

`Credential` reads either of the two files a deployment can actually have, and **which one it is
is a closed two-variant shape rather than a struct of `Option`s.** That matters for a reason
stronger than tidiness: a document carrying *both* a refresh token and a private key is
unrepresentable here, so there is no state in which it is ambiguous which flow will run or which
credential material was used.

| Kind | Who holds it | The exchange |
| --- | --- | --- |
| `authorized_user` | a developer, from `just gcloud-login` | trade a refresh token |
| `service_account` | CI, and a deployment | sign an assertion and trade that |

**Both are `SharedServiceUser` and neither is a step towards per-subject execution.** One identity
reaches the dataset for everybody who asks; on a laptop that identity is the developer and in CI it
is a service account. `docs/adr/0017`'s amendment is where the decision to run the acceptance leg
in CI on the second kind lives.

**The signing costs no new dependency, which was verified rather than assumed.** `ring` is already
in the graph - it is `ureq`'s and `tokio-rustls`'s crypto provider - and it carries
`RsaKeyPair::from_pkcs8` plus `RSA_PKCS1_SHA256`, which is exactly the primitive and exactly the
key encoding a service-account key uses. `base64` is already resolved too. So `docs/adr/0018`'s
446-to-446 measurement survives this, and the alternative that would have cost a package -
`jsonwebtoken`'s `use_pem`, which pulls `simple_asn1` because its DER path wants `PKCS#1` while a
service-account key is `PKCS#8` - was priced and refused.

**What IS first-party here is the JWT's text, and that boundary is deliberate.** `ring` computes the
signature; this module base64url-encodes two JSON documents and joins them with dots.
`docs/adr/0014` draws exactly that line when it argues for hand-writing a metrics exposition format
and against hand-writing signature verification in the same breath: one is a text format, the other
is cryptography. And this side SIGNS rather than verifies, which is where algorithm confusion does
not live - the algorithm is a constant, not a field read off somebody else's document.

# Two credential shapes exist and neither is built

- the **metadata server**, which is how a deployment on the provider's own compute gets a token
  with no key at all. It is a plain unauthenticated `GET` and would cost nothing in dependencies -
  the cheapest option available. It is out because **nothing in this repository can verify it**: it
  exists only inside that provider's network, so building it would add an unexercised code path to
  a module whose whole point is that it does not claim more than it has.
- **workload or workforce identity federation**, which is the per-subject step and an architecture
  decision with an owner outside this repository.

# Nothing is cached, and both the decision and its REASON were wrong once

A token is minted for every call into the endpoint. **The decision stands; the paragraph that
justified it did not, in two ways a review caught, and both are corrected here because the
per-subject step will read this as the argument it inherits.**

**The cost, counted properly.** `sutura_app::answer` calls `Warehouse::dry_run` and then
`Warehouse::execute`; each goes through the wire's `submit`, and each calls
`AccessTokens::bearer`. So one question is **two** token exchanges before its two job round
trips, and **every anchor verified at boot is one more**. The earlier wording - *"one extra round
trip per job"* - was half the real number and counted the wrong unit.

**The reason, corrected.** The earlier version said a token cache keyed by nothing is the
credential-shaped version of the result cache this crate refuses. That is true of a cache shared
across SUBJECTS and false here: a `Credential` **is** one identity, so a token held until its
`not_after` is keyed by exactly the thing that matters and leaks to nobody. The
`std::sync::Mutex` ban in `clippy.toml` is not an argument either - `sutura-http`'s own key-set
cache holds a lock.

**So the honest reason is the small one: it is not needed until it is measured.** Minting is one
`HTTPS` round trip against a query that costs seconds and money, and the shape with nothing to
reuse is the shape that cannot get *a credential is not reused past its expiry* wrong - which is one
of the assertions the per-subject step owes. **What that step must NOT inherit is a prohibition**,
because caching per subject, keyed by subject, is a different question this paragraph does not
answer.

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

The token, still opaque. A caller has to reach `Secret::expose_secret` to write it into a header, and
that call is greppable.

##### Implements

`Clone`, `Debug`

#### `enum QuotaProject`

```rust
pub enum QuotaProject
```

Whether a request has to name the project whose quota and billing it is attributed to.

Two variants and no `Option`, because "the credential already says" is a real answer rather than a
missing one - which is the same argument `Expiry` makes for having no `Option`.

##### Variants

- `Required` - State it on the request. An application-default credential is an END-USER credential, and the endpoint's own direct-REST guidance requires a quota project for one - without it a perfectly valid token is refused, with a message about user credentials not being supported that reads as an authentication fault and is not one.
- `FromTheCredential` - The credential carries its own project, so stating one would add a permission requirement - `serviceusage.services.use` on that project - that a service account holding only dataset grants does not have. **So the header that MAKES the first kind work BREAKS the second**, which is why this is a two-variant answer and not a constant on the request.

##### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

#### `trait AccessTokens`

```rust
pub trait AccessTokens
```

Where the token a job is submitted with comes from.

The clock is a parameter for the reason `Expiry::passed_by` takes one: an implementor that reads
the wall clock itself cannot be tested against a deadline, and the expiry logic is the half of a
credential source most likely to be wrong in the direction nobody notices. Who reads the real clock
is the transport, once.

#### `struct CredentialFile`

```rust
pub struct CredentialFile
```

The file a credential lives in.

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

**Every variant names what is wrong and none of them quotes a value from the file.** A path is
carried where the fix is *which file*, a field NAME where the fix is *what is missing*, and the
credential's own `type` only after `crate::wire::bounded` has cut it to a fixed character set -
because a 16 KiB file can put 16 KiB of newlines there and this string reaches a log.

##### Variants

- `Unreadable` - The file could not be opened or read.
- `TooLarge` - The file is larger than any credential document is.
- `NotADocument` - The file is not the JSON document this expects.
- `UnknownKind` - The file names a credential shape this build does not implement.
- `Incomplete` - A document missing one of the fields its own kind needs.
- `AnotherUniverse` - The credential was minted against a different service universe than the one this build talks to.
- `UnreadableKey` - The private key is not a `PKCS#8` PEM block holding a key this build can sign with.

##### Implements

`Debug`, `Display`, `Error`

#### `enum KeyUnusable`

```rust
pub enum KeyUnusable
```

How far a private key got before it was refused.

**Three stages rather than one boolean, because the fix for each is a different thing.** A missing
delimiter is a truncated or wrongly-encoded file; a body that is not base64 is a corrupted one; a
body that decodes and is not a key is a key of the wrong kind - a `PKCS#1` block whose delimiters
somebody rewrote, an EC key, or a truncated DER. None of the three quotes anything.

##### Variants

- `NotAPemBlock` - The `PKCS#8` delimiters are absent, or there is nothing between them.
- `NotBase64` - The body between the delimiters is not base64.
- `NotAKey` - The body decodes and is not a `PKCS#8` RSA key this build can sign with.

##### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

#### `enum TokenUnavailable`

```rust
pub enum TokenUnavailable
```

Why no token came back.

##### Variants

- `Unreachable` - The token endpoint did not answer.
- `Unreadable` - The token endpoint answered, and the answer could not be read.
- `Refused` - The token endpoint refused.
- `NotADocument` - The answer was not the JSON document a token response is.
- `NoToken` - The answer carried no token.
- `AlreadyExpired` - The answer's own deadline had already passed when it arrived.
- `DeadlineSpent` - The call's budget was gone before the exchange could be attempted.
- `Unsigned` - The assertion could not be signed.
- `NotSigned` - The signature itself failed.

##### Implements

`Debug`, `Display`, `Error`

#### `struct Credential`

```rust
pub struct Credential
```

A credential this build can present, read from a file.

**One type for both kinds, so a caller does not have to know which file it has.** CI points
`GOOGLE_APPLICATION_CREDENTIALS` at a service-account key and a laptop has an
application-default login; both reach the endpoint through this.

##### Methods

```rust
pub const fn kind(&self) -> &'static str
```

Which kind this is, for a banner or a test. A fixed word, never the file's own text.

```rust
pub const fn project(&self) -> Option<&String>
```

Which project this credential names, where it names one.

`Some` for a service-account key and `None` for an application-default login, which is the
difference the two files actually have. **This is why CI configures no project variable:** the
key carries it, so a second declaration would be a second answer to *who pays* that can
disagree with the first.

```rust
pub fn read(file: &CredentialFile, agent: WireAgent) -> Result<Self, UnusableCredential>
```

Reads a credential file, whichever of the two kinds it holds.

**The agent is a `WireAgent` and not a `ureq::Agent`, which is the point of that newtype:**
the exchange and the job then share one connection pool and one set of pins by construction,
rather than because two call sites happened to call the same builder. A previous version took
any agent, so a composition root could have exchanged a credential over a client with redirects
on and no timeout.

**The two doctests below are the mechanism, not decoration.** The first is the mistake the
newtype exists to make impossible; the second is its compiling twin, so the failure is the
missing pin rather than a typo in the example.

```compile_fail
use sutura_exec_bigquery::wire::credential::{Credential, CredentialFile};

// A client with redirects on, plaintext allowed and no timeout. There is no way to hand it in.
let _ = Credential::read(&CredentialFile::at("/nonexistent"), ureq::Agent::new_with_defaults());
```

```
use sutura_exec_bigquery::wire::credential::{Credential, CredentialFile};
use sutura_exec_bigquery::wire::{BytesBilledCeiling, JobBounds, QueryDeadline, WireAgent};

let bounds = JobBounds::of(
    QueryDeadline::parse(30).expect("a deadline"),
    BytesBilledCeiling::parse(1024 * 1024).expect("a ceiling"),
);
// Compiles, and refuses at run time because the path is not there - which is the point: what
// the first example cannot get past is the TYPE, before any file is read.
let refused = Credential::read(&CredentialFile::at("/nonexistent"), WireAgent::pinned(bounds));
assert!(refused.is_err());
```

##### Implements

`AccessTokens`, `Debug`
