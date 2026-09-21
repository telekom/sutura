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

The **transport** - one `transport::JobTransport` that executes the statement - is `adbc`,
behind the default-off `adbc` feature: it loads the self-built `libadbc_driver_bigquery.so`
(`nix/bigquery-adbc.nix`), runs the query through the driver, and decodes the Arrow result. The
driver owns the HTTP transport and its own authentication, so this crate ships no TLS stack and
reads no credential file - the previous HTTP `wire` transport and its STS/credential machinery
were removed when ADBC became this adapter's only mode.

So nothing here may be cited as a round-tripped invariant. `sutura serve` links this adapter and
dispatches `kind: bigquery` behind its default-off `bigquery` feature, but the ADBC driver path is
not yet a shipped artefact and no live acceptance leg against a real dataset is wired under it -
the `wire`-era acceptance/corpus/differential legs went away with the transport. A default build
links none of this.

# Identity

`BigQueryWarehouse::IMPERSONATION` is `PerSubjectCredential`, and it is now true by
construction rather than by this paragraph: the asking subject's **own verified assertion** is
what the data system authenticates. `adbc` hands the driver a workload-identity credential
document naming a loopback source for that assertion, so Google's token service verifies it
against the pool the source declares and the question executes as whatever principal that pool
resolves the subject to. `adbc`'s own `subject` module carries the mechanism and its exposure.

**Two modes, and they are XOR rather than a ladder.** A source declares one posture and gets one
mechanism:

| declared posture | mechanism | identity at the source |
| --- | --- | --- |
| `shared-service-user` | the deployment's own application default credentials, **mandatory** for this posture, impersonating nothing | the deployment |
| `impersonation-at-source` | the subject's own assertion, federated against the declared pool | the asking subject |

**What does not exist is a path from the second to the first**, and that is the whole point: an
impersonating source whose subject cannot be federated is REFUSED
(`adbc::identity::authenticate`'s third arm), never answered as the deployment. The reverse is
refused one layer up, by `Presented::agrees_with`: a shared source handed a subject's
credential is a posture disagreement before any transport sees it.

**The mechanism this replaced is deleted rather than kept beside it.** For two rounds this
adapter set the driver's `bigquery.impersonate.target_principal` from the DEPLOYMENT's own
credentials - so the connection was the deployment's, the subject's credential was nowhere in
the chain, and leg 1 was the only barrier. `docs/adr/0018`'s fifth amendment records it, and
`JobIdentity` has no spelling for it: a principal switch is unrepresentable here, not refused at
runtime.

**So a `Presented::SubjectPrincipal` is refused.** It is the same POSTURE as an assertion to
the domain, so `Presented::agrees_with` passes it and only this adapter can say it has no
mechanism - `BigQueryError::NoPrincipalSwitch`. `GoogleSQL` has no proxy-user or `SET ROLE`
equivalent either, so there is nothing to build it out of.

**And because leg 1 still gates who may be federated at all, two of ITS limits bound this.**
`sutura-http`'s inbound gate bounds a gateway assertion's replay *window* and binds nothing to a
request (`within_the_lifetime_ceiling`'s own doc), so inside that window a captured assertion is
replayable - and Google would accept it, because it is the caller's real document. And the
signing-key age bound `KeySetCache::stale_for` measures excludes a failing refresh. Neither is
this crate's to fix; both are cited here because *the limit belongs beside the claim*.

**What is still unproven, and no green run here changes it:** nothing reachable from this
repository shows Google ACCEPTING the assertion. The document is built and the loopback source
is asserted against a real socket; the exchange needs a pool, a project and a hosted run.
`docs/where-identity-is-proven.md` records that venue as `wired` and undispatched, so **leg 2 is
not proven.**

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
- `UnresolvableConnection` - A table path could not be resolved against this connection, or the resolved tables - taken together - answer to one identifier the statement cannot tell apart.

  **Unreachable in practice**: see `UnresolvableConnection`'s own documentation for why. A
  typed variant rather than a panic for the same reason every other "unreachable" case in
  this workspace is one - the input reaching it is not bounded by the type system alone.
- `LegWithoutCombiner` - A federated leg arrived, and there is nothing above it to combine legs.

  **A refusal to execute rather than an execution**, worded as `sutura-exec-duckdb` words it: a
  leg run with nothing above it returns rows at a finer grouping than the question asked for,
  which is a wrong number under a certified name.
- `NoPrincipalSwitch` - The leg presents a principal for the data system to switch to, and no transport here has a spelling for one.

  **Reinstated, and the reason is the mechanism reversal rather than a revert.** For two rounds
  this adapter delivered exactly that shape, through the driver's `target_principal` option and
  the deployment's own application default credentials - so the connection was the
  deployment's and the subject's credential was nowhere in the chain. The owner rejected it,
  so `crate::transport::JobIdentity` carries one subject arm now and this refusal is what a
  broker presenting the other one gets. Accepting it would submit the job under the
  credential the transport already holds while provenance, read off this source's posture,
  reported the answer as impersonated.
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

**The same comparison `sutura_conformance::execute`'s pack now also makes over this
adapter** (`telekom/sutura#710`, `crates/sutura-exec-bigquery/tests/conformance.rs`) - kept
here as well so it can be checked against this adapter's own dry-run path without going
through a fixture at all. A live endpoint's own guarantee that it always prices one is
still unverified by either cell; this only compares what an already-answered pre-flight
carried against the declaration.

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
pub fn over_adbc(source: SourceName, posture: SourcePosture, billing_project: ProjectId, default_dataset: DatasetId, driver_path: impl Into<String>, impersonation: adbc::Impersonation) -> Self
```

Opens a dataset over the ADBC transport.

`driver_path` is the on-disk location of the self-built
`libadbc_driver_bigquery.so` (one per release triple, see
`nix/bigquery-adbc.nix`).

`impersonation` is whether this source impersonates and at what scope - the source's declared
`workload_identity.scope`, or `adbc::Impersonation::Disabled` for a shared one. Taken here
rather than read per request because it is a property of the source, and a declared scope the
driver would refuse then fails before a listener is bound.

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

## `use UnresolvableConnection`

Why this connection's own billing project could not be read as the domain's project vocabulary.

**Unreachable in practice, and a typed branch rather than an `.expect()` because a panic here
would be reachable from a live query, not just a catalog file.**
`sutura_config::sources::placement::BillingProject::parse` accepts 6 to 30 characters of
`[a-z0-9-]`, starting with a letter and never ending in a hyphen - a strict subset of what
`ProjectName::parse` accepts, so a billing project that reached this adapter at all already
satisfies it.

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

## `use DeclaredPrincipalBroker`

Presents the principal a source declared for the asking subject, and the operator's witness for a
shared one.

**Both maps, because one plan may read one of each and a broker is per answer rather than per
source** - the same reason `crate::WorkloadIdentityBroker` holds two.

## `use DeclaredPrincipals`

The subjects one source may be asked as, and the account each of them resolves to.

**A parsed type and not a bare map, because the empty map is the interesting value.** An
impersonating source with no declared subject can serve nobody: every request would be refused,
while the boot log said the source opened. That is the exact defect this whole change exists to
remove, so the emptiness is refused at the boundary that can turn it into a startup failure
rather than documented at the one that cannot.

## `use DeclaredPrincipalsUnusable`

A defect in this broker itself, which no configuration reaches.

Stated rather than unwrapped for `sutura_config::StaticCredentialsUnusable`'s reason: the one
thing minting can fail on is a credential set that does not cover the sources it was asked
about, and this broker builds its map from that same set. `unwrap_used` is denied and a panic
here would be process death under `panic = "abort"` for a case a type already describes.

## `use NoDeclaredPrincipals`

Why a declared impersonation map is not one a source can be served under.

One variant, and an enum for the reason every other error in this crate is one: a second reason
has somewhere to go.

## `use ImpersonateAsAccount`

The second hop, telekom/sutura#376's iamcredentials step: a federated access token in, a
service-account access token out.

**A second port and not a second `StsExchange` method** - the two calls have different request
and response shapes (RFC 8693 token exchange vs `{scope, lifetime}`) and different failure modes
(STS `invalid_target` vs `iamcredentials`'s own `403` for "may not impersonate"). Everything this
broker decides about WHEN to call it is exercised against a fake; the real HTTP call arrives at
this port as the `wire` transport's `IamCredentialsOverHttp`, behind the same removal
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
`Minted::agreeing_with`, the `wire` transport's `AccessTokens::bearer` - and that is the better shape. It
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

## Module `adbc`

The ADBC transport: loads the self-built `libadbc_driver_bigquery.so`
(`nix/bigquery-adbc.nix`) through `adbc_core` + `adbc_driver_manager` and
decodes its Arrow result sets.

```text
adbc_core + adbc_driver_manager → C ABI → libadbc_driver_bigquery.so
  → BigQuery → Arrow RecordBatchReader → decode::Decoding → RowSet
```

Behind the crate's default-off `adbc` feature, like the `wire`: the native
driver and its Arrow graph are a per-triple addition a lean build should not
link. Off is not hidden - every gate passes `--all-features`.

# Who a job runs as

Two declared postures, XOR, decided at composition and never per request: a `shared-service-user`
source runs on the deployment's own application default credentials and impersonates nothing, and
an `impersonation-at-source` source federates the asking subject's own assertion against the pool
it declares. `identity` is where that decision lives and is asserted, and `subject` is where the
federation's mechanism and its exposure are written down.

**A subject at a source with no pool is REFUSED** rather than answered as the deployment - the
one thing this transport must never do, and the reason there is no third state. The mechanism
this replaced - an impersonation target minted from the deployment's own credentials - has no
spelling in `crate::transport::JobIdentity` any more, so it is unrepresentable rather than
refused.

# The values

Bound as one Arrow batch of one row, per `bind` - which is where the driver's own per-row
execution loop is read off the pinned source and why one row is the only correct count. The
statement carries positional `?` and nothing is ever interpolated into it.

**Nothing is shared between two jobs.** The driver handle, the database and the connection are
locals of `AdbcBigQuery::connect`, built from one request's own options; the endpoint itself
owns only a path and a declaration. That, and not a check, is what keeps two concurrent subjects apart
here - stated with its limit in `AdbcBigQuery`'s own documentation.

### `enum AdbcError`

```rust
pub enum AdbcError
```

Why the ADBC transport could not answer.

#### Variants

- `Load` - The driver `.so` could not be loaded.
- `Adbc` - An ADBC call (connect, prepare, execute) failed.
- `Batch` - A result batch could not be read from the stream.
- `Decode` - The result set could not be decoded into the adapter's own shape.
- `Uncovered` - ADBC does not yet cover a port method this transport was asked for.
- `NoDryRun` - There is no ADBC call that prices a statement without running it.

  **Its own variant rather than an `Self::Uncovered`, because one caller has to be able to
  tell it apart and a `&'static str` is not something to match on.**
  `JobTransport::declined_to_dry_run` reads this variant and nothing else, which is what
  lets the adapter answer `PreFlight::NotAsked` for a dry run nobody made while a dry run that
  was made and failed stays a failure.
- `Parameters` - The plan's values could not be assembled as the batch this driver binds them from.

  Its own variant rather than an `Self::Adbc`, because the failure is on THIS side of the C
  ABI: nothing has been sent, and what went wrong is an Arrow batch this transport built. The
  cause is Arrow's own, kept as a `#[source]` so the chain still walks.
- `SubjectSource` - The loopback source a subject's assertion is served over could not be opened.

  Its own variant rather than an `Self::Adbc`, because nothing has been sent and the failure
  is this process's own: a host with no usable loopback interface cannot serve an impersonated
  question, and saying that is better than a driver failing to fetch a token for reasons of
  its own.
- `NoRandomness` - The operating system would not supply the randomness this request's two secrets need.

  **A refusal and not a fallback**, and `subject::unguessable`'s own doc carries why: every
  constant available here would be written into the same document the driver reads, so the
  fetch would authenticate against a value any local process could guess.

#### Implements

`Debug`, `Display`, `Error`

### `struct AdbcBigQuery`

```rust
pub struct AdbcBigQuery
```

A `BigQuery` endpoint over ADBC.

**Two owned values and nothing else, which is load-bearing rather than tidy.** There is no
connection here, no database handle and no token: `Self::connect` builds all three per job from
that job's own request and drops them with it. That is what keeps one subject's principal off
another subject's query - not a check, but the absence of anything two jobs could share.

**The limit beside it:** nothing in the type system forbids a future field from holding a
connection, and a pool keyed on anything but the identity would be exactly the cross-user leak
this shape avoids. `identity::tests::one_subjects_principal_never_appears_in_the_next_subjects_options`
is the cell that dies if the option list starts being memoised; a *connection* cache would need
its own.

#### Methods

```rust
pub fn new(driver_path: impl Into<String>, impersonation: Impersonation) -> Self
```

Names the driver `.so` a composition root resolves to load, and whether this source
impersonates.

```rust
pub fn probe(driver_path: &str) -> Result<(), AdbcError>
```

Does the driver at this path load and initialise at all?

**The one thing a boot path or a diagnostic can find out about the `.so` without a project**,
and it is worth more than reading the environment variable: `dlopen` of this driver runs the
GO RUNTIME's own initialisation inside this process, beside tokio and beside the allocator a
release build links. That is the coexistence nobody could assert while the only caller was a
question - so a link-success check would have passed and been wrong, and this executes instead.

It opens a DATABASE and stops there, deliberately. `new_database_with_opts` is option-setting
on the Go side and reaches no network; `new_connection` is where the driver builds its client
and looks for application default credentials, which on a host with none is a metadata-server
probe this has no business making. So what a success means is exactly *the `.so` is this ABI
and its runtime started*, and nothing about whether a question could be answered.

# Errors

`AdbcError::Load` where the `.so` is absent, is not this ABI, or cannot be loaded at all -
which is what a static-musl binary answers, because it has no dynamic loader.
`AdbcError::Adbc` where the driver loaded and refused the database.

#### Implements

`JobTransport`

### `use Impersonation`

Whether this source impersonates at all, and against which pool when it does.

**A two-variant type rather than an `Option<WorkloadPool>`, because the absence is a
DECLARATION.** A source is opened shared or impersonating - `sutura_config` refuses a
`workload_identity` block on a shared entry and refuses its absence on an impersonating one - so
which of these a transport holds is decided once, at composition, from a value an operator wrote.

### `use UnusablePool`

Why a declared pool is not one this transport can exchange against.

### `use WorkloadPool`

Everything about a source that does not change per request: the pool it exchanges against.

**Parsed at composition, so an unusable declaration fails to start.** The audience is the pool
provider resource the subject token is exchanged for - `externalaccount::Options::validate`
refuses an empty one outright, so a deployment that declared nothing would fail on its first
question instead of at boot.

**One field, and the declared SCOPE is not it.** `sources.<alias>.workload_identity.scope` is
parsed by `sutura_config` and reaches nothing here, because the pinned driver has nowhere to put
it: `credsfile::ExternalAccountFile` (`cloud.google.com/go/auth@v0.23.2`) has no `scopes` member,
so the document cannot carry one, and the driver's only scope option is
`bigquery.impersonate.scopes`, which `connection.go`'s `hasImpersonationOptions` treats as a
request for the DELETED mechanism - it then demands a target principal and replaces the
federated credential with an impersonated token source. A screened value this transport cannot
send would read as a control that is in place, so it is not held here at all and the operator is
told where they declare it.

### `use MOST_RESULT_ROWS`

How many rows this transport will materialise from one result stream before refusing.

**A ceiling on THIS PROCESS's memory, not a cap on an answer**, and that distinction decides the
value. `sutura_domain::plan::MAX_ROWS` caps an answer and travels in the statement's own
`LIMIT`; a federation LEG carries no `LIMIT` at all - `sutura_domain::plan::leg`'s header says
so, because a leg is not an answer - so for a leg there is nothing in the statement bounding
what the source may stream back, and the only thing between a driver that streams without end
and this process is a number here. The refusal fires WHILE reading, in
`Decoding::push`, so it cannot be reached by first materialising the whole stream.

Two orders of magnitude above `MAX_ROWS`, because it has to refuse only a stream no plan could
have asked for: a leg legitimately returns more rows than the one answer re-aggregated above it
keeps.

### `use Reported`

The row count a stream reports, when it reports one at all.

`Reported::Unreported` is an honest absence, never a defaulted `0`.

### Module `decode`

Arrow -> `JobRows` decode for the ADBC transport (telekom/sutura#913).

The driver returns Arrow record batches; this pure half turns a schema +
batches into the adapter's own `JobRows`, reusing the same
`crate::transport::FieldType` vocabulary the wire uses, so one plan answered
by two transports agrees on field kinds.

# A batch is checked against the schema it was announced under, by NAME

**A width check is not a schema check, and the difference is a wrong answer rather than a
failure.** Arrow itself validates a `RecordBatch` positionally and never by name -
`arrow_array::RecordBatch::try_new` zips columns against fields and compares type and
nullability - so a driver that hands back two same-typed columns in the wrong order builds a
perfectly valid batch, and a decoder that only counted columns would label those values with the
outer schema's names. That is a transposed answer under a certified metric name, with no error
anywhere. The same defect class was measured in a third-party positional cast: a swapped integer
key and decimal measure came back transposed, and a same-typed swap came back silently empty.

So `Decoding::push` refuses a batch whose field at a position is not the field the schema
announced there - name AND type - as `Decode::Mislabelled`, before a single value is read.
Nothing in `DataFusion` would have done it for us: `SchemaAdapter`/`SchemaMapper` are deprecated
and their default implementation returns `not_impl_err!`, the live `PhysicalExprAdapter`
resolves by name only on the datasource path, and nothing validates that a custom plan's stream
matches its declared schema at all. For a foreign driver behind this transport the obligation is
ours.

# The stream is decoded as it arrives, under a ceiling

One pass, not two: `Decoding` takes one batch at a time and appends its rows, so nothing ever
holds a `Vec<RecordBatch>` beside the rows decoded from it. The cast to text is one vectorised
`arrow_cast::cast` per COLUMN per batch - it used to run inside the row loop, which cast every
column once per row.

Completeness is decided here. The wire refused a first page by comparing the
delivered count to the endpoint's `totalRows`; an ADBC read streams the whole
result, so completeness is the stream draining fully. `Decoding::finish` takes a
`Reported` total (the driver attaches job statistics to the schema metadata,
measured in the provisioned leg) and refuses a delivered count that does not reach
what was reported.

#### `enum Decode`

```rust
pub enum Decode
```

Why a result set could not be decoded.

##### Variants

- `UnmappedColumn` - A column whose Arrow type this adapter does not map.
- `Shape` - A batch that disagrees with the schema it was announced under - a stream arriving over a C ABI from a foreign driver is exactly the case to refuse rather than trust.
- `Mislabelled` - A batch of the right WIDTH whose field at one position is not the field the schema announced there, so its values would have been labelled with somebody else's name.

  Carries both descriptors - `name type`, which is a driver's own metadata and not a value -
  so an operator can see which way round the two are without the refusal quoting a cell.
- `Incomplete` - The stream was not complete: a reported total the delivered rows do not reach.
- `OverBound` - The stream carried more rows than this transport will materialise - see `MOST_RESULT_ROWS`.

##### Implements

`Debug`, `Display`, `Eq`, `Error`, `PartialEq`

#### `enum Reported`

```rust
pub enum Reported
```

The row count a stream reports, when it reports one at all.

`Reported::Unreported` is an honest absence, never a defaulted `0`.

##### Variants

- `Unreported`
- `Total`

##### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

#### `struct Decoding`

```rust
pub struct Decoding<'announced>
```

One result stream, decoded batch by batch into the adapter's own rows.

**The accumulator exists so the bound and the schema check can fire WHILE the stream is read.**
The transport's `run` drives it directly off the driver's `RecordBatchReader`, so a stream that
will be refused is refused at the batch that crosses the line rather than after every batch has
been collected and then decoded a second time. It replaced a `job_rows(&schema, &batches, ..)`
whose only caller collected the whole stream first; nothing needs that shape now, so it is gone
rather than kept beside this one.

##### Methods

```rust
pub fn finish(self, reported: Reported) -> Result<JobRows, Decode>
```

The decoded rows, refusing a stream that did not reach a reported total.

# Errors

`Decode::Incomplete` where `reported` names a total the delivered rows do not reach.

```rust
pub fn of(announced: &'announced Schema, most: usize) -> Result<Self, Decode>
```

Reads the announced schema's types, refusing one this adapter does not map.

**The type pass is here rather than per batch on purpose:** a result with no rows at all
still refuses an unmapped column, which is the answer a caller needs before it reads a field
list. `most` is the row ceiling, passed rather than defaulted so the call site says which
bound it is under - `MOST_RESULT_ROWS` is what the transport passes.

```rust
pub fn push(&mut self, batch: &RecordBatch) -> Result<(), Decode>
```

Checks one batch against the announced schema and appends its rows.

# Errors

`Decode::Shape` for a batch of the wrong width, `Decode::Mislabelled` for one whose
field at a position is not the announced one, `Decode::OverBound` where this batch would
take the stream past the ceiling, and `Decode::UnmappedColumn` where a column would not
cast to text.

#### `constant MOST_RESULT_ROWS`

How many rows this transport will materialise from one result stream before refusing.

**A ceiling on THIS PROCESS's memory, not a cap on an answer**, and that distinction decides the
value. `sutura_domain::plan::MAX_ROWS` caps an answer and travels in the statement's own
`LIMIT`; a federation LEG carries no `LIMIT` at all - `sutura_domain::plan::leg`'s header says
so, because a leg is not an answer - so for a leg there is nothing in the statement bounding
what the source may stream back, and the only thing between a driver that streams without end
and this process is a number here. The refusal fires WHILE reading, in
`Decoding::push`, so it cannot be reached by first materialising the whole stream.

Two orders of magnitude above `MAX_ROWS`, because it has to refuse only a stream no plan could
have asked for: a leg legitimately returns more rows than the one answer re-aggregated above it
keeps.

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
  licences exactly, and it arrives in exactly one place: `crate::adbc`, behind the crate's
  default-off `adbc` feature. The HTTP `wire` it replaced was removed with this adoption. The sentence that kept this seam empty for
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
- `Boot` - The boot path: no caller, no request timeout. `verify_anchor`, a fixture load or drop, and the identity read build this arm; the ADBC driver opens a fresh window under its own configured bounds instead.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `enum JobIdentity`

```rust
pub enum JobIdentity<'job>
```

Which identity one job is to be executed as.

**TWO variants, and the second one is the whole of leg 2.** A round of this type carried three -
a bearer, and a PRINCIPAL the data system was asked to become on a connection the DEPLOYMENT
authenticated. The owner rejected that mechanism, so the arm is gone rather than deprecated: a
question answered under the deployment's identity while provenance reported it as the asker's has
no spelling here, which is what makes `Warehouse::IMPERSONATION` a fact about the code.

**The domain still presents three shapes** (`sutura_domain::identity::Presented`), so the
mapping is 3 -> 2 and not one to one: `BigQueryWarehouse::job_identity` is where the third becomes
a refusal (`BigQueryError::NoPrincipalSwitch`), because *can this be delivered* is a transport's
fact and `Presented::agrees_with` passes both subject shapes - they are one POSTURE.

`Copy`, because every arm is a borrow: it is read out of a request and matched on, never stored.

#### Variants

- `Transport` - Whatever identity the transport itself already holds.

  The shared posture, and the boot path - see `JobDeadline::Boot` for the other half of what
  "no caller" means to a request.
- `AsSubject` - The asking subject's own verified assertion, for the data system to authenticate itself.

  **The subject's own credential and not a stand-in for it**, which is the whole of leg 2:
  `crate::adbc` puts this behind a workload-identity credential document, so Google's own
  token service verifies it and the source executes as whatever principal the pool resolves
  the subject to. Nothing on that path runs the question under the deployment's identity.

  **One subject arm and not two, which is the deletion that makes the claim true.** There used
  to be a second - a PRINCIPAL the deployment asked the data system to become on the subject's
  behalf, on a connection the deployment authenticated - and a transport could serve that one
  while provenance reported the answer as impersonated. It has no spelling here any more, so
  the weaker mechanism is unrepresentable rather than refused.

#### Implements

`Clone`, `Copy`, `Debug`

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
each arm means to the transport's own submit.

```rust
pub const fn default_dataset(&self) -> &DatasetId
```

The dataset the statement's unqualified table names resolve in.

**This is why the generated statement needs no qualifying**, and it is the reason the fourth
dialect changed nothing about how a table is rendered: the endpoint's request carries a default
dataset beside the SQL, so a bare backticked table name resolves there. The generator emits the
same shape it emits for every other dialect.

```rust
pub const fn identity(&self) -> JobIdentity<'_>
```

Who this job is to be executed as.

**This is the half that makes a `BigQuery` source execute as the asker**, and which of
`JobIdentity`'s arms a leg carries is decided once, above, from what the broker presented -
never re-derived here. A transport that cannot serve the arm it is handed refuses; one that
ignored it would answer as itself while provenance reported the asker.

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

**The rest send nothing, and they are listed BY KIND rather than counted.** The count was wrong
twice, and then a third time: the sentence said *three more members* while five predicates had
been added under it, because a number in prose is a second thing to keep true and no gate reads
this one. So: `list_tables` sends a metadata read rather than a statement, which is what makes
it cheap enough for a boot check; `apply`, behind the `fixtures` feature, is the second
statement-issuing method - present only in a build that loads fixtures, so no deployment can
reach it; and every remaining member is a PREDICATE that sends nothing and asks the implementor
about a failure it already holds, because `Self::Error` is the implementor's own type and the
adapter above it cannot read one.

### `type_alias DryRunEstimate`

A dry run's own byte estimate, when it priced one - `None` is `docs/adr/0030`'s honest absence,
never a defaulted zero.

**A named alias rather than `Option<EstimatedBytes>` written out at every return type**, because
this exact shape - wrapped in a `Result` - is the return type of `JobTransport::validate` and
every one of its implementors, fake and real; a name spares each of those sites the `Option` and
says what the value MEANS at the read site, which the bare composed type would not. It does not
cross this workspace's `clippy::type-complexity` threshold - it is well under it - so the alias
earns its place on readability alone, not on a lint that does not fire either way.
