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
- one federated LEG, rendered through the same `sutura-sql` at the same dialect, which is what
  makes two `BigQuery` sources federate with a per-subject credential at each
  (`telekom/sutura#929`); the refusal this replaces is gone rather than relaxed - see
  `BigQueryWarehouse::EXECUTES_LEGS`;
- handing the driver's Arrow batches to the interior's own decode, which is where a wrong
  number would come from and which is no longer this crate's code (`docs/adr/0039`);
- the boot pre-flight, which asks each dataset once - not once per model - whether it holds the
  tables the bundle names, so a mistyped table name costs a boot refusal here as it already does
  on a `files` deployment rather than a failed answer for whoever asks first.

**A limit of that mapping, stated because it decides what a time column on this source is:**
`sutura_domain::warehouse::arrow` maps `Date32` and refuses every timestamp type, so a
`TIMESTAMP` or `DATETIME` column is refused NAMING its Arrow type and fails the answer - the
correct and loud outcome. A time column therefore has to be a `DATE` here. The refusal moved
there with the rest of the mapping (`docs/adr/0039`); it used to be this crate's own
`FieldType::Unmapped` over a type NAME the deleted HTTP transport read out of a JSON schema.

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
- `NoPrincipalSwitch` - The leg presents a principal for the data system to switch to, and no transport here has a spelling for one.

  **Reinstated, and the reason is the mechanism reversal rather than a revert.** For two rounds
  this adapter delivered exactly that shape, through the driver's `target_principal` option and
  the deployment's own application default credentials - so the connection was the
  deployment's and the subject's credential was nowhere in the chain. The owner rejected it,
  so `crate::transport::JobIdentity` carries one subject arm now and this refusal is what a
  broker presenting the other one gets. Accepting it would submit the job under the
  credential the transport already holds while provenance, read off this source's posture,
  reported the answer as impersonated.
- `NoImpersonationTarget` - A subject's own credential arrived with no account declared beside it.

  **Refused rather than run as the pool principal, which is the half-configured state this
  variant exists to keep off a dataset.** The credential document's
  `service_account_impersonation_url` is what makes a declared account decide anything; with
  no account there is nothing to name, and the alternative to refusing is a question that runs
  as whatever principal the pool resolves the subject to while the deployment's `impersonate`
  map says it runs as somebody specific. `telekom/sutura#929`'s review is explicit that a
  security-critical setting must not be accepted and then ignored - and *silently widened* is
  the same defect from the other side.

  Reachable only from a broker that is not `DeclaredPrincipalBroker`: that one mints the
  account off the map it parsed, so a served deployment refuses the declaration at boot
  instead. `Presented` is a public port, so the refusal is typed rather than an
  `unreachable!`.
- `PresentedDisagreesWithPosture` - The leg's credential and this source's declared posture do not agree.
- `Unreadable` - A result column could not be read as a domain value.

  **This one variant replaces seven**, and `docs/adr/0039` is the record. The seven were an
  unmapped type, an `INT64` that did not parse, a `FLOAT64` that did not parse, a `BOOL` that
  was neither spelling, a non-finite double, a date that did not parse, and a row whose width
  disagreed with the schema. Every one of them existed because the deleted HTTP wire transport
  received each value as TEXT whatever its declared type was, so "declared an integer" and
  "parses as an integer" were two separate facts this adapter had to check. The ADBC driver
  hands back typed Arrow arrays, so there is no text to re-parse and no place for those four
  parse failures to occur; what remains is the interior's own mapping and its errors.

  The column is named one level down, on `UnreadableCell`, which is where every adapter now
  names it.
- `NoIdentityInTheAnswer` - The identity read came back as something other than one row of one text cell.

  Its own variant rather than `Self::Unreadable`, because what a caller does about it is
  different: that one is a result set this workspace could not map, and this is
  *the endpoint did not tell us who ran the job* - which for the one caller that asks
  (`BigQueryWarehouse::session_user`) is the whole
  answer rather than a cell of it.

  **It carries the SHAPE and never the value**, deliberately. The one thing this answer can
  contain is an account identifier, and the venue that reads it writes to a public log - so a
  refusal that quoted what came back would be the disclosure the read exists to check for.

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
pub const fn over_adbc(source: SourceName, posture: SourcePosture, billing_project: ProjectId, default_dataset: DatasetId, driver: adbc::DriverLocation, impersonation: adbc::Impersonation, max_bytes_billed: adbc::BytesBilledCeiling) -> Self
```

Opens a dataset over the ADBC transport.

`driver` is where this process reaches the self-built driver: linked into a release
artefact's own binary, or a `.so` a deployment mounted. `adbc::DriverLocation` carries why
that is a parsed value and not a path, and `nix/bigquery-adbc.nix` builds both shapes from
one pinned source.

`impersonation` is whether this source impersonates and at what scope - the source's declared
`workload_identity.scope`, or `adbc::Impersonation::Disabled` for a shared one. Taken here
rather than read per request because it is a property of the source, and a declared scope the
driver would refuse then fails before a listener is bound.

`max_bytes_billed` is the source's own `sources.<alias>.max_bytes_billed`, already parsed:
every job this transport submits carries it as `BigQuery`'s `maximumBytesBilled`, so the bound
on bytes scanned is enforced by the service. `adbc::BytesBilledCeiling` states what it does
NOT bound - it is per JOB, and `governance.per_replica_spend_ceiling` is a different key that
this adapter still does not reach.

```rust
pub fn session_user(&self, presented: &Presented) -> Result<SessionUser, BigQueryError<<T as >::Error>>
```

Who this data system says the leg presenting `presented` is executing AS.

**The observable for the claim this adapter's `IMPERSONATION` constant makes.** A
`Presented::SubjectToken` becomes this job's own credential - the subject's assertion
federated, then impersonating the account declared beside that subject - so what the
endpoint resolves it to IS the identity the source executed under, and asking the source
rather than asserting it is the difference between evidence and a comment. `docs/adr/0008` names
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
`BigQueryError::Unreadable` where the one cell is not a text this workspace maps, and
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

Presents the asking subject's own verified assertion at a source that declares it, beside the
account declared for that subject - and the operator's witness for a shared one.

**The assertion AND the account, because either alone loses the property.** The assertion is
what the caller possesses and what the pool verifies; the account is what a deployment declared
this caller's questions should run as, and a broker that presented only the assertion ran every
declared caller as one pool principal whatever the map said.

**Both maps, because one plan may read one of each and a broker is per answer rather than per
source** - the reason `docs/adr/0008` part 4 gives for a broker being per answer at all.

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

Two variants, and the second one is the reason the enum was one from the start: an empty
declaration can serve nobody, and a declared ACCOUNT this transport cannot name in a request is
the same class of defect one level down.

## Module `adbc`

The ADBC transport: opens the self-built `BigQuery` driver
(`nix/bigquery-adbc.nix`) through `adbc_core` + `adbc_driver_manager` and
decodes its Arrow result sets.

```text
adbc_core + adbc_driver_manager → C ABI → the `BigQuery` ADBC driver
  → `BigQuery` → Arrow RecordBatchReader → decode::Decoding → RowSet
```

**Two routes to that driver and one type deciding between them** - `DriverLocation`, resolved
once at composition. A release artefact carries the driver in its own link (the `c-archive`
half of one nix derivation), which is the only route a STATIC musl binary has; a source build
opens a `.so` a deployment mounted. `location` carries why that is a parsed value rather than
the arbitrary path `telekom/sutura#929`'s sixth finding named.

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
- `Unannounced` - A batch did not carry the fields the driver's own announced schema said it would.

  **The check is `sutura_domain::warehouse::Accumulating`'s and not this crate's**, which is
  `docs/adr/0039`'s point: a foreign driver streaming over a C ABI is exactly the case to
  refuse rather than trust, and the obligation is the same for every adapter that has one.
  This variant also carries the row ceiling being reached - see `MOST_RESULT_ROWS`.
- `Uncovered` - ADBC does not yet cover a port method this transport was asked for.
- `NoDryRun` - There is no ADBC call that prices a statement without running it.

  **Its own variant rather than an `Self::Uncovered`, because one caller has to be able to
  tell it apart and a `&'static str` is not something to match on.**
  `JobTransport::declined_to_dry_run` reads this variant and nothing else, which is what
  lets the adapter answer `PreFlight::NotAsked` for a dry run nobody made while a dry run that
  was made and failed stays a failure.
- `DeadlineSpent` - The port's own deadline was already spent when the statement was prepared, so nothing was sent: a `jobTimeoutMs` of what is left would be `0`, which the driver reads as unbounded.
- `Parameters` - The plan's values could not be assembled as the batch this driver binds them from.

  Its own variant rather than an `Self::Adbc`, because the failure is on THIS side of the C
  ABI: nothing has been sent, and what went wrong is an Arrow batch this transport built. The
  cause is Arrow's own, kept as a `#[source]` so the chain still walks.
- `SubjectSource` - The loopback source a subject's assertion is served over could not be opened.

  Its own variant rather than an `Self::Adbc`, because nothing has been sent and the failure
  is this process's own: a host with no usable loopback interface cannot serve an impersonated
  question, and saying that is better than a driver failing to fetch a token for reasons of
  its own.
- `UnusableTarget` - A declared impersonation target is not one this transport will name in a request.

  **Its own variant rather than an `Self::Uncovered`, because it is a refusal about a
  VALUE and the string in that one is a missing capability.** The account rides into one path
  segment of `service_account_impersonation_url`, which decides which account the question
  runs as, and `cloud.google.com/go/auth`'s impersonation provider POSTs that URL verbatim
  with no shape check at all. `sutura_domain::identity::PrincipalName`'s parser is the
  domain's shared one and accepts `/`, so the narrowing is this crate's.

  **It carries nothing**, deliberately: the shipped broker parses the same rule at boot and
  names the value there, where an operator can act on it, and this arm is reachable only from
  a broker that built a `Presented` by hand. A refusal at the send boundary that echoed the
  value would put a caller-influenced string into an error that reaches a log.
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

**Three owned values and nothing else, which is load-bearing rather than tidy.** There is no
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
pub const fn new(driver: DriverLocation, impersonation: Impersonation, max_bytes_billed: BytesBilledCeiling) -> Self
```

Takes the driver a composition root resolved, whether this source impersonates, and the
money bound every job it submits is capped at.

**A `DriverLocation` and not a path**, which is `telekom/sutura#929`'s sixth finding: the
driver a release artefact carries has no path, and a mounted one has been parsed before it
gets here.

**`max_bytes_billed` has no default, and that is the same argument
`crate::BigQueryWarehouse::new` makes about its own arguments**: a defaulted ceiling is
money somebody else pays. It is also why the type is `BytesBilledCeiling` and not a
number - `sources.<alias>.max_bytes_billed` is required at boot, and for the length of one
review round it was required, unparsed and sent nowhere, so a declared `0` and a declared
`u64::MAX` both booted green over a source with no bound on bytes scanned at all.

```rust
pub fn probe(driver: &DriverLocation) -> Result<(), AdbcError>
```

Does this artefact's driver load and initialise at all?

**The one thing a boot path or a diagnostic can find out about the driver without a
project**, and it is worth more than reading a manifest or an environment variable:
initialising this driver runs the GO RUNTIME inside this process, beside tokio and beside
the allocator a release build links. That is the coexistence nobody could assert while the
only caller was a question - so a link-success check would have passed and been wrong, and
this executes instead. `nix/bigquery-driver-check.sh` is the venue that runs it against the
release artefacts, and it is the whole of what makes the driver *carried* rather than
*built*.

It opens a DATABASE and stops there, deliberately. `new_database_with_opts` is option-setting
on the Go side and reaches no network; `new_connection` is where the driver builds its client
and looks for application default credentials, which on a host with none is a metadata-server
probe this has no business making. So what a success means is exactly *this driver is this ABI
and its runtime started*, and nothing about whether a question could be answered.

# Errors

`AdbcError::Load` where a mounted `.so` is absent, is not this ABI, or cannot be loaded at
all, and where a linked-in driver's own initialisation refused.
`AdbcError::Adbc` where the driver loaded and refused the database.

#### Implements

`JobTransport`

### `use BytesBilledCeiling`

The most a single job this transport submits may be billed for scanning.

**Sent as the pinned driver's `bigquery.query.max_bytes_billed` statement option, which that
driver maps onto `BigQuery`'s own `maximumBytesBilled` job configuration - so the bound is
enforced at the service and not by a check here.** A job that would exceed it fails and is not
charged. That is what makes it worth more than a client-side estimate: nothing on this side has
to be consulted, kept accurate, or trusted.

**A newtype and not the `u64` the settings tree holds, because two values in that range are not
ceilings.** Zero is `BigQuery`'s own spelling of *no ceiling* - the field is read as unset below
one - so a deployment that wrote `0` asked for a bound and would have been given none; and a
value above `Self::MAX_BYTES` is indistinguishable from no ceiling in practice. Both are
refused by `Self::parse`, which is the only constructor, at the composition root, before a
listener is bound.

**The limit, and it is the whole of what this bound is not.** `maximumBytesBilled` bounds BYTES
BILLED for one job. It is not a bound on a deployment's total spend, on one subject's spend, or
on a window - `governance.per_replica_spend_ceiling` is that key and it charges only priced
estimates, which this transport does not produce (see `super::AdbcError::NoDryRun`). It is
also not a bound on a job billed for SLOT TIME rather than bytes scanned: on a
capacity-priced reservation the bytes a job scans are not what it costs, and this ceiling then
bounds the scan without bounding the bill. And it is per JOB, so N questions cost N times it.

### `use UnusableCeiling`

Why a configured bytes-billed ceiling is not one this transport will send.

### `use Impersonation`

Whether this source impersonates at all, and against which pool when it does.

**A two-variant type rather than an `Option<WorkloadPool>`, because the absence is a
DECLARATION.** A source is opened shared or impersonating - `sutura_config` refuses a
`workload_identity` block on a shared entry and refuses its absence on an impersonating one - so
which of these a transport holds is decided once, at composition, from a value an operator wrote.

### `use DriverLocation`

Where the driver is, once something has decided that it is reachable at all.

**There is no third state and no `Option`.** A source cannot be opened without one of these,
so a composition root either resolved a driver or refused to serve - the shape
`crate::transport::JobIdentity` uses for the same reason.

### `use UnusableDriverPath`

Why a named driver path is not one this process will open.

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

### `constant MOST_RESULT_ROWS`

How many rows this transport will materialise from one result stream before refusing.

**A ceiling on ROWS and not on BYTES, which is the limit this sentence used to overstate.** It
was called a ceiling on this process's memory; a row count times an unbounded row width is not a
memory bound, and what decides the width is the plan's projection - a property of the plans a
deployment can ask, not of this constant. So what it holds is that a stream is FINITE: an
unending driver is refused, a million very wide rows are not.

It is not a cap on an answer either, and that distinction decides the value.
`sutura_domain::plan::MAX_ROWS` caps an answer and travels in the statement's own `LIMIT`; a
federation LEG carries no `LIMIT` at all - `sutura_domain::plan::leg`'s header says so, because a
leg is not an answer - so for a leg there is nothing in the statement bounding what the source
may stream back, and the only thing between a driver that streams without end and this process is
a number here. The refusal fires WHILE reading, in `Accumulating::push`, so it cannot be reached
by first materialising the whole stream.

Two orders of magnitude above `MAX_ROWS`, because it has to refuse only a stream no plan could
have asked for: a leg legitimately returns more rows than the one answer re-aggregated above it
keeps. **The VALUE is held rather than commented** - review measured that raising it to
`usize::MAX` left the whole suite green, because `delivered + n > usize::MAX` is never true and
the refusal test passes its own ceiling in. Both bounds of that sentence are asserted by
`tests::both_of_the_transports_own_result_ceilings_are_pinned_to_what_they_were_derived_from` -
which is where that sentence became true: it named a cell called
`the_transports_own_ceiling_is_two_orders_of_magnitude_above_the_answer_cap` that no file in this
tree ever defined, so for as long as the sentence stood the value was held by the sentence.

**The engine passes `usize::MAX` deliberately**, and the contrast is the reason this is the
caller's argument rather than the guard's default: `sutura-exec-datafusion` produces its own
batches from its own plan and is bounded by its memory pool, which is where `docs/adr/0009`
puts it. A foreign driver is what a row ceiling exists for.

### `constant MOST_RESULT_BYTES`

How many bytes this transport will spend holding and converting one result stream before
refusing.

**The bound `MOST_RESULT_ROWS` is not**, and round 7 of `telekom/sutura#929`'s review is the
report: a row count cannot be a memory bound when the caller controls row WIDTH, so a million
narrow rows and a few thousand very wide ones are the same number there and orders of magnitude
apart here. Both apply - whichever is crossed first refuses - and the byte one is the one that
protects the process.

**A constant here and not a configured key, and the difference from the engine's own budget is
worth stating.** `sutura-exec-datafusion` derives its budget from
`runtime.working_set_max_bytes`, which the composition root has already checked at boot against
the memory this process can reach. This crate depends on no settings crate - an adapter does not
call another adapter, and nothing hands a transport that number - so the value is written here
instead, and it is therefore **not checked against the memory available**: a container smaller
than this ceiling can still be ended by a result under it.

**The value, and why this one.** A quarter of `docs/adr/0009`'s provisional 1 GiB working set,
so two federated legs plus the combine above them cannot each spend the whole of a query's
provisional byte budget. It is provisional for exactly the reason that number is - 0009's
amendment says the corpus it was measured on is too small to justify moving it, and it names
driver buffering and row conversion as the paths its harness never observed. This is one of
them.

**The VALUE is held rather than commented**, because a ceiling nothing asserts can be raised to
`usize::MAX` with a green suite - review measured exactly that happening to `MOST_RESULT_ROWS`.
`tests::the_transports_byte_ceiling_is_a_quarter_of_the_provisional_working_set` is the cell.

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

- `Port` - A request-time call's own `Deadline`. `Warehouse::dry_run`/`execute` build this arm, and only this arm - see `JobRequest::new`'s own doc, which carries the limit: the ADBC transport sends what is left of it as the job's `jobTimeoutMs`, a server-side stop.
- `Boot` - The boot path: no caller, no request timeout. `verify_anchor`, a fixture load or drop, and the identity read build this arm. This used to add *the ADBC driver opens a fresh window under its own configured bounds instead*, and this process configures no TIME bound at all: the database options it sets are `bigquery.project_id` and `bigquery.dataset_id`, plus the three an impersonating leg chains for the credential document (`adbc::identity`'s `credential_options`), and none of them is a window - so whatever window exists is the driver's own default and is not ours to state. **It is not the only bound, and the earlier *no bound at all* overstated that:** `bigquery.query.max_bytes_billed` is set on every statement this transport submits, which bounds what a job may SPEND and says nothing about how long it may take. It is an `OptionStatement` rather than an `OptionDatabase`, which is why it is not in the list above.

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
- `AsSubject` - The asking subject's own verified assertion, and the account this deployment declared that subject's questions should execute as.

  **The subject's own credential and not a stand-in for it**, which is the whole of leg 2:
  `crate::adbc` puts the assertion behind a workload-identity credential document, so
  Google's own token service verifies it and resolves the subject to the declared pool's
  principal. Nothing on that path runs the question under the deployment's identity.

  **`target` is the SECOND hop and is a field rather than a third arm**, because it is not a
  second mechanism: the credential the driver ends up holding is still derived from the
  caller's own assertion, and the pool principal impersonating a declared account is one chain
  with two links. It is not the deleted principal switch, which ran from the deployment's own
  application default credentials with the caller's credential nowhere in the chain - that has
  no spelling here and a broker presenting it is refused by
  `BigQueryError::NoPrincipalSwitch`.

  **Both fields are read by `crate::adbc`**, and a transport that read only `assertion` would
  run every declared caller as one pool principal while a deployment's `impersonate` map said
  otherwise.

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

**This is the half that decides who a `BigQuery` job is executed as**, and which of
`JobIdentity`'s arms a leg carries is decided once, above, from what the broker presented -
never re-derived here. A transport that cannot serve the arm it is handed refuses; one that
ignored it would answer as itself while provenance reported the asker. It does not make the
source execute as the asker on its own: `JobIdentity::AsSubject` carries the subject's
assertion and the account declared for that subject, and the declared pool is what resolves
the assertion to a principal able to impersonate it - unproven against a live pool, per
`docs/where-identity-is-proven.md`.

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

**This is the only reported-total cross-check left in this crate.** There used to be a second,
`BigQueryError::Incomplete`, comparing a query answer's delivered count against the endpoint's
own `totalRows`; `docs/adr/0039` records why an ADBC read's completeness is the full drain
instead, and it went with the paging it described. A LISTING still carries a total, because a
dataset listing is a metadata document and not a result stream. This type carries the inventory
evidence; preflight decides whether it leaves a requested table unaccounted for and refuses
through a value.

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
