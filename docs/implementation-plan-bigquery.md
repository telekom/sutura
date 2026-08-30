# The plan, BigQuery

The BigQuery steps of [the implementation plan](implementation-plan.md), on their own page.

**The stack table on [the main page](implementation-plan.md) stays the only owner of a step number.**
These are rows 15 and 16.

**Two reasons for the page, and NEITHER is the line limit** - which is worth saying because the first
split of this plan was made for that reason, the limit no longer applies to prose, and a page that
justified itself by a rule that has since been removed would be a page with no justification. The
reasons that stand on their own:

- **BigQuery's identity route is a long argument with three options**, each spending a different
  control, and interleaving it with the Postgres steps made both harder to follow. A seam at a source
  rather than at a phase is a departure from the first split - that one cut at *needs a live service
  or an identity decision* - and it earns it here.
- **Two branches were editing the identity page at once.** Moving these sections out returned that
  file to byte-for-byte what the other branch expected, so the two merge without a conflict. That is
  a real reason and an unglamorous one, and it is the kind that actually predicts whether a split
  survives.

## Before either step: authenticating a developer against a test instance

**`just gcloud-login` is the whole entry point**, and it lands ahead of the adapter because the
decision the first step is blocked on - *what does a test run against* - cannot be made without
something live to try it against. It performs both logins that matter, in one task: `gcloud auth
login` authorizes the CLI, and `gcloud auth application-default login` writes the credential a
client library reads. Doing the first and forgetting the second is the failure that reads as a
broken adapter.

**The CLI is a pinned container rather than a package**, and `pixi.toml` carries the argument:
conda-forge publishes no `win-64` build of it, this workspace declares that platform, and the
manifest's own rule is that tooling which cannot resolve on a platform we ship to is tooling nobody
can run there. The alternative - a package on three platforms and a container on the fourth - is two
invocations to keep in step, and the one that drifts is the one nobody runs. The image is reached
through the same registry variable every service in the development tier uses, so a network behind a
registry mirror needs no change here; `xtask`'s own suite compares the two defaults, because the two
spellings cannot be compared by reading.

**Nothing lands in this repository.** Both logins write into the developer's own gcloud
configuration directory, which is bind-mounted into the container, so a native `gcloud`, `bq` or any
client library on the host is authenticated afterwards - and there is no repo-local credential to
leak or to clean up. `CLOUDSDK_CONFIG` is honoured where that directory has been moved, which is
also how a developer on Windows points at it.

**Which project, dataset or location a developer works against is not written down here and will not
be.** A developer names it in their own environment: `.envrc` already sources a file under the user's
own configuration directory for exactly this class of value, and [building without direct
egress](enterprise-mirrors.md) is the generic form of the same split. This repository is public, so
the value belongs on the machine and only the hook belongs here.

**This is tooling availability and not the adapter.** No Rust dependency, nothing in
`sutura-config`, no `Dialect::BigQuery`. Everything below is still to do.

### Which identity this login serves, and which it does not

Stated because the answer is easy to get wrong in the direction that matters, and because a reader
who gets it wrong concludes that BigQuery authentication is solved when the part that matters is not
built. [Row 8](implementation-plan.md) gave a source exactly two spellings, and this login serves one
of them:

- **`SharedServiceUser` - what this login is for, and it is genuinely for it.** One identity reaches
  the source on behalf of everybody who asks. On a developer's machine that identity is the developer,
  established by `gcloud auth application-default login`; in a deployment it is a service account, and
  the credential arrives as static configuration. **Not throwaway scaffolding:** single-user mode is a
  first-class posture that [0008](adr/0008-a-credential-per-leg-for-the-calling-subject.md) part 5a
  calls a shape rather than a degradation, and it needs exactly this. So the row below can be built and
  demonstrated with nothing more than what `just gcloud-login` provides.
- **`ImpersonationAtSource` - what this login is NOT, and cannot become.** A credential is minted per
  leg for the subject who asked, so two subjects reading the same metric get different rows. Nothing on
  this branch moves towards it. It needs the credential port, a workforce identity pool, and an
  RFC 8693 exchange whose subject token this deployment does not hold when it starts - none of which an
  application-default credential participates in.

**They are not two points on a spectrum, and that is the whole reason this section exists.** An
application-default credential is *one identity for every caller*. The federated exchange is *one
identity per asker*. Widening the first never reaches the second; the second replaces it. A deployment
that authenticated with `gcloud` and then read every row as the developer has not partially
impersonated anybody - it has answered every question as one identity, which is the posture
`SharedServiceUser` exists to make a deployment declare out loud.

**Where a service account sits in the federated chain is a deployment decision and not settled here.**
The federated principal can hold the dataset grants directly, or it can impersonate a service account
that holds them - Google recommends the first and documents services where only the second works. Both
are different from the credential this login writes, and the choice belongs with the pool's owner. The
per-subject step below is where it gets recorded.

## BigQuery, on a service account

**Goal.** The first cloud data source, and the one the deployment actually cares about. Queried
directly, no federation, no impersonation - a warehouse declaring `SharedServiceUser`, exactly the
posture `examples/single-player` ships, authenticated by a service-account key or by the ambient
credential the host provides.

**Why this is ahead of Postgres now, where it used to be absent.** The stack table's ordering
argument has always been cost - Postgres is nearly free, because the dialect is compiled and the
statement and parameter goldens exist. That is still true and it is still not the deciding argument,
because **BigQuery is where per-subject execution has to work and Postgres is where it would be nice
if it did.** An ordering that puts the cheap step first delivers the cheap step first; this stack is
paid for by the expensive one.

**This step is NOT nearly free, and the difference from Postgres is one line of manifest with a
measurable blast radius.** There is no `Dialect::BigQuery`: the enum is `DuckDb`, `Postgres`,
`ClickHouse`, and `dialect::ALL` is a `const` with an exhaustiveness test, so a fourth variant is a
generator arm and a full dialect of goldens rather than a registration. `polyglot-sql` does carry a
`dialect-bigquery` feature - checked in the pinned 0.9.2's own manifest, alongside 32 others - so
nothing has to be written from scratch, and the cost is the corpus:

- **63 snapshots become 84**, being 21 questions times four dialects. **Landed, and the prediction
  held exactly:** 84 files under `crates/sutura-app/tests/snapshots/` now contain `LIMIT 10001`, and
  52 snapshot files were added in total - 42 for the question corpus and 10 for the five leg fixtures,
  which carry no row cap and so do not move the counted number. No EXISTING snapshot changed, which is
  what makes the fourth dialect additive. **The count later moved to 88:** the `week`-grain question
  below adds a twenty-second question, and its BigQuery statement carries the row cap too, so `88
  files ... contain LIMIT 10001` is what `AGENTS.md` reads today - the same `check-guidance` gate holds
  that number.
- **`cargo xtask check-guidance` fails until AGENTS.md says the right number.** Not incidentally - the check reads
  the number written before the marker `SQL goldens read` and compares it to what it counts, which is
  the mechanism that caught `39` after the corpus had grown. So the invariant row is part of the
  change, and the gate puts it in the diff rather than trusting anyone to remember.
- **The placeholder style is a third variant or it is a bug.** `PlaceholderStyle` is `Question` and
  `Numbered`, and BigQuery's job API takes either positional parameters or `@name` named ones. Decide
  it against the client's actual request shape, not against the dialect layer's rendering, because the
  two are separately capable of being right.

    **Decided: `Question`, and no third variant.** Verified against the REST reference rather than
    inferred - the request body carries `parameterMode`, positional parameters are written `?` and
    supplied as an ORDERED array whose entries omit `name`, and a query may use one form or the other
    and not both. A `GeneratedQuery` already carries an ordered list of values and no names, because a
    parameter's identity in a plan IS its position, so positional matches end to end. Named would need
    a name invented per parameter, a third `PlaceholderStyle` and a map on `GeneratedQuery` - three new
    things with nothing in the domain to fill them. `transport::JobRequest::PARAMETER_MODE` is where
    the adapter states it.

- **The generated statement needs no dataset qualifying, and that was the other thing to check.** The
  job request carries a `defaultDataset`, so a bare backticked table name resolves there - confirmed
  on both `jobs.query` and `JobConfigurationQuery`, along with a single backtick-quoted unqualified
  table name being a documented table path. So the generator emits the same shape it emits for every
  other dialect, and the dataset is a declared field on the source rather than something rendering has
  to know about.

**The blocking decision is MADE, and it is [0017](adr/0017-what-a-bigquery-test-runs-against.md).** A
real project reachable from a developer's own machine, with the acceptance leg marked not-in-CI. An
emulator is refused on principle rather than on cost - it is the option that produces the most
confident-looking green - a secret in CI is refused because this repository is public and a
fork's pull request cannot see one, and the Storage Read API is the wrong instrument because it never
submits the statement. **So the corpus for this dialect claims rendering and parse-checking and never
acceptance**, and DuckDB remains the only data system that vouches for acceptance in CI.

**Writing it turned that hedge into a measured number, and the finding is worth reading before the
next dialect.** Within one target the parse check cannot see a function's ARGUMENT ORDER: both bucket
spellings, rendered for BigQuery, parse as BigQuery. `sutura_sql::dialect::DateTruncShape` is therefore
an exhaustive declaration rather than a check, and a test keeps the measurement so the claim cannot
quietly stop being true. 0017 carries the severity split - the bucket getting it wrong is a rejection
at the service, while the QUOTE CHARACTER is the wrong-number risk, because a double quote opens a
string in GoogleSQL.

**The grain is the other place the fourth dialect silently disagrees, and it is pinned rather than
left to drift.** BigQuery's `DATE_TRUNC(x, WEEK)` begins on **Sunday** - its own reference says `WEEK`
is `WEEK(SUNDAY)` - while every other dialect's week is Monday-based. So the keyword arm maps `Week`
to `ISOWEEK` (the Monday part), and 'a week bucket renders Monday' now has a question in the corpus, a
BigQuery statement that is rendered and parse-checked, and `sutura_sql::grain_keyword` documenting the
wrong number a naive `WEEK` would certify. The measured reason the shared lowercase string form stays
correct is recorded on `sutura_sql::unit`.

**Touches.** A new adapter crate; `crates/sutura-sql` for the fourth dialect; `Cargo.toml` for the
feature; `crates/sutura-config` for the source declaration; the `tests/adapters` registry; AGENTS.md
for the golden count.

**Adds.** A `Warehouse` over BigQuery: render through `sutura-sql` in the new dialect, bind parameters
as parameters, forced quoting intact, `LIMIT 10001` unchanged. Nothing about the plan or the generator
moves. **A billing project is declared, not inferred** - a federated identity has no project of its
own to bill, so the declaration has to exist before the impersonation step needs it, and putting it
here means the impersonation step does not also introduce it.

**Tests.** The whole existing golden and refusal corpus, registered for this adapter. Whether the
acceptance leg runs in CI is the decision above.

**Done when** the corpus renders and is green, the fourth dialect's 21 statement goldens have been
reviewed as a diff rather than typed, and the fixture decision is recorded rather than implied by
whatever the first test happened to do.

**Where this stands.** All three are done: the corpus is green over four dialects, the goldens were
regenerated and reviewed as a diff, and the decision is
[0017](adr/0017-what-a-bigquery-test-runs-against.md). The adapter, the source declaration and the
registry entry landed with them.

**What did NOT land with the adapter, deliberately: the wire.** `sutura-exec-bigquery` implemented
`Warehouse` and was tested against a fake transport; `transport::JobTransport` had no implementor that
spoke to the endpoint. 0017 decided that the change adding one is the change that can first verify it
against a real project - which is also where the dependency decision belongs, because an outbound HTTP
stack and a credential source reach the musl release builds and the licence gate.

**The wire landed next, and the dependency decision is
[0018](adr/0018-what-the-bigquery-wire-is-built-from.md).** `jobs.query` called directly over `ureq`,
behind a default-off `wire` feature, plus a second narrow port - `wire::credential::AccessTokens` -
whose one implementor reads the file `just gcloud-login` writes. What decided it was not API surface:
every wrapper crate, the official preview one included, pulls **`anyhow`** transitively through
`prost`, which is a claim this workspace makes in writing and enforces with a gate. Two of them pull
an Arrow major that is not the engine's and one pulls `openssl`. The chosen client costs **zero new
packages in `Cargo.lock`** - measured, 446 before and 446 after - because `libduckdb-sys` already
resolves exactly that version and feature set, and it therefore costs the licence allowlist nothing
either.

**And 0017's prediction about itself did not come true, which is the part to read before believing
any of this.** The change that wrote the wire could NOT run it: the machine had no `gcloud`, no
application-default credential and no project. So the acceptance leg exists as
`crates/sutura-exec-bigquery/tests/acceptance.rs`, three `#[ignore]`d tests behind
`just bigquery-acceptance`, and **it is unexecuted**; no composition root links the crate,
`sutura-serve` still refuses `kind: bigquery` by name, and the `data_systems:` axis still gains no
entry. The honest summary is 0017's sentence with one word moved: **the statement is right as far as
five mechanisms can tell, and nobody has run one.** That sentence is what the per-subject step below
inherits, and the first thing that step owes is a green acceptance run rather than more code.

## BigQuery, per subject

**Goal.** The first real impersonation, and the one that matters most. A query executes as the
subject who asked it, and two subjects get different rows.

**Builds on the adapter above rather than introducing one.** Driver, dialect, source declaration and
corpus registration are merged and green on a service account, so this step changes one thing: how the
connection is authenticated. Same shape as the Postgres pair below, and for the same reason - if the
identity route fails, it blocks one step rather than the whole cloud story.

**Blocked on one verification, to do FIRST, and it is the riskiest item in this plan:** *can the
enterprise identity provider mint an **ID token** whose audience is a third party's provider?* The
chain needs it, and nothing else in the chain is in doubt:

1. The agent presents a token whose audience is **sutura**. Correct per RFC 8707 - a token presented
   to us must be for us.
2. sutura validates it; the subject is established. This is [0014](adr/0014-how-a-caller-proves-who-it-is.md).
3. **Exchange one, at the enterprise provider** (RFC 8693): swap it for a token whose audience names
   the workforce pool provider.
4. **Exchange two, at the cloud provider's security token service**: that becomes an access token for
   the federated principal, and IAM on the dataset decides what it may read.

Steps 1, 2 and 4 are documented mechanisms with documented request shapes. **Step 3 is the one that
can simply not be available.** The pool provider takes a subject token typed as an ID token; RFC 8693
lets a client *ask* for `requested_token_type: id_token`, and an enterprise provider commonly answers
a token-exchange for a registered downstream resource with an **access** token instead - which is
precisely what the pool provider will not accept. So the verification is not "does token exchange
work", it is "does token exchange return an ID token for an audience we do not control".

**And a cheaper path to price at the same time, because it may remove step 3 entirely - through a
much narrower door than the obvious one.** An earlier version of this paragraph said the pool
provider's *allowed-audiences list* is configurable, and **that was wrong**: `allowedAudiences` is a
field on a **workload** identity pool provider. A **workforce** provider's OIDC object has five
fields - issuer, client id, client secret, JWKS and single-sign-on configuration - and no audience
list at all. Its client id **is** the only audience it accepts.

So the shortcut survives, and it is: **set the workforce provider's client id to the audience
sutura's own token already carries.** One exchange, no provider round trip, no ID-token question at
step 3. Three consequences the wrong version did not state, and the third can foreclose it outright:

1. **It is one value, not a list.** The provider then accepts sutura's audience and nothing else -
   which is tighter than an allowlist in one direction and completely inflexible in the other.
2. **The single-sign-on configuration is required on a workforce OIDC provider, so that same client id
   is also the client the console and the CLI use for browser sign-in.** Sutura's front-door client
   registration and the workforce sign-in client become **one registration at the identity provider.**
   That is the real coupling being bought, and it is worse than the earlier framing in a specific way:
   an allowlist entry can be revoked, and there is no allowlist entry here to revoke.
3. **The front-door token has to be an ID token, JWT-formatted.** The exchange takes
   `subject_token_type: id_token` for a workforce provider. If sutura's front door accepts an opaque
   access token - which is the ordinary shape for a token presented to a protected resource - the
   shortcut is foreclosed before the audience question arises. **This is a token-type boundary, not
   only an audience one**, and describing it as "less isolation" understated it.

**The cost that was stated correctly stays correct:** a leaked sutura token is replayable at the
exchange endpoint as that subject. So the honest framing is *more plumbing versus less isolation plus
a merged client registration* - and it is a decision for whoever owns the pool and the identity
provider registration, not one this step may quietly take because it is easier.

**One thing this does NOT rest on, so nobody re-derives it as an escape:** "one token cannot satisfy
two audiences" is not a syntax limitation. An ID token's `aud` may be an array and may carry several
audience identifiers. That does not rescue a shared credential - it is still the wrong credential
*type* for one of the two uses, and sharing a bearer across intended recipients is the exposure
audience restriction exists to prevent.
 . "
" . **A third route exists, it is newly the most practical one, and it changes what the blocking
verification is for.** The two options above are both **workforce** pools. A **workload** identity
pool is the one that has a configurable `allowedAudiences` list - up to ten values of 256 characters -
and researching whether a human subject may use one produced a clearer answer than expected:

- **Nothing prohibits it.** There is no documented statement that a workload pool subject must be a
  machine, and the documentation's own language assumes a person: `google.subject` is described as
  *"a unique identifier for the user"*, attribute conditions are described as restricting *"which
  users"* may federate, and the spoofing guidance reasons about a user changing *their own email
  address*. That is not permission granted, but it is not silence either.
- **A JWT-format ACCESS token is explicitly acceptable**, which is what dissolves the token-type
  boundary. Workload pools take `subject_token_type: jwt` as well as `id_token`, and the guidance says
  in as many words that ID tokens are *typically best* but that an access token works provided it is
  JWT-formatted and its issuer serves a discovery document. Opaque tokens are not supported, and that
  is the real precondition: **our front door must issue JWT access tokens, not opaque ones.** Many
  providers do; ours is a question to answer rather than assume.
- **BigQuery is GA for this and carries no workload-pool limitation.** Its row reads *"no known
  limitations"* in the Google-Cloud-API column, and every BigQuery exception listed is scoped to
  **workforce** by name. That is a meaningful signal rather than an absence, because the same page does
  record a workload-specific gap where one exists - Cloud Run's - so the column is not merely unfilled.

**So the chain collapses to one exchange, with the front-door token used directly.** Which is what
makes this the most practical route: it removes the ID-token question at step 3 entirely, and it
avoids the merged client registration that the workforce shortcut buys.

**What it costs, stated as plainly as the benefit.** The provider-URL audience default is not an
arbitrary convention - it is documented as a **confused-deputy defence**, in those terms: requiring the
provider's own URL as the audience *"helps reduce the risk of a confused deputy attack, in which a bad
actor presents a token that wasn't intended to be used for Workload Identity Federation, but for some
other API."* Widening the list to our own audience is the one configuration that deliberately removes
that control, and the framing to keep is that **this is turning off a named security default, not
exercising a neutral option.** The compensating control the guidance itself points to is an
`attributeCondition` pinning the token to a trusted issuer and tenant, and a route taken this way
without that condition has spent the control and bought nothing back.

**Two mechanical caveats, both real and both cheap:**

- **The billing and quota project has no workload-pool fallback.** The documented precedence names a
  service account's project and a *workforce* pool's user project, and then says a request with none
  **fails**; workload federation is absent from that list. So the project is set explicitly - a
  request header, a client option or an environment variable - and the caller needs the
  service-usage-consumer role on it. This is why the adapter step above declares a billing project
  rather than inferring one.
- **The subject attribute maps an immutable claim.** `sub`, not `email`: the spoofing guidance is
  explicit that a provider may let a user change their own email address or aliases, and a principal
  bound to a mutable claim is a principal somebody can become.

**Which route the plan takes is not decided here, and deliberately.** Two of the three spend a
security control and one is blocked on a capability we have not confirmed, so this is an architecture
decision with an owner outside this repository. What the plan owes is that the three are priced the
same way - mechanism, precondition, and the control spent - so the decision is made on the trade
rather than on which one somebody found first.

**Touches.** The adapter crate; `crates/sutura-config`; the credential port from step 12.

**Adds.** The warehouse declares `ImpersonationAtSource`. A credential is minted per leg, for the
calling subject, per [0008](adr/0008-a-credential-per-leg-for-the-calling-subject.md). **Not** a
service account that the request's identity is passed to as an argument: that is the shared-service-user
posture with extra steps, and the source would authorize the service account.

**Tests.** Compose tier cannot help here - there is no local cloud - so these run wherever the fixture
decision put the acceptance leg.

- **Two identities, a row access policy at the source, different rows**, asserted against what each
  identity is entitled to rather than merely against each other. "Different" alone passes against a
  fixture that differs for the wrong reason.
- **The session reports the borrowed subject.** The job's own principal is readable, so the test
  asserts the query ran as the asker.
- **A credential is not reused past its expiry.** A minted access token is short-lived by design and
  the job outlives it, so a question after expiry mints again - the assertion that keeps a per-subject
  credential cache from becoming the long-lived session 0008 rejects.
- **The exchange failing is a refusal, not an error.** A subject the pool declines to federate is a
  question this deployment cannot answer, which is `ToolOutcome::Refusal` in the `Ok`, per
  [0005](adr/0005-a-refusal-carries-a-status.md). A caller must not be able to mistake it for a hiccup
  and retry.

**Done when** two subjects get different rows through the same question, the ID-token verification has
an answer recorded either way, and - if the answer was the allowed-audiences shortcut - the audience
cost is written into 0008 rather than left in a commit message.
