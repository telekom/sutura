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
   `BigQuery`. **What that one is, exactly:** one hand-built `SUM` over a two-column
   fixture, so it says nothing about a join, `COUNT(DISTINCT`, `CASE WHEN`, a `NULLIF` ratio or
   `ISOWEEK` - and the last is one of the two constructs `docs/adr/0017` measured the parse check
   to be blind about. **The corpus-wide leg is `tests/corpus.rs`**, behind the default-off
   `fixtures` feature: it loads the example fixtures into four tables through
   `BigQueryWarehouse::load_fixture`, runs the corpus questions, and compares its rows with the
   engine's for the same plan. That is where the join, the ratio and `ISOWEEK` are reached.

So nothing here may be cited as an invariant. `sutura serve` DOES link this adapter and dispatch
`kind: bigquery` behind its default-off `bigquery` feature - `docs/adr/0017`'s second amendment
records the day the last *not wired* was spent. A default build links none of it, and
the `data_systems:` axis of the golden matrix gains no entry - because a cell in that registry
runs inside `just test` and this one cannot: the nix sandbox has no network, so acceptance is a
`nix run` app and not a `checks.*` output.

# Identity

`BigQueryWarehouse::IMPERSONATION` is `PerSubjectCredential`, which is what makes a source
executed as the asking subject representable here: the credential a broker mints for the asker is
carried as a `Presented::SubjectToken` and sent as this job's bearer, so the dataset evaluates
the statement under whoever that token is. The `wire`'s own credential source stays for the
shared posture. Per-subject execution still needs a broker that mints a per-leg credential through
a token exchange - this crate performs no exchange, it presents one - and that broker lives beside
the composition root that links this adapter: `crates/sutura-cli/src/serve/broker.rs` composes
`sts::WorkloadIdentityBroker` today - built, though the run that proved the exchange drove no
served binary, so a served source has not executed as a caller yet
(`docs/where-identity-is-proven.md`).

**ONE of the two subject shapes, and the other is refused rather than degraded.** A
`Presented::SubjectPrincipal` is a principal the data system switches to on a connection the
DEPLOYMENT authenticated, and `BigQuery` has no such mechanism; it is the same POSTURE as a
subject token, so `Presented::agrees_with` passes it
and only this adapter can say it has nowhere to put it. `BigQueryError::NoPrincipalSwitch` is
that refusal, and the reason it is a refusal is the reason the whole-shape `NoPlaceForASubject`
it replaced existed: a leg accepted here would be submitted under the transport's own credential
while provenance, read off this source's posture, reported the answer as impersonated.

**What no version of this is:** a deployment where a served source executes as its asker.
`sutura serve` refuses an `impersonation-at-source` `bigquery` entry by name, because no broker
that exchanges is attached to a served source yet - see `sutura-cli`'s
`crates/sutura-cli/src/serve/bigquery.rs`, `build_bigquery`.

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

  **A refusal to execute rather than an execution**, worded as `sutura-exec-duckdb` words it: a
  leg run with nothing above it returns rows at a finer grouping than the question asked for,
  which is a wrong number under a certified name.
- `NoPrincipalSwitch` - The leg presents a principal for the data system to switch to, and there is no such mechanism here.

  **The narrow half of a refusal that used to be wholesale, and it has to stay refused.** This
  adapter declares `PerSubjectCredential` and delivers exactly one of the two subject shapes: a
  `SubjectToken` rides as this job's bearer, so the
  dataset evaluates the statement under whoever the token is. `BigQuery` has no proxy-user or
  `SET ROLE` equivalent for a `SubjectPrincipal`,
  so a leg carrying one has no material to send - and
  `agrees_with` passes it, because the two shapes are
  the same POSTURE. Accepting
  it would submit the job under the credential the transport already holds while provenance,
  read off this source's posture, reported the answer as impersonated: every row as the
  process, recorded as the asker.
- `PresentedDisagreesWithPosture` - The leg's credential and this source's declared posture do not agree.
- `UnmappedType` - A column came back as a type this adapter does not map.

  It NAMES the type rather than answering null, which is the whole reason
  `FieldType::Unmapped` carries the endpoint's own
  spelling.
- `NotAnInteger` - A cell declared `INT64` did not parse as one.

  **Two variants rather than one carrying a `&'static str`, because the CAUSE differs.** The
  endpoint sends every value as text, so "declared an integer" and "parses as an integer" are
  two facts, and the standard-library error that says why is worth keeping on the chain.
- `NotADouble` - A cell declared `FLOAT64` did not parse as one.
- `NotABool` - A cell declared `BOOL` was neither `true` nor `false`.

  No `#[source]`: there is no parse behind it, because the check is a comparison against the two
  spellings the endpoint documents. A variant with an invented cause would be worse than none.
- `NotFinite` - A double came back non-finite.

  **What this arm actually guards, on THIS target, is narrower than the two SQL adapters
  agreeing.** In `GoogleSQL` the `/` operator raises on a zero divisor for every numeric type -
  only `IEEE_DIVIDE` answers `inf`/`NaN` - so an unguarded zero-division ratio fails at the
  service first, as `Self::Endpoint` with the same `503` as a dead data system. What reaches
  this arm is a non-finite value STORED in a `FLOAT64` column, and the check keeps that stored
  `Infinity` from answering a real under a certified metric name. It is `sutura-exec-duckdb`'s
  same arm that gives `zero_denominator: fails` its meaning, because there the unguarded `/`
  does answer `inf`; the sentence that credits this arm with the ratio case belongs to `DuckDB`.
- `NotADate` - A cell declared as a date did not parse as one.
- `RowWidth` - A row had more or fewer cells than the schema had columns.

  Distinct from `Self::Shape`: this one is the ENDPOINT disagreeing with itself, caught before
  a row is built, so the position of the offending row is reportable.
- `Incomplete` - The endpoint delivered a page whose row count is not what it reported as total.

  `jobs.query` answers one page at a time, and completeness is stated as `totalRows` beside the
  rows - never by the rows alone. A first page, or an incomplete job's empty `rows`, would read
  to `answer()` as *under the cap, not truncated*: a wrong number under a certified name, through
  the exact row the row-cap invariant exists to hold. So a delivered count that does not equal the
  reported total is refused here, at the seam, rather than certified.
- `NoIdentityInTheAnswer` - The identity read came back as something other than one row of one text cell.

  Its own variant rather than `Self::RowWidth` or `Self::Shape`, because what a caller does
  about it is different: those two are a result set this adapter could not map, and this is
  *the endpoint did not tell us who ran the job* - which for the one caller that asks
  (`BigQueryWarehouse::session_user`) is the whole
  answer rather than a cell of it.

  **It carries the SHAPE and never the value**, deliberately. The one thing this answer can
  contain is an account identifier, and the venue that reads it writes to a public log - so a
  refusal that quoted what came back would be the disclosure the read exists to check for.
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

Behind the same `fixtures` feature and in the same impl block, for `load_fixture`'s reasons.

```rust
pub const fn dry_run_estimate_agrees_with_its_declaration(estimated_bytes: Option<EstimatedBytes>) -> bool
```

Whether an accepted pre-flight's own estimate agrees with what `Warehouse::PRICES_DRY_RUN`
declares.

**The same comparison `sutura_conformance::execute`'s pack makes** over the three adapters
`execute_packs!` binds - none of which is this one (`telekom/sutura#710`) - named here so it
can be checked against this adapter's own dry-run path without that binding. A live
endpoint's own guarantee that it always prices one is still unverified; this only compares
what an already-answered pre-flight carried against the declaration.

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
pub fn session_user(&self, presented: &Presented) -> Result<SessionUser, BigQueryError<<T as >::Error>>
```

Who this data system says the leg presenting `presented` is executing AS.

**The observable for the claim this adapter's `IMPERSONATION` constant makes.** A
`Presented::SubjectToken` rides as this job's own bearer, so what the endpoint resolves
that bearer to IS the identity the source executed under - and asking the source rather than
asserting it is the difference between evidence and a comment. `docs/adr/0008` names
`SESSION_USER()` as the primitive; `SESSION_USER` is the only statement this can issue.

It goes through `Self::deliverable` like every other credential-taking method, so a leg
whose credential disagrees with the source's posture is refused here too rather than being
answered by a read that looks harmless. The `SessionUser` answer redacts under `Debug`;
explicit access and `Display` still reveal it. Neither this read nor its return type
establishes how the bearer was obtained.

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

## `use SessionUser`

An endpoint identity answer whose `Debug` never renders its contents.

`Self::as_str` and `Display` expose the unchanged answer. This is a `Debug` boundary,
not a restriction on intentional logging, nor validation or authentication of the identity.

## `use Dropped`

What a DROP answers with: nothing, or why it did not happen.

Named for the same reason `Loaded` is - `Result<(), FixtureNotLoaded<T::Error>>` is over the
`type_complexity` threshold this workspace tightened, and the generic error is the point.

## `use FixtureNotLoaded`

Why a fixture did not reach the dataset.

Two shapes rather than one, because a defect in a file in this repository and a refusal from the
endpoint are different problems for whoever reads the failure: the first is fixed in a diff and
the second is a grant, a quota or a dataset that is not there.

## `use FixtureNotUsable`

Why a committed fixture cannot become a table.

Every variant is a defect in a file in this repository rather than an input to handle, which is
why none of them carries the offending text: whoever sees one has the file.

## `use Loaded`

What one load answers with: the row count, or why it did not happen.

Named because `Result<usize, FixtureNotLoaded<T::Error>>` is over the `type_complexity` threshold
this workspace tightened, and for the reason `crate::Mapped` is named: the generic error is the
point, and erasing it would lose which transport failed.

## `use ImpersonateAsAccount`

The second hop, telekom/sutura#376's iamcredentials step: a federated access token in, a
service-account access token out.

**A second port and not a second `StsExchange` method** - the two calls have different request
and response shapes (RFC 8693 token exchange vs `{scope, lifetime}`) and different failure modes
(STS `invalid_target` vs `iamcredentials`'s own `403` for "may not impersonate"). Everything this
broker decides about WHEN to call it is exercised against a fake; the real HTTP call arrives at
this port as `crate::wire::IamCredentialsOverHttp`, behind the same default-off `wire` feature
`StsExchange`'s real implementor is.

## `use NoImpersonation`

The impersonation port a broker holds when it was never wired to one.

**The default for the same reason `SystemClock` is one for the clock parameter**: a composition
root that never calls `WorkloadIdentityBroker::impersonating_via` gets a broker whose TYPE says
the hop cannot run, rather than a value that happens never to be invoked. Every test and every
existing call site that declares no `impersonate` entry at all never reaches
`ImpersonateAsAccount::impersonate` - `WorkloadIdentity::target_for` answers `None` for every
subject, so `mint` never calls it.

**A composition root can still reach it by mistake**, declaring a source's `impersonate` map
without ever calling `WorkloadIdentityBroker::impersonating_via` - the type system does not
forbid attaching an `I` and a per-source map independently, since `WorkloadIdentityBroker::impersonating`
(the per-source declaration) takes no `I` at all. So this is a REFUSAL, not an
invariant asserted with `unreachable!`: a caller whose subject resolves a target here is told the
composition is wrong, the same way `ExchangeUnusable::Provider` tells it about any other
provider defect, rather than the process panicking on a request nobody malformed.

## `use StsCredential`

One exchanged credential: a Google access token and the instant it stops being usable.

The deadline is carried beside the token - the whole point of `docs/adr/0008`'s
`Expiry` - so a broker can compute one deadline for the whole
answer and nothing answers with a token that was already dead.

**No `PartialEq`/`Eq`, because it holds a `Secret`** - a derived `==` on credential material is a
timing oracle, the same reason the domain's `Secret` has no comparison.

## `use StsExchange`

Exchanges one subject's token for a credential to a `BigQuery` source.

**The narrow port that keeps `WorkloadIdentityBroker` testable without a network**, for the same
reason `crate::transport::JobTransport` exists: everything the broker decides is exercised against
a fake, and the HTTP exchange is one implementor behind the `wire` feature. Nothing here takes a
`&Warehouse` or a deadline - it is as narrow as a broker's need.

## `use SystemClock`

The wall clock: the shipping `UnixClock`.

What `WorkloadIdentityBroker::empty` hands a composition root, so wiring a served deployment
takes no clock argument and a test that wants a fixed instant says so through
`WorkloadIdentityBroker::measured_against`.

**Not the only ambient time read on this path, and the distinction is the control's limit.**
`crate::wire::StsOverHttp::exchange` reads its own clock to turn the provider's `expires_in`
into the deadline this floor then judges. So the floor's COMPARISON is deterministic; the path
it judges still has two clock reads in it, milliseconds apart in production and not the same
instant.

## `use UnixClock`

Where this broker reads "now" for its expiry floor.

**A port for the same reason `StsExchange` is one.** Everything this broker DECIDES is
exercised against a fake, and the floor is one of the things it decides - so an ambient
`SystemTime::now()` inside `WorkloadIdentityBroker::mint` would make the outcome of every
broker-level test a function of the day it ran on. The instant is an input instead, and
`A_FIXED_NOW` in this file's suite records what that bought.

**The narrower shape this is NOT.** Every other time-dependent API in this workspace takes the
instant as a parameter - `Expiry::passed_by`, `LegCredentials::still_usable_at`,
`Minted::agreeing_with`, `crate::wire::AccessTokens::bearer` - and that is the better shape. It
is unavailable here because `CredentialBroker::mint` is a DOMAIN port signature carrying no
instant, and widening it reaches ten implementors across eight crates. A held clock is what an
adapter can do alone; the parameter is the follow-up.

## `use WorkloadIdentity`

The setup one impersonating source needs from the settings tree, minus the borrowing.

Carried here rather than as a reference into configuration because an adapter may not depend on
the settings tree. The composition root constructs one of these per source from the parsed
declaration, which has already refused a value that is not usable.

## `use WorkloadIdentityBroker`

A broker that mints a per-subject credential for impersonating sources and a declared witness for
shared ones.

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

### `enum JobDeadline`

```rust
pub enum JobDeadline
```

Which clock one job answers to: the port's own `Deadline`, or the boot path's fresh window.

**A two-variant type rather than `Option<Deadline>`, so the boot arm cannot be spelled by
accident.** `None` reads the same whether it means *forgot to pass the deadline* or *this is
deliberately the boot path* - indistinguishable at a call site and in review. `Boot` is a name a
reader has to notice, and an `execute` or `dry_run` call site that wrote it instead of `Port(..)`
reads as exactly the regression it would be.

#### Variants

- `Port` - A request-time call's own `Deadline`, opened by the transport at the answer's arrival. `Warehouse::dry_run`/`execute` build this arm, and only this arm - see `JobRequest::new`'s own doc.
- `Boot` - The boot path: no caller, no request timeout. `verify_anchor`, a fixture load or drop, and the identity read build this arm; `crate::wire::BigQueryWire::submit` opens a fresh window from this transport's own configured `crate::wire::JobBounds` instead.

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
pub const fn deadline(&self) -> JobDeadline
```

Which clock this call answers to. See `JobDeadline` and the constructor's own doc for what
each arm means to `crate::wire::BigQueryWire::submit`.

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

A readable total can expose a shortfall. An unreadable total beside zero readable IDs exposes
an inventory this adapter could not read, without supplying a count. `ListingTotal::Unreported`
still cannot distinguish an empty dataset from a changed document. Readable IDs dropped by name
filtering remain identified, so an ordinary dataset of unsupported names is not that finding.

**Why it travels on the answer rather than being decided here, now that it IS decided on:** the
decision needs the tables the BUNDLE names, and this port has never seen them - it answers about
a dataset. `BigQueryWarehouse::preflight` is where the two meet, and that is the layer that reads
this field. `JobTransport::listing_was_refused` shows the port CAN hold a decision on the layer
above's behalf, so the layer is a choice rather than a constraint; the reason it is this one is
that a verdict minted here would be one taken without half its input.

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
caller may conclude** - a missing total and an unreadable one cannot carry the same decision.
A reader has to name the case, for the reason
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
here so a reader who greps one finds the other. This type carries the inventory evidence;
preflight decides whether it leaves a requested table unaccounted for and refuses through a value.

#### Variants

- `Unreported` - The document carried no total at all, so an empty listing and an empty dataset are one value.

  **Where every boot stood before the field was decoded**, and where one stands again the day
  the service stops sending it - which is why this is a variant rather than a zero.
- `Unreadable` - It carried a total this crate could not read as a count.

  Distinct from `Self::Unreported` on purpose: *the service said nothing* and *the service
  said something this crate did not understand* are different findings, and the second is
  itself a shape change worth being able to see. Nothing of the value is kept - a foreign
  scalar is not something this crate carries around to print.

  Beside zero readable IDs, preflight returns a count-free unreadable-inventory refusal.
  A readable ID rejected by `usable_table_id` still counts, so a legitimately dropped name
  cannot be mistaken for an inventory from which no ID was readable.
- `Accounted` - It reported a total, and carried a readable table id for every table the total claims.

  *At least* every one: `reported` may be below the number of ids the document carried without
  anything being wrong, because a total read off a dataset being written to is a moving number.
- `Short` - It reported MORE tables than the same document carried readable table ids for.

  **On a document that carried no readable id at all this is the shape change** - a dataset
  that answered with tables the listing did not name, or with entries this crate could read no
  id out of - which is exactly what an empty `tables` array cannot be told from an empty
  dataset without. Where `Shortfall::identified` is non-zero it is weaker: a table created
  between the total and the array, or a page contract this transport read differently than the
  service meant it.

  **What it does not separate, so a decision does not read it as more:** an identified count
  of zero merges *the array was empty* with *no entry carried a readable id*, because the raw
  entry count is not kept. Both are the same finding for the caller that has one - no ids
  beside a non-zero total - so nothing needs the third number today, and a decision that wants
  to tell those two apart has to add it rather than read this one harder.

  What a pre-flight produces from this reading is
  `TablesPresent::Unaccounted` rather than an absence: a table the bundle names that this
  listing did not name may be sitting in the gap, so a boot refuses without saying the catalog
  is wrong. A VALUE and not an `Err`, because `preflight_was_refused` puts everything that is
  not a `401`/`403` in the warning half. `docs/adr/0018` carries the argument and
  `telekom/sutura#275` is where it was taken. `Self::Unreadable` can also refuse, but has no
  shortfall and does so only beside zero readable IDs. The other readings retain ordinary absence.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct Shortfall`

```rust
pub struct Shortfall
```

How far a listing fell short of its own reported total.

**A parsed type and not two `u64` fields on the variant, because the variant's fields were
PUBLIC and the invariant lived in an `if` one module away.** Review reproduced
`telekom/sutura#275` through that door on an unmutated tree: `ListingTotal::Short { reported: 1,
identified: 5 }` is constructible, `HeldTables::of` is a `pub const fn`, and the pre-flight's
subtraction then saturated to a shortfall of zero and fell back to reporting the bundle's tables
ABSENT - the exact defect being fixed, reachable through the public API. A type that forecloses a
zero shortfall is worth nothing while a constructor can route around it, so the door is closed
rather than documented.

Stored as `identified` plus a `NonZeroU64` gap rather than the two totals, so
`Self::unaccounted` is a field read: the *count that decides* cannot be derived wrongly, and
`Self::reported` reconstructs exactly because the sum is the number `Self::parse` was given.

#### Methods

```rust
pub const fn identified(&self) -> u64
```

How many entries of the same document carried a table id this crate could read.

```rust
pub fn parse(reported: u64, identified: u64) -> Result<Self, NotShort>
```

Parses a reported total against the ids of the same document this crate could read.

# Errors

`NotShort::Accounted` where the total is not ABOVE the identified count - which is
`ListingTotal::Accounted`'s case and belongs in that variant, not this one.

```rust
pub const fn reported(&self) -> u64
```

The total the document reported.

```rust
pub const fn unaccounted(&self) -> NonZeroU64
```

How many tables the total claims that no readable id accounted for. Never zero.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `enum NotShort`

```rust
pub enum NotShort
```

Why a pair of counts is not a shortfall.

One variant, an enum for the reason every other error in this crate is one: a second reason has
somewhere to go.

#### Variants

- `Accounted` - The total is not above the ids the same document accounted for, so nothing is missing.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

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

  **The position is carried and the value is not.** A project id is one of the things this
  repository does not print, so a refusal says where the problem is rather than quoting it.

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

### `type_alias DryRunEstimate`

A dry run's own byte estimate, when it priced one - `None` is `docs/adr/0030`'s honest absence,
never a defaulted zero.

**A named alias rather than `Option<EstimatedBytes>` written out at every return type**, because
this exact shape - wrapped in a `Result` - is the return type of `JobTransport::validate` and
every one of its implementors, fake and real; a name spares each of those sites the `Option` and
says what the value MEANS at the read site, which the bare composed type would not. It does not
cross this workspace's `clippy::type-complexity` threshold - it is well under it - so the alias
earns its place on readability alone, not on a lint that does not fire either way.

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

So: one live statement is not a registered data system, and the `data_systems:` axis of the
golden matrix still gains no entry. `sutura` DOES link this adapter and dispatch
`kind: bigquery` behind its default-off `bigquery` feature; the sentence that used to stand here
said it linked none, which `docs/adr/0017`'s second amendment had already spent.

# What this module decides, and every one of them is pinned by a TYPE or by a test

- **A job is bounded in TIME and in MONEY, and neither bound is a constant here.** `JobBounds`
  carries both, `WireAgent` carries the `JobBounds`, and `BigQueryWire` can only be built from
  a `WireAgent`. `jobTimeoutMs` is what cancels a job (`timeoutMs` alone does NOT: it bounds the
  client's own wait, and an expired one leaves the job running and billing); `maximumBytesBilled`
  stops a question scanning a petabyte, which neither the row cap nor the one-page refusal does.
- **The time bound is ONE ABSOLUTE DEADLINE PER ANSWER, opened by the port and not by this
  adapter, and this bullet exists because the earlier two shapes were each the second thing while
  claiming the first.** `timeout_global` on the agent once gave every HTTP operation a full budget
  of its own, so a review measured one ANSWER at four independent budgets against a transport
  whose own request timeout is thirty seconds. `CallDeadline`, opened once per CALL, fixed that
  leak - and then could not fix the next one, because neither `Warehouse` nor `JobTransport`
  took a deadline, so the two calls one answer makes still could not share one; a composition
  root's own configured job bounds substituted an arithmetic that divided the request timeout by
  how many calls one answer makes, checked by nothing outside this crate. **`docs/adr/0029` now
  carries a `Deadline` across the port itself** - one absolute instant,
  opened by the transport at the answer's arrival and shared by every leg. `submit` reads what it
  says is left via `crate::transport::JobRequest::deadline` and opens a `CallDeadline` FROM
  that via `CallDeadline::opened_at_for`, so a slow token exchange shortens the job that
  follows it rather than being followed by one with a full budget of its own, and `timeoutMs`/
  `jobTimeoutMs` are what is left of THAT rather than of this adapter's own configured job bounds.
  A budget spent before the job is `WireError::DeadlineSpent` rather than a send - checked
  BEFORE the credential exchange too, since a spent caller should not spend it on an exchange
  nobody waits for. **The boot path has no port `Deadline` to read** (`verify_anchor`, a fixture
  load or drop, the identity read) and opens a fresh window from this adapter's own configured
  `JobBounds` instead, exactly as every call did before this record. Pinned at `call_body`;
  that `submit` hands it the exchange's own `call` is READ, not measured (`HOST` is unreachable).
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
  because the tunnel is still TLS to `HOST`, so a proxy sees a hostname and no bytes. It is
  written out in `WireAgent::pinned` rather than inherited, so it is a decision a reviewer can
  disagree with.
- **Which roots verify `HOST` is `ureq`'s compiled-in set by default, and a deployment MAY declare
  its own instead - `github.com/telekom/sutura#125`.** `WireAgent::pinned` is unchanged: it
  verifies against `ureq`'s own `RootCerts::WebPki`, exactly as before this change.
  `WireAgent::secured` is the second constructor `security.outbound.transport_anchors` reaches: a
  composition root resolves the declaration through `sutura_tls::load_anchors` once at boot and
  hands the loaded certificates here, which replaces `RootCerts::WebPki` with `RootCerts::Specific`
  built from exactly that bundle or host store - never both. `crate::wire::tls` is the one place
  either constructor turns `sutura_tls`'s `CertificateDer` output into `ureq`'s own certificate
  type, so the conversion is written once rather than at every call site. **No client identity
  travels this way**: `security.outbound` is anchors only - Google's endpoints take a bearer
  token, not mTLS, so there is no `ClientCert` this module ever builds.
- **Failure is derived from the RESULT SHAPE and never from `errors` being non-empty.** The
  endpoint documents that array as *"the first errors or warnings encountered"* and says entries
  *"do not necessarily mean that the job has completed or was unsuccessful"* - so refusing on it
  would decline successful queries that merely warned. What refuses is `jobComplete`, a
  `pageToken`, an absent `totalRows`, and a delivered count that is not the reported total - the
  last of those in the adapter (`BigQueryWarehouse::rows`, `BigQueryError::Incomplete`), not here;
  the reported
  `reason` is folded into whichever of those fires, because it is the best diagnostic
  available at that point. See `complete`, and the limit stated there.
- **Refusal text is closed, not passed through.** `ReasonCode` maps the endpoint's reason to a
  fixed local vocabulary and turns an unrecognized value into a static marker;
  `EndpointMessage` retains the free-text `message` and redacts it under `Debug`. `Display` on
  the refusal renders the status and the local reason code and never the message, so a cause-chain
  walk cannot carry endpoint text either - see `WireError::Refused` for the limit.

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

The compiled-in-roots constructor: `Self::secured` with no declared anchors.

This is every deployment's behaviour before `github.com/telekom/sutura#125` and stays the
default for one with no `security.outbound.transport_anchors` block - see `Self::secured`
for the one setting that differs when a deployment declares one.

```rust
pub const fn rotating(bounds: JobBounds, agent: sutura_tls::Rotating<ureq::Agent>) -> Self
```

The rotation-lane constructor over a handle built by `Self::rotating_agent`.

```rust
pub fn rotating_agent(bounds: JobBounds, anchors: Option<sutura_tls::Anchors>) -> Result<(sutura_tls::Rotating<ureq::Agent>, Option<sutura_tls::Rotator<ureq::Agent>>), sutura_tls::LoadError>
```

Builds the wire's rotating agent handle for a declared `security.outbound.transport_anchors`
set (and, when one is declared, the poll handle the composition root drives on
`sutura_tls::POLL_INTERVAL`). `None` returns a fixed handle over `ureq`'s compiled-in roots
(the pre-`#125` behaviour, nothing to re-read); `Some` rebuilds `RootCerts::Specific` from each
freshly loaded bundle, adopted by the next request.

# Errors

The declared bundle cannot be loaded at boot.

```rust
pub fn secured(bounds: JobBounds, anchors: Option<sutura_tls::LoadedAnchors>) -> Self
```

The one place every non-default setting is decided, and every one of them is a decision:

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
  `HOST`, so what the environment chooses is the route and not the destination. The module
  header states that distinction, because a previous version of it claimed the stronger thing.
- `tls_config`, over `crate::wire::tls::config` - `RootCerts::WebPki` (`ureq`'s own default)
  for `anchors: None`, which is every call `Self::pinned` makes and every deployment before
  `#125`; `RootCerts::Specific` built from `anchors` for `Some`, which is what
  `security.outbound.transport_anchors` resolves to. No client identity: `security.outbound`
  is anchors only, so there is no `ClientCert` in either arm.

The agent is wrapped in a never-rotating `sutura_tls::Rotating` - this constructor has no
declaration to re-read. The rotation lane is `Self::rotating`, fed by
`Self::rotating_agent`.

#### Implements

`Clone`, `Debug`

### `struct EndpointMessage`

```rust
pub struct EndpointMessage
```

The endpoint's own message on a refusal: free text, and the one field here that can name an
account.

**A type rather than a `String`, because the rule it carries is about RENDERING and a rule about
rendering cannot be held at call sites.** `Display` on the whole refusal omits this field;
`Debug` redacts it; only an explicit accessor produces the raw value. That is the whole
mechanism, and it is here because the alternative was asking fourteen acceptance legs to
remember which formatter they used.

**Measured, which is why this exists.** A leg ending `.expect("the endpoint answered")` formats
its error with `Debug`, and `Debug` walks the struct: on a real refusal that printed
`Access Denied: ... permission: <an account>` into a public workflow log. Ten of the fourteen
legs `nix run .#bigquery-acceptance` invokes were in exactly that shape, and the job's
`::add-mask::` step covers the project, the dataset and the table - **not an account**.

**The refusal's own `Display` used to interpolate this field, and that made the type's
redaction narrower than it read.** A cause-chain walk that flattens every link with `Display` -
which is what the transports' sinks do - carried the message into a deployment's own log. That
no longer happens: `WireError::Refused`'s `Display` renders the status and the closed reason
code and never this field. The limit, stated next to the claim: the endpoint's message
remains a queryable string on the error TYPE, reached only by an explicit call - so a caller
that deliberately opts in to rendering it can. The free text is bounded and stripped on the way
in regardless - see `Self::bounded`.

**Why a caller would never reach the raw value by accident, and the cost of that shape:** there
is no `Display` and no `Debug` here that prints the sentence - the only door is `Self::as_str`,
named on purpose - so any formatter that would have leaked it cannot be written without naming
the field and calling that accessor. Keeping the raw sentence out of every ordinary rendering is
the one control this type holds; it does not change what the endpoint itself records on its side.

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

`Clone`, `Debug`, `Eq`, `PartialEq`

### `enum ReasonCode`

```rust
pub enum ReasonCode
```

The fixed reason vocabulary this adapter exposes from an endpoint response.

Provider text is parsed into this type before it reaches an error. The endpoint may add a reason
this adapter does not know; that value becomes `Self::Unrecognized` and its text is discarded.
This keeps ordinary error rendering useful for known conditions without allowing provider-owned
text to become a log line.

#### Variants

- `Absent` - No reason was present in the response.
- `Unrecognized` - The endpoint returned a reason outside this adapter's vocabulary.
- `AccessDenied` - The caller was not authorized.
- `InvalidQuery` - The request was not valid for the service.
- `NotFound` - The requested resource was not found.
- `RateLimitExceeded` - The request exceeded a short-term service rate limit.
- `QuotaExceeded` - The request exceeded a service quota.
- `ResponseTooLarge` - The response exceeded the service's maximum response size.
- `BackendError` - The service reported a temporary backend failure.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The stable text this adapter renders for the code.

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `PartialEq`

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

  **Checked here rather than trusted, and it is the one check that would be pointless if
  anything were cached.** A source that hands back an expired token is a source with a clock
  problem, and presenting it turns that into a `401` from the data system - which reads to an
  operator as a permissions fault.
- `NoClock` - This process could not read a wall clock.

  Reachable only on a machine whose clock is before the epoch. It is a variant rather than a
  fallback because the alternative is presenting a token whose deadline nothing compared.
- `DeadlineSpent` - This call's budget was gone before the job could be submitted.

  **Refused rather than sent with whatever budget was left, because there was none. Reachable
  two ways, and the first is new with `docs/adr/0029`:** the port's own `Deadline` was already
  spent when `submit` was asked to do anything at all - the exchange included, so a
  caller that ran out of time before this adapter was even reached does not spend it on an
  exchange nobody is still waiting for - and the older way, a token exchange slow enough to spend
  what was left of it before the job could be built. Submitting anyway would either mean an
  unbounded wait or a job the service keeps running after the client has stopped waiting, which
  is the pair of failures this whole shape exists to rule out. `budget_seconds` is the port's own
  configured budget where a request carried a `Deadline`, and this adapter's own configured
  `JobBounds` at the boot path, where there is no caller's budget to name.
- `RequestNotSerializable` - The request could not be serialized.

  **A defect-only path, and it is named rather than unwrapped.** Everything in the body is a
  string, a bool or a number, so nothing here can fail the serializer; the variant exists
  because `unwrap` is denied and a silent `unwrap_or_default` would send a different query.
- `Unreachable` - The endpoint was not reached.
- `Unreadable` - The endpoint answered and the answer could not be read.
- `Refused` - The endpoint refused.

  The status, the endpoint's reason mapped to `ReasonCode` in `named`, and its MESSAGE in
  `detail`. An absent or unrecognized reason carries a static local marker rather than provider
  text, which is honest: the status is what is guaranteed.

  **This used to say the message was deliberately not carried, and the field beside it was
  built from `error.message`.** The wrong half mattered: `detail` is free text the endpoint
  writes, it quotes the resource and the principal it refused, and `Display` interpolated it -
  so anything that rendered this variant into a public log leaked both. `ci.yml`'s masking step
  exists because of exactly that, and `tests/exchanged_identity.rs` prints `status` and `named`
  and never `detail` for the same reason.

  **`Display` does NOT render `detail`.** It prints the status and the closed reason code, and
  nothing else: a cause-chain walk that flattens every link with `Display` - which is what the
  transports' sinks do - carries the same pair and never endpoint-owned text.
  `detail` is an `EndpointMessage`, whose `Debug` is redacted and whose raw value is reached
  only through an explicit accessor a caller has to opt into. Each of the three renderings
  this error can meet is therefore one of those, and the one that leaks is the one a caller
  cannot write by accident. `docs/adr/0018` carries this decision and its limit.
- `NotADocument` - The answer was not the document a query response is.
- `NotComplete` - The job had not finished when the endpoint answered.

  **Refused rather than polled.** The alternative is `getQueryResults`, which needs a `location`
  this deployment does not declare - see the module header - and a partial answer is a wrong
  number under a certified name.

  **It should now be reachable only through a defect or a cancellation**, because the request
  carries `jobTimeoutMs` equal to the client's own wait: the service cancels the job at the same
  instant the client stops waiting for it, so an incomplete answer is no longer a live job this
  adapter walked away from. `named` carries the endpoint's reason as a closed `ReasonCode`,
  which for a cancelled job is the useful half.

  **This is the DOCUMENTED shape `crate::transport::JobTransport::deadline_exceeded` answers
  `true` for, and it is stated as documented rather than measured because that is exactly what
  it is.** `timeoutMs` and `jobTimeoutMs` travel as the same number by construction - see
  `crate::wire::document::body` - so a synchronous `jobs.query` reply cannot say *the wait
  expired* without also saying *the service was asked to cancel at the same instant*: the
  endpoint's own documentation of `timeoutMs` is that an expired one answers `jobComplete:
  false`, which is this variant.

  **Not yet measured against a real endpoint, and that is stated here rather than implied.** An
  acceptance cell that raced a statement against a real deadline to reach exactly this reply was
  tried and reverted - the corpus fixture is a handful of rows, so the round trip reliably
  finishes before any budget short enough to matter, and a budget picked to "usually" lose that
  race flakes against a project that bills for it. `tests/tests/deadline.rs`'s cell proves the
  narrower claim instead - a spent port deadline refuses through the REAL wire and credential
  before a request is sent - and leaves this variant's own shape open, closed only by a
  statement that reliably outruns a real budget without depending on fixture size or jitter.
- `MoreThanOnePage` - The answer is one page of more than one.
- `NoTotal` - A complete job that stated no total.

  **Refused rather than read as zero**, because zero is what a complete empty result and a
  missing field both look like, and only one of them is an answer this adapter may certify.

  This is also where a FAILED job lands: the endpoint reports one as complete with no total, so
  `named` carries the endpoint's reason as a closed `ReasonCode` and is the whole diagnostic.
  That is why failure is derived from the shape here rather than from `errors` being non-empty -
  see `reported`.
- `NotATotal` - The total was not a number.

  It arrives as text, because the endpoint writes 64-bit integers as JSON strings.
- `NotAnEstimate` - `totalBytesProcessed` was present and not a number.

  Same reason as `Self::NotATotal`: the endpoint writes this 64-bit count as a JSON string
  too. Refused rather than read as `None` - which is reserved for the field being ABSENT -
  because a value that arrived and did not parse is a shape this adapter does not understand,
  not a dry run that declined to price.
- `NoSchema` - A complete job with rows and no schema to read them against.
- `NotAScalar` - A cell that is neither a string nor a null.

  Every scalar the endpoint returns is JSON text whatever its declared type; an array or an
  object is a `REPEATED` or `RECORD` column, which is outside `crate::transport::FieldType`'s closed
  vocabulary. It names the position rather than the value, because the value is a row.
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

### `use BytesBilledCeiling`

The most a single job may be billed for scanning.

**Sent as `maximumBytesBilled`, which is enforced at the service and is what makes it worth
more than a client-side check.** A job that would exceed it FAILS and is not charged. Nothing else
in this repository bounds bytes scanned: `LIMIT 10001` bounds rows RETURNED, the one-page refusal
bounds a page, and `MAX_ANSWER_BYTES` bounds what is read into memory - a question can satisfy
all three and still scan a partitioned table end to end.

### `use CallDeadline`

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

**The limit this type used to carry is resolved by `docs/adr/0029`, and the record of it stays
here rather than being deleted, because the fix is a fact about the type above it and not about
this one.** One ANSWER calls the port twice - `Warehouse::dry_run` and then `Warehouse::execute` -
and this type alone could never make the two share a budget: it is opened fresh by whoever calls
`Self::opened`/`Self::opened_at`, and nothing HERE remembers what an earlier call spent. The
port now carries a `sutura_domain::warehouse::deadline::Deadline` - one absolute instant per
answer - and `crate::wire::BigQueryWire::submit` opens a `CallDeadline` from what THAT says is
left via `Self::opened_at_for`, so the sharing lives one level up, where the two port calls
actually are. The boot path (`verify_anchor`, a fixture load or drop) has no such `Deadline` to
read and keeps opening fresh from this adapter's own configured `QueryDeadline`, exactly as
every call did before this record.

### `use JobBounds`

What every job this adapter submits is bounded by.

Two bounds in one value, because they are one decision: *how much of a deployment's time and money
may one question spend*. A struct rather than two arguments so a call site cannot supply one and
forget the other, and so `super::WireAgent` can carry them both.

### `use QueryDeadline`

How long a job may run when there is no port `Deadline` to read one from, and the ceiling this
adapter's socket is pinned to for every call.

**A newtype rather than a constant, because the value belongs to the deployment.** The setting
that decides it is the one the transport in front of this service already uses -
`server.request_timeout_seconds`, which ships as 30.

**It used to be a SHARE of that setting rather than the setting itself, and `docs/adr/0029` is
why it no longer is.** One answer made two calls through this transport - `Warehouse::dry_run`
then `execute` - and neither took a deadline, so this type had to divide `30` by the two of them
and their own connection overhead to keep an answer inside the caller's own wait -
`within_request_timeout` was that arithmetic, checked by nothing outside this file. The port now
carries ONE `Deadline` shared by every call one answer makes - `CallDeadline` opens FROM it at
request time - so this type is left with a narrower job: the boot path, which has no `Deadline`
to read (`verify_anchor`, a fixture load or drop, the identity read), and the socket ceiling every
call is pinned to as a backstop regardless of what a request supplies. `Self::parse` is a
composition root's one door in, and it takes the setting directly rather than a share of it.

### `use UnusableBound`

Why a bound this adapter was handed is not usable.

### `use IamCredentialsOverHttp`

An `ImpersonateAsAccount` that talks to Google's `iamcredentials` API over HTTP.

### `use StsOverHttp`

An `StsExchange` that talks to Google STS over HTTP.

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

How long a job may run when there is no port `Deadline` to read one from, and the ceiling this
adapter's socket is pinned to for every call.

**A newtype rather than a constant, because the value belongs to the deployment.** The setting
that decides it is the one the transport in front of this service already uses -
`server.request_timeout_seconds`, which ships as 30.

**It used to be a SHARE of that setting rather than the setting itself, and `docs/adr/0029` is
why it no longer is.** One answer made two calls through this transport - `Warehouse::dry_run`
then `execute` - and neither took a deadline, so this type had to divide `30` by the two of them
and their own connection overhead to keep an answer inside the caller's own wait -
`within_request_timeout` was that arithmetic, checked by nothing outside this file. The port now
carries ONE `Deadline` shared by every call one answer makes - `CallDeadline` opens FROM it at
request time - so this type is left with a narrower job: the boot path, which has no `Deadline`
to read (`verify_anchor`, a fixture load or drop, the identity read), and the socket ceiling every
call is pinned to as a backstop regardless of what a request supplies. `Self::parse` is a
composition root's one door in, and it takes the setting directly rather than a share of it.

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

**The one door in, since `docs/adr/0029` retired `within_request_timeout`'s arithmetic**
(deleted, along with the `CALLS_PER_ANSWER` constant it depended on and the `NoBudget`
refusal it alone produced): a composition root used to have to divide
`server.request_timeout_seconds` by how many calls one answer makes before filling this in,
because neither call carried a budget the other could see. The port now carries one
`sutura_domain::warehouse::deadline::Deadline` shared by every call one answer makes, so what
this type bounds is narrower and needs no division: the boot path, which has no such
`Deadline` to read, and the socket ceiling every call is pinned to regardless. A composition
root fills this from `server.request_timeout_seconds` directly.

```rust
pub const fn socket(self) -> Duration
```

How long a socket may stay open for a call that has spent none of its budget yet.

**The backstop on the agent rather than the bound that holds.** What a single operation is
really allowed is `CallDeadline::socket(left)` over what is LEFT of the call's budget - see
`CallDeadline`, and see the module header for why a per-operation timeout was not enough. This
value is what the agent is configured with, so an operation that somehow reached the client
without an override is still bounded.

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

**The limit this type used to carry is resolved by `docs/adr/0029`, and the record of it stays
here rather than being deleted, because the fix is a fact about the type above it and not about
this one.** One ANSWER calls the port twice - `Warehouse::dry_run` and then `Warehouse::execute` -
and this type alone could never make the two share a budget: it is opened fresh by whoever calls
`Self::opened`/`Self::opened_at`, and nothing HERE remembers what an earlier call spent. The
port now carries a `sutura_domain::warehouse::deadline::Deadline` - one absolute instant per
answer - and `crate::wire::BigQueryWire::submit` opens a `CallDeadline` from what THAT says is
left via `Self::opened_at_for`, so the sharing lives one level up, where the two port calls
actually are. The boot path (`verify_anchor`, a fixture load or drop) has no such `Deadline` to
read and keeps opening fresh from this adapter's own configured `QueryDeadline`, exactly as
every call did before this record.

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

  Bounded before it is parsed, which is the ordering *secure by design* asks for: the cheap
  check that stops work proportional to the input runs first. A credential file is a few
  kilobytes at most.
- `NotADocument` - The file is not the JSON document this expects.
- `UnknownKind` - The file names a credential shape this build does not implement.

  **It names the shape rather than saying "unsupported"**, because each named shape has a
  different answer: the metadata server needs no file at all, and a federated credential is the
  per-subject step. The module header lists both.
- `Incomplete` - A document missing one of the fields its own kind needs.
- `AnotherUniverse` - The credential was minted against a different service universe than the one this build talks to.

  **Refused rather than tried.** The endpoints this crate reaches are compile-time constants in
  the default universe, so a credential minted for another one would be presented to a service it
  was not issued for - which is a credential sent to the wrong recipient, whatever the answer
  turns out to be.
- `UnreadableKey` - The private key is not a `PKCS#8` PEM block holding a key this build can sign with.

  **Nothing from the key reaches the message.** The whole value is key material, so there is no
  half of it that would be safe to quote - which is why the context is a typed `KeyUnusable`
  naming the STAGE that refused rather than any part of the value.

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

  **The status and the provider's own short code, and not its description.** The two `OAuth`
  fields are a fixed vocabulary - `invalid_grant`, `invalid_client` - which is what an operator
  needs; the description is free text from another service, and this repository does not put
  unbounded foreign text where a log will read it.
- `NotADocument` - The answer was not the JSON document a token response is.
- `NoToken` - The answer carried no token.
- `AlreadyExpired` - The answer's own deadline had already passed when it arrived.

  A token that is expired on delivery is a clock disagreeing with a clock, and presenting it
  anyway would turn one clear failure into a `401` from the data system.
- `DeadlineSpent` - The call's budget was gone before the exchange could be attempted.

  **Reachable only where something before the exchange spent the whole call**, which today means a
  clock read and a signature. It is a variant rather than a send with no timeout because a zero
  budget handed to the client underneath means *no timeout at all* - see
  `CallDeadline::remaining`.
- `Unsigned` - The assertion could not be signed.

  Only reachable for a `service_account`. The cause is kept: `ring`'s own error says whether the
  key was rejected, and that is the difference between a bad key and a bad build.
- `NotSigned` - The signature itself failed.

  `ring` reports this opaquely on purpose, so there is nothing to carry beyond the fact.
- `NotAServiceAccount` - `Credential::mint_id_token` was asked of an `authorized_user` credential.

  Trading a refresh token for an ID token is a different grant this crate does not build - only
  `mint_id_token`'s CI-only caller reaches this, never `AccessTokens::bearer`.

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
pub fn mint_id_token(&self, target_audience: &str, now_unix_seconds: u64, within: CallDeadline) -> Result<Secret, TokenUnavailable>
```

Trades this service account's own key for a Google-issued OIDC ID token, for
`examples/mint_subject_assertion.rs` to mint a subject assertion at job time -
telekom/sutura#376. The one line `id_token` adds to this type's public surface; everything
else lives in that submodule, which `clippy::multiple_inherent_impl` (denied
workspace-wide) is why this stays one line here rather than a second `impl Credential`.

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

### Module `document`

The two documents this adapter exchanges with the endpoint, and the code that builds and reads
them.

**Split out of `wire.rs` when that file crossed 1000 lines**, and the cut is at a real seam rather
than at a line count: everything here is about the SHAPE of a request and an answer, and nothing
here opens a socket, holds a credential or reads a clock. That is also what makes it the half a
test can exercise - `super::tests` asserts on the SERIALIZED body and over answer documents, which
is the whole of what this repository can prove without a project.

Every decision these types carry is argued in `wire.rs`'s own header, which is the one place to
read them together; what is written here is why each field is the way it is.

#### `fn deserializes`

```rust
pub fn deserializes(text: &str) -> Result<(), serde_json::Error>
```

Whether the endpoint's raw reply deserializes into `QueryAnswer`, with the value discarded.

**`pub`, and the only reason.** This reply is a data SYSTEM's response - foreign bytes on their
way to an agent, not a file an operator placed or a caller's own payload - and the
`bigquery_reply` fuzz target drives it from outside this crate. Widening `QueryAnswer` itself
to reach that target would publish a type this crate's own newtype rule keeps field-private
(`cargo xtask check-boundaries`'s `api_shape` half refuses a `pub` field on a `pub` struct); this
wrapper reaches the identical `parse` call without needing to.
