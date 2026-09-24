<!-- GENERATED FILE - do not edit.
     Written by docs/.tools/rustdoc_to_markdown.py from rustdoc JSON.
     Edit the doc comments in the crate source instead, then regenerate.
     The commands are on the API reference landing page. -->

# sutura-app

The public API of `sutura-app`, rendered from rustdoc JSON.

The service: what happens between a question arriving and an answer leaving.

Generic over the ports and holding no framework types, so it can be exercised against a fake
warehouse in a test and a real one in production without either of them knowing. The composition
root decides which; this crate never names an adapter.

Two entry points, and the order between them is the point:

`verify_and_validate` re-executes every metric that declares a certified number, against the
`Warehouse` it is handed, and hands back the bundle as a `Validated` one only if every anchor
reproduced its number. `answer` takes nothing else. So **a bundle whose anchors were never
checked cannot reach the query path** - not by discipline, and not because a caller was asked to
call the two in order: `Validated` has no other constructor, and the one it has takes a
warehouse and calls it.

That is the correction to what this crate used to claim. The proof used to be
`sutura_domain::pinned::Validated::new(pinned, &report)`, and `AnchorReport::new`,
`AnchorReport::record` and `AnchorCheck::Matched` are all public - so any caller could enumerate
the bundle's anchors, record `Matched` for each without opening a data system, and get a bundle
the service would serve. The golden suite did exactly that. The wrapper attested to the caller's
own assertion and read like proof, which is worse than no wrapper. `verify_anchors` survives
because a report is worth rendering to an operator; what it cannot do any more is mint the proof.

`surface` is those same two entry points with the ports' generic parameters erased, for a
transport whose request handler is a concrete function. It is a *driving* port and it lives here
rather than in a transport crate, which is a correction: it used to be `sutura-http`'s, and a
second transport would have had to depend on the first to reach it. Nothing in that module names
a framework type, so this crate still holds none.

## `enum ServiceError`

```rust
pub enum ServiceError<E, M, C>
```

Why the service could not produce an outcome.

Neither variant is a refusal. A refusal is something the caller asked for and may not have; these
are the data system being unreachable and our own bundle or generator being wrong, and offering
either as a refusal would invite a caller to retry a different question forever.

Generic in the warehouse error rather than boxing it, so the adapter that failed keeps its own
typed error all the way out. A `Box<dyn Error>` here would be the same loss of information the
boundary gate bans `anyhow` for, arrived at by a different route.

### Variants

- `Compile` - The pinned bundle would not compile this question, or the splitter built a two-source plan this workspace could not then assemble.

  **Both are our own side being wrong, which is what keeps them out of a refusal.**
  `telekom/sutura#338` is the second one's report: it used to arrive as
  `RefusalReason::FederationNotExecutable`, which is what a build whose adapter type does not
  declare `Warehouse::EXECUTES_LEGS` is told, so a wiring defect was indistinguishable from a
  build that cannot run a leg. `sutura_semantic::CompileFailure` keeps them apart and keeps the
  typed cause.
- `Warehouse`
- `Combine` - The federated combiner could not assemble the two legs' results.

  **Typed in the COMBINER's own error, which is what the port being a port buys.** `docs/adr/0039`
  step 3 moved the combine into an adapter, so the cause is that implementor's type exactly as
  `Self::Warehouse`'s is a data adapter's - and `sutura-app` still names no engine.

  **An internal defect rather than a refusal, and only because the two governance outcomes are
  taken off it FIRST.** `answer_federated` asks
  `FederationCombiner::working_set_exhausted`
  and then
  `answer_not_well_formed`,
  so what reaches here is a leg result that does not carry a label the plan named, or a plan
  that would not build - this workspace's own wiring, which no caller caused and none can fix.
- `Miswired` - The federated path's own wiring produced a shape it cannot answer for.

  Every arm is unreachable through the one production splitter - see `FederationMiswired`.
- `Broker` - The credential broker could not mint. Nothing about the question was wrong.

  **Its own variant rather than a refusal, and its own variant rather than sharing the one
  above.** A refusal would let a client library retry a governance decision until something
  works, which is what `sutura_domain::query::ToolOutcome` exists to prevent. And sharing
  `Warehouse` would collapse two causes a caller has to act on differently: `docs/adr/0014`
  makes the point that a caller told "unavailable, retry" against an authorization-server
  outage will retry successfully, while one told the same against a bound that fires again
  retries forever.
- `Posture` - The broker's answer does not agree with the request it was made for.

  **A wiring defect between the broker and the request, so an `Err` and not a refusal** - the
  question was fine. Four things can be wrong and the domain's own enum names them: the grant
  was minted for a different subject, it covers a different set of sources, its deadline had
  already passed, or the refusal named a source nobody asked about.

  **It used to carry one of them**, a bare "nothing was granted for this source", and the other
  three were not checked at all. Widening the cause rather than adding three variants is the
  shape of the fix: they are one question asked once, and a transport that had to tell them
  apart would be a transport making a judgement about our own wiring.

  Executing anyway is the alternative this variant exists to remove, and it is the one that
  would have run the leg as somebody other than the asker.
  The leg the broker minted disagrees with the posture the adapter was opened with.

  **Its own variant because the two values come from different places, and that is the whole of
  what the comparison is worth.** The broker read the settings tree; the registry holds what the
  composition root opened. A leg that says "the deployment's own identity" against a source
  declared `impersonation-at-source` means one of those two is wrong about this deployment, and
  executing anyway is the case that is silent: provenance is read off the ADAPTER's posture, so
  the answer would have been reported as impersonated while it ran as the process.

  Both shipped adapters make this comparison too, and this variant does not replace theirs - an
  adapter is the last thing before a driver and may not assume who called it. What it replaces is
  the assumption that every FUTURE adapter will remember to.
- `Credentials`
- `Unreadable` - A result came back as Arrow and one of its columns could not become a domain value.

  **This variant is where the Arrow port's decode moved to, not a new failure mode.**
  `docs/adr/0039` step 2 puts the one Arrow-to-`Value`
  decode in the interior and step 2's second half moves the CALL to the presentation edge, so
  the failure that used to arrive wrapped in an adapter's own error - `BigQueryError::
  Unreadable`, the engine's `DataFusionError::Unreadable` - arrives here instead, for every
  adapter at once.

  **An internal failure rather than a refusal, and that is today's classification kept rather
  than chosen afresh:** both adapters mapped it into their own error type, which reaches a
  transport as `Self::Warehouse` does. What it means is that a data system returned a column
  of a type this workspace does not map, or a value no domain cell can hold - a non-finite
  double, a day number that is not a date. No caller caused it and narrowing the question does
  not avoid it, which is why it is not a refusal a caller is told to act on.

### Implements

`Debug`, `Display`, `Error`

## `enum FederationMiswired`

```rust
pub enum FederationMiswired
```

The federated path's own wiring, in the three shapes it cannot answer for.

**Every arm is unreachable through `sutura_semantic::plan::federated_plan`, the one production
splitter, and each is typed anyway rather than assumed away.** A previous revision reached for
a `DuplicateLabels` combine failure for the first of them - a failure about a leg RESULT, minted
from a value that has nothing to do with one - which is the shape this enum replaces.

### Variants

- `LegsCollide` - The two legs named one data system, so the provenance record could not hold both.

  `FederatedPlan::new` refuses same-source legs, so this is a splitter invariant that changed.
- `LegsAreNotOneOfEach` - The two leg results did not resolve to one of each side.

  `FederatedPlan::legs` returns the fact leg and the lookup leg by construction, so a pair
  built from it is one of each.
- `RankedAnswer` - Ranking a combined answer produced a row set whose rows contradict its own columns.

  `FederatedPlan::rank` re-orders and truncates rows it was handed, so it cannot change a
  width.

### Implements

`Debug`, `Display`, `Error`

## `struct Answered`

```rust
pub struct Answered
```

One call's result: what the caller is told, and what it ran under.

**Two values rather than one, and the second one never reaches the caller.** The outcome is the
answer or the refusal, and it goes back through the transport. The deadline is the `Expiry` the
credentials this call executed with carried, and it goes to the audit sink - `docs/adr/0008` fixes
the record's content as the chain, the outcome, the posture per leg **and the expiry the
credentials carried**, and until this type existed there was no way for the last of those to reach
`surface::LocalService`, which is what writes the record.

**Why not on the outcome.** `sutura_domain::pinned::Provenance` rides to the caller, so putting a
credential's lifetime there would publish, on both wire surfaces, how long this deployment's
credential for a data system is good for. That is the deployment's business rather than the
asker's, and a widened wire shape is a worse place to learn it.

`None` means nothing was minted for this call: the question was declined by compilation or by the
source lookup, both of which run before the broker is asked - which `answer`'s own suite pins.

### Methods

```rust
pub const fn executed_until(&self) -> Option<Expiry>
```

How long the credential this call ran under was good for. `None` if none was minted.

```rust
pub fn into_outcome(self) -> ToolOutcome
```

What the caller is told, owned, for a transport that is about to render it.

```rust
pub const fn outcome(&self) -> &ToolOutcome
```

What the caller is told.

### Implements

`Debug`

## `fn answer`

```rust
pub fn answer<W, B, C>(definitions: &Validated<sutura_domain::pinned::PinnedDefinitions>, query: &sutura_domain::query::Query, context: &sutura_domain::identity::RequestContext, broker: &B, warehouses: &Warehouses<W>, combiner: &C, working_set_bytes: u64, deadline: sutura_domain::warehouse::deadline::Deadline, ledger: &SpendLedger, row_ceiling: sutura_domain::plan::RowCeiling) -> Answering<W, B, C>
```

Answers one question, or says why it will not.

The source lookup is not a formality. A plan names exactly one data system, and running it against
a different one would answer a question about other data under the same provenance. So the plan
SELECTS its warehouse out of the registry, and a plan naming a source this process did not open is
a refusal rather than an error: it is a governance outcome, and `SourceUnavailable` now says what
its name says - nothing is configured under that name.

**The registry is what made that refusal honest.** Under one warehouse the check compared the
plan's source against the single adapter's own, so "nobody configured this data system" and "this
is the other one of the two we opened" were the same refusal.

# Nothing here executes without a credential somebody minted

`context` says who is asking - established by the transport, never stated by the caller - and
`broker` is what turns that into what each leg presents. The credential is minted **once, for
every source the plan reads**, which is one source today and is the shape a federated answer
needs: one asker and one deadline for N legs, rather than N mintings that could disagree.
`docs/adr/0008` is the decision and `sutura_domain::identity::LegCredentials` is where the
argument lives.

The order is deliberate: mint **before** the pre-flight and before execution. A pre-flight asked
as the wrong identity answers a different question, and a subject with no credential at that
source is refused before this deployment has asked the data system anything on their behalf.

# And what comes back is checked against what was asked

A broker is an adapter outside the hexagon, so its answer is input. `Minted::agreeing_with` is the
one guard: the grant's subject must be the subject this request arrived under, it must cover
exactly the sources this plan reads, its deadline must not have passed, and a refusal must name a
source that was actually asked about. Any disagreement is a `ServiceError::Credentials` - our own
wiring, an internal failure on the wire - and never a refusal, because a refusal is a statement
about the caller's access and none of these is one.

**Three separate findings, one guard, and that is a decision rather than a shortcut.** Each of the
three could have been a check of its own next to the value it protects. Three checks are three
places the fourth case gets forgotten, and they were all the same question. What makes the single
guard un-skippable rather than merely conventional is on the domain side:
`sutura_domain::identity::BoundToTheRequest` is the only type that hands out a `Presented`, and
`agreeing_with` is the only thing that builds one.

## `fn scoped_for`

```rust
pub fn scoped_for<'a>(pinned: &'a sutura_domain::pinned::PinnedDefinitions, context: &sutura_domain::identity::RequestContext) -> sutura_domain::pinned::view::ScopedView<'a>
```

The view a request context resolves against - `docs/adr/0028`.

**Here, beside `Asked` and `crate::capability::Permitted`**, so no transport owns the
decision: `answer` below reads it, and so does every route that renders a catalog through
`Asked::context`.

**Derived from the SUBJECT, not from whether the caller presented anything else.** A verified
caller's granted set stays on `context` regardless of whether it maps to anything, so an empty
grant and no verification at all must not read alike: `Subject::TheDeploymentItself` is the
explicit single-player posture the ADR's surface table names - every non-verified surface reaches
this value and always has - while `Subject::Verified` is a caller this deployment
authenticated, whose mapped audiences decide what is visible even when that set is empty.

## `use SourceAlreadyOpen`

Two adapters were registered for one source.

Its own type rather than a silent overwrite, because whichever adapter lost would then be the one
nobody opened and nothing would say so. It is not the same failure as a duplicate *alias* in the
settings tree - that one is refused before any adapter is built - it is a composition root that
built two adapters naming one source.

## `use Warehouses`

The data systems this process opened.

Keyed by each adapter's own `Warehouse::source` rather than by a name the caller passes
alongside it, so the key and the adapter cannot disagree about which source this is - the same
reason `PinnedDefinitions::pin` computes its digest from the definitions it stores.

## `use Asked`

Everything one call was established to be: who is asking, and what it may invoke.

**One constructor, `Asked::established`, and it takes the two halves already produced by a
verification - it performs no verification of its own.** `sutura_http::capability::establish_asked`
is the one place that calls it today, handing over what leg 1 already derived - a
`VerifiedCaller`'s chain and scopes, or the deployment's own when there is none. The agent
surface's own call arrives with PR2 of `telekom/sutura#378`; until then this type adds no third
way to decide either half.

`Clone` because a transport may need to hand the same value to a blocking-pool closure that
outlives the request extension it was read from - `sutura_runtime::spawn_carrying_span` is the
reason `RequestContext` is already `Clone`, and `Permitted` clones a `BTreeSet` of at most three
elements today.

## `use Capability`

One thing this surface can be asked to do.

Closed, and closed on purpose: a third capability is a third exhaustive match to satisfy - the
walk in `Capability::next`, the scope in `Capability::scope` and the identifier in
`Capability::id` - plus whatever each transport's own match needs. There is no wildcard arm in
any of them, so a variant added here does not compile until every one of those has been answered.

**Declaration order is `Ord`, and it runs from the least to the most a caller can get out of this
deployment**: describing what is measured comes before asking for a number. A
`BTreeSet<Capability>` therefore iterates in that order, which is what makes a rendered tool list
deterministic.

## `use Permitted`

What one caller may do on this surface.

**Two named constructors and no third way in**, because the two cases are the two honest answers to
*who is asking* and a reader has to name which one a deployment is in:

* `Permitted::every_capability` - nothing established a caller identity, so there is no verified
  claim to narrow by. A single-player deployment, and the agent surface over standard input and
  output.
* `Permitted::granted_by` - a verified token's scopes decide, and **only** they do.

# It fails closed, and the consequence is named rather than softened

A verified caller whose token names no capability scope is permitted **nothing**: it sees no tools
and every call is refused. That follows from `AGENTS.md`'s *fail closed on the query path* and from
`docs/implementation-plan.md`'s own *"a caller without a scope cannot see the tool it lacks"*, and
it means a deployment that switches `security.inbound` on without authoring scopes at its
authorization server has switched every caller off.

**What keeps that discoverable rather than mysterious is the refusal**: the HTTP surface answers
`403` with `code: insufficient_scope` and a sentence naming the exact scope string to grant, so an
operator reads the fix out of the response rather than out of this comment.

# Why this takes strings rather than a parsed scope type

`sutura_http::inbound::Scopes` is where a scope claim is parsed and bounded, and it stays there.
Moving it would be taking a decision `docs/adr/0014`'s closing section explicitly reserves - *"how
[the agent surface] is reached at all, and then which crate the validator moves to ... is an
architecture decision, not a refactor"* - and nothing here needs the parse: this compares against
two fixed literals, and a string that could not be a scope simply matches neither.

## `use Validated`

A `T` that has been shown to hold up.

**The service accepts only this, so an unvalidated bundle is unrepresentable rather than
merely refused.** The field is private to the module this type is declared in, and
`verify_and_validate` is the only thing in that module which builds one.

Generic in the type it wraps, but obtainable only for `PinnedDefinitions`, and that
asymmetry is the point: validating means re-running every anchor the bundle declares, so
whatever mints this has to be able to enumerate them and to execute them. A blanket
constructor for any `T` would be a wrapper that proves nothing, which is worse than no
wrapper because it reads like proof.

## `use verify_and_validate`

Holds every cardinality declaration and re-runs every anchor, and returns the bundle only if
both held.

**Two checks rather than one, and the second was added against a defect the first cannot
see.** An anchor is one range and one number, so a dimension row that duplicates a join key
outside that range moves nothing an anchor compares - while it changes a grouped answer, and
changes it differently depending on how the plan was shaped. `crate::declared_keys` carries
the measurement, the three outcomes that are deliberately not refusals, and what the pair
still does not cover.

The one operation that produces a `Validated` bundle. It takes the `Warehouse` and calls
it, which is the whole of what the type is now allowed to claim: not "somebody asserted these
anchors match", but "these statements were executed against this data system and reproduced
the numbers their author certified".

**What it still does not claim.** `W` is a port, so a caller may pass a fake - and a fake is
exactly what the golden suite passes, deliberately, because the alternative is a test suite
that needs a database to check a refusal. What the type proves is that a warehouse was
called; that the warehouse was the one holding the business's data is a composition-root
decision no signature can make. `answer` narrows it a little further by refusing a plan whose
source is not the adapter's own.

The forgery this closes does not compile:

```compile_fail
use sutura_app::Validated;
use sutura_domain::model::MetricName;
use sutura_domain::pinned::{AnchorCheck, AnchorReport, PinnedDefinitions};

// Enumerate the anchors, claim each one matched, hand the claim to the validator.
// No data system is opened and no statement is executed.
fn _forge(pinned: PinnedDefinitions) -> Validated<PinnedDefinitions> {
    let names: Vec<MetricName> = pinned.anchored_metrics().map(|(name, _)| name.clone()).collect();
    let mut report = AnchorReport::new();
    for name in names {
        report.record(name, AnchorCheck::Matched);
    }
    // Neither the constructor that was here nor the tuple constructor is reachable.
    Validated::new(pinned, &report).unwrap()
}

fn _wrap(pinned: PinnedDefinitions) -> Validated<PinnedDefinitions> {
    Validated(pinned)
}
```

The twin of that block, which pins the names so a rename cannot make it pass vacuously:

```
use sutura_app::{Validated, Warehouses, verify_and_validate};
use sutura_domain::pinned::{NotValidated, PinnedDefinitions};
use sutura_domain::warehouse::Warehouse;

fn _served(_bundle: &Validated<PinnedDefinitions>) {}

fn _mint<W: Warehouse>(
    pinned: PinnedDefinitions,
    warehouses: &Warehouses<W>,
) -> Result<Validated<PinnedDefinitions>, NotValidated> {
    verify_and_validate(pinned, warehouses)
}
```

# It takes the registry, not one warehouse

Each metric's anchor runs against the data system that metric's own plan names, so a bundle
spanning two configured sources verifies both halves. Under one warehouse every anchor on the
second source came back as a source mismatch, which is a bundle that cannot be validated for a
reason that has nothing to do with its numbers.

**What it still does not take is an identity**, and that is the honest limit on what an executed
anchor proves. The registry says which posture each adapter was handed; it does not hand the
adapter a credential to re-run the anchor under, because the port has no parameter for one yet.
So the bundle is proven to compute its certified numbers for whatever identity each adapter is
configured with - the process, for the file engine that ships - and the composition root refuses
a bundle with an anchor on a source that declared no verification identity, which is the half
available before the port changes.

# What is refused before any anchor runs

A metric whose computation is catalog-authored SQL, unless `W` declares
`Warehouse::EXECUTES_AUTHORED_SQL`. The fragment is stored as written and nothing published
compiles it, so an adapter taking the default cannot execute the metric; refusing the bundle
here, naming the metric, is what stands between that and a served bundle with a metric that
is silently skipped or a measure quietly substituted. Read off the ONE adapter type
`Warehouses<W>` holds, the way `EXECUTES_LEGS` is - so it is a fact about the build, not
about the data. No adapter this workspace ships opts in; `docs/adr/0004` is the decision.

**"Before any anchor runs" is a placement, not an assertion.** It is true because this check
sits ahead of `declared_keys::hold` and `verify_anchors` in the body below, and the ordering
ahead of `declared_keys::hold` is held INCIDENTALLY, by the `examples/authored-sql` cell: that
catalog declares a relationship and attaches no data to it, so a block moved below `hold`
fails there first, on `declared_keys::hold`'s own refusal, rather than on this one. Nothing
separates the placement from `verify_anchors` alone, so no fixture's anchor or declared key is
ever touched to prove that half of the ordering.

## `use SpendBudget`

A byte ceiling and the window it resets on, already validated.

A plain pair rather than a re-export of `sutura_config::SpendBudget`: this crate depends on
nothing outside `sutura-domain`, `sutura-semantic` and `thiserror` - see this crate's own module
documentation - and a composition root reads the validated ceiling and window out of its
settings and hands the two primitives here, the same shape `working_set_bytes: u64` already
uses for `RuntimeSettings::working_set`.

## `use SpendLedger`

The counter: one instance per process, consulted by every question this replica answers.

**`&self`, not `&mut self`** - one ledger is shared by every request without a lock in this
type's own signature, the same shape `sutura_domain::audit::AuditSink::record` uses for the same
reason. The mutable state is inside a `Mutex` guarding the per-subject map.

## `type_alias Answering`

What answering produced, or why it could not.

A named alias because the inline form is over the complexity threshold in `clippy.toml`, and
naming it is the better half of that trade: the generic parameter is a warehouse, not a result.

## `use AnsweredRaw`

One raw call's result: what the caller is told, and what it ran under - the
`Answered` of the raw path, over `sutura_domain::raw::RawOutcome` rather
than `ToolOutcome`.

## `use RunSqlError`

Why running a raw statement did not produce an outcome.

**Deliberately not `ServiceError`.** That type's `Compile` and
`Federated` arms describe the compiler and the splitter, neither of which this path touches - a
raw statement is unparsed text, end to end. What is left is the credential half
`answer` also has, plus one arm of its own for a state the boot refusal is
supposed to make unreachable: the raw tool turned on over an adapter that does not accept raw
text at all.

## `use RunningRaw`

What running a raw statement produced, or why it could not.

## `use run_sql`

Runs one literal statement against the deployment's configured source, or says why it will not.

# PR1's scope, stated as a limit rather than left implicit

**This targets the sole registered data system, and refuses `RunSqlError::NoAcceptingSource`
where more than one is open or none is.** `docs/adr/0013`'s showcase is one Postgres source; a
deployment naming which of several sources the raw tool may run over is future work, not a
decision this function makes by omission - a second source is refused rather than guessed at.

# Otherwise, this mirrors `answer`'s credential handling exactly

Mint once, check the grant agrees with the request, check the presented leg agrees with the
adapter's declared posture - the same three findings behind the same one guard, for the same
reason: a broker is an adapter outside the hexagon, and its answer is input.

# What is different from `answer` on the way out, and why

**Every failure to execute becomes a refusal, never a `RunSqlError`.** The statement is the
caller's own text, so a syntax error, a statement timeout, or the server refusing a write inside
the read-only transaction `docs/adr/0013`'s amendment wraps every call in are all answers *about
that statement* - not an infrastructure outage this deployment must page for. What remains an
`Err` is only what happens before the statement ever reaches the data system: the broker not
answering, or credentials that do not fit.

## `use grains_coarsest_first`

The grains a metric declares, coarsest first.

A small helper the composition root uses to describe a metric, kept here so the ordering is the
same one `verify_anchors` picks a grain by.

## `use source_of`

The data system one metric's own model sits on.

Exposed for the composition root's anchor check: an anchor is asked with no dimensions, so it
resolves to the metric's own model and therefore to that model's source - which is the source whose
declared verification identity would have to run it.

**Narrower than "every source this metric's plan could read", deliberately.** A question WITH
dimensions can reach a joined model, and the plan stage refuses one that spans two sources - so for
a plan that compiles at all this is the only source there is. What it is not is a general answer for
a federated plan, and it stops being the right function the moment one exists.

## `use sources`

The data systems a bundle reads from.

Exposed because a composition root has to decide which adapters to open before it can answer
anything, and reading it off the bundle beats being told twice.

## `use verify_anchors`

Re-executes every declared anchor and reports what each produced.

Returns a report rather than a `Result`, because "this one metric no longer computes its number"
and "the data system is down" are both outcomes worth recording per metric. Collapsing either into
a single error would lose which metric, and the whole point is to name it.

## Module `surface`

What a transport needs from this crate, with the ports' generic parameters erased.

# Why this trait exists at all

`crate::answer` is generic over a `Warehouse`, and `Warehouse` has an associated error type.
A transport's request handler is a concrete function - an `axum` handler registered in a route
table by path, and named again by the macro that generates the interface description - so a
handler cannot be generic over the warehouse without the whole router becoming generic in it,
and the generated document becoming generic in it too.

`Surface` is the seam: this crate's operations, with `W` gone - and with the audit sink's own
parameter gone for the same reason, since `LocalService` is generic in that too.

# Why it is HERE and not in the transport that uses it

**It used to be in `sutura-http`, and a review was right that that is the wrong crate.** The
module comment there said a second transport - an MCP one - would consume the same trait, which
is exactly the problem: it would have made one transport adapter depend on another, and the rule
this repository is built on is that nothing depends on an adapter. A driving port declared inside
an adapter is a port every other adapter has to reach through that one.

**The tension with "a port arrives WITH its adapter", stated rather than dodged.** That rule
exists because a trait with no implementor is a guess at a signature, and `pub` hides the guess
from `dead_code`. `LocalService` moved with the trait, so the rule still holds: the port and
its only implementor are in one place, and neither is a guess. What the rule does not say is
*which* crate that place has to be.

And there is a sharper reason it is not the transport's. A **driven** port - `Warehouse`,
`SemanticCatalog` - is dependency inversion: the interior declares what it needs, an adapter
outside implements it, and the trait has to sit inside the hexagon or the direction reverses. A
**driving** port inverts nothing. The caller is already outside and the implementation is already
the application, so there is no adapter for it to arrive with: `LocalService` is not an adapter
at all, it is this crate's own service with one generic parameter erased. It holds a
`Validated` bundle and a warehouse, and every method forwards. Putting that in a transport
crate made the application's interface the property of one of its callers.

**Is the erasure an application concern or an HTTP one?** The *trigger* is an HTTP fact: a
handler is a concrete function. The *content* is not - `definitions`, `answer` and `run_sql` are
this crate's own operations, and `LocalService::start` is `crate::verify_and_validate` with
the catalog port consumed. Nothing in this file names a framework type, which is checkable
rather than asserted: `cargo xtask check-boundaries` fails on a framework anywhere in a tree it
governs, and this file added no dependency to this crate's manifest. A shape the application can
offer for free, that its callers all need, belongs with the application.

**Why not delete the trait instead, under YAGNI?** That was the other option offered, and it does
not survive being tried. Deleting a one-implementation trait leaves the concrete
`LocalService<W>`, and then a transport either becomes generic in `W` - the router, its state and
the generated document with it, which is what this trait exists to prevent - or erases `W` inside
`LocalService`, which is the same trait under another name one crate lower. So the trait is not
speculative generality; it is the only shape that keeps a handler concrete with one transport,
let alone two. What WAS speculative is the sentence about MCP, and that sentence is what made the
location wrong rather than the trait. Deleting the trait would have answered a different finding.

# Why the methods are synchronous

Because `Warehouse` is. The port takes `&self` and returns a `Result`, and the engine behind
it drives its own single-threaded runtime and blocks on it. Calling that from inside an `async`
handler on a worker thread would panic - a runtime cannot be entered from within a runtime - so a
transport moves the call onto a blocking pool. Making this trait `async` would hide that
requirement behind a signature that looks like it had been dealt with.

# Where the typed error goes

Erasing the generic means the adapter's own error type cannot survive *as a named type*. It does
survive as an error: each variant of `SurfaceFailure` keeps the cause it was built from as an
owned `#[source]`, so `Error::source()` walks the whole chain and a caller that knows what
adapter is behind the port can still `downcast_ref` to it.

**The previous shape was prose.** Both variants held a `String` message and a `Vec<String>` of
causes, walked to text at construction - so `source()` returned `None`, downcasting was
impossible, and the only thing a caller could do with the failure was print it. A vector of
display strings is a presentation of an error, not an error API. Flattening to text is still what
happens, but it happens at the *logging sink* - `cause_chain`, called by whoever writes the
line - which is the one place where text is the point.

`Box<dyn Error + Send + Sync>` and not `Box<dyn Error>`: a transport that answers on a blocking
pool sends the error back across a thread boundary. That is where the `W::Error: Send + Sync`
bound on the implementations below comes from. It is a bound on *this* type and not on the port,
so `sutura_domain::warehouse::Warehouse` is unchanged, and it is a `std` marker rather than a
framework type - a requirement a transport states, satisfied here.

### `trait Surface`

```rust
pub trait Surface
```

The service, as a transport sees it.

`Send + Sync + 'static` because it is shared between connections and moved onto a blocking pool.
A transport holds it behind an `Arc` in its request state.

### `enum SurfaceFailure`

```rust
pub enum SurfaceFailure
```

Something went wrong that is not a refusal.

Neither variant is something a caller can fix by asking differently, which is why neither is a
refusal: one is our own bundle or generator being wrong, and the other is the data system not
answering.

The variants are exhaustive and stay exhaustive - a transport chooses its status code from the
split - and each one carries the cause it was built from rather than a rendering of it.

**No audit record is written for either, and that is the limit on "every call is recorded".**
`sutura_domain::audit` records an *outcome* - an answer or a refusal - and neither of these is
one: a bundle that will not compile and a data system that did not answer are our own faults
rather than answers to a question. A transport logs them, with the cause chain, which is what
`cause_chain` is for. Widening the record to cover a failure means giving
`sutura_domain::audit::RecordedOutcome` a third variant, and that is a change to what a record
means rather than a field added to one.

#### Variants

- `Compile` - The pinned bundle would not compile this question, or the splitter built a two-source plan this workspace could not then assemble.

  **The message names neither, and that is deliberate since `telekom/sutura#338`.** The three
  sentences on this variant's path - this `Display`, the HTTP sink's log line and the MCP tool
  result a model reads - each blamed the bundle, which was true of the only cause this could
  carry and stopped being true when `sutura_semantic::CompileFailure` gained its second arm: an
  assembly failure is a defect in this workspace's own wiring, not a bundle that fails to hold
  what it names. The cause is kept as a `#[source]` and says which, so the sentence does not
  have to guess. **No sentence on this variant's path blames the pinned bundle**, and that is
  registered in `xtask`'s `ABSENCES` table rather than left to review - all three wordings are
  scanned for across every crate's library source.

  **The limit, next to the claim:** nothing drives this variant through either transport. No
  test builds a bundle that will not compile, or a plan that will not assemble, and asks for it
  over HTTP or MCP - so the `500` an assembly failure now gets and the sentence a caller reads
  with it are held by the code and by no cell. What IS measured is one layer in:
  `crates/sutura-app/tests/differential/federated.rs` sees an assembly failure as a failure
  rather than as a refusal.
- `Warehouse`
- `Broker` - The credential broker did not answer, so nothing could be executed as the asking subject.

  **Its own variant because the two outages are retried differently**, which `docs/adr/0014`
  states as a requirement rather than a preference: an authorization server that is down comes
  back, and a caller told the same sentence for both will retry a data-system outage the same
  way and learn nothing. A transport chooses a different code for it.
- `Miswired` - Credentials came back that do not fit the request: a wiring defect on this side.

  Not a refusal - the question was fine - and not `Self::Broker` either, because a broker that
  answered and a broker that could not be reached are different things to whoever is paged. A
  caller can do nothing about it, so what it becomes on the wire is an internal failure.

  **What it covers grew, and the variant did not**, deliberately: the grant naming another
  subject, covering another source set, carrying a deadline that had passed, or a refusal naming
  a source nobody asked about are one thing to a transport - this deployment is wrong about its
  own identity wiring - and four things to whoever reads the log line, which is where the typed
  cause is. Splitting them here would ask each transport to pick a status code for a distinction
  that changes nothing a caller can do.

#### Implements

`Debug`, `Display`, `Error`

### `enum ServiceNotStarted`

```rust
pub enum ServiceNotStarted
```

Why a service could not be started.

#### Variants

- `Catalog` - A catalog adapter could not produce a bundle.
- `Composition` - The catalog contributions do not compose: two sources define one element, certify different versions, or one of them supplies a kind its declaration does not.
- `NotValidated` - The bundle loaded and an anchor did not reproduce the number its author certified, or could not be run at all.

  **This is the readiness gate, and it is a startup failure rather than a degraded mode.** A
  bundle whose anchors do not hold is a set of definitions that no longer computes the numbers
  somebody signed off on; serving it would answer questions with figures nobody certified.

#### Implements

`Debug`, `Display`, `Error`

### `struct LocalService`

```rust
pub struct LocalService<W, S, B, C>
```

The one implementation: a validated bundle, the data systems this process opened, and one audit
sink, behind the ports.

Holds the bundle as `Validated`, which has no constructor other than one that executes every
anchor against a warehouse - so a `LocalService` that exists is one whose anchors held. That
is not a check this type performs; it is a type it could not otherwise have been built from.

**The sink is a constructor argument and not an `Option`.** A service cannot be started without
one, so "this deployment forgot to attach a sink" is not a state that exists - which is the
difference between a record that is always written and a record that is usually written. What the
sink then *does* with a record is the deployment's, and `sutura_domain::audit` states that limit
where the port is declared.
**It holds a `Warehouses` and not one warehouse, which is what makes more than one source
configurable.** A plan names one data system, so the registry is a lookup rather than a fan-out:
`crate::answer` selects the adapter the plan named and refuses `SourceUnavailable` when nothing
is registered under that name. The limit is stated where the type is - every entry is the same
adapter type `W`, so a deployment holds two file sources or two databases behind one adapter, and a
heterogeneous set is an architecture decision rather than a change here.
**And it holds the credential broker, which is what makes a question executable at all.** Every
answer mints once, for every source its plan reads, and `sutura_domain::warehouse::Warehouse`
has no signature that runs without the result - so a service with no broker is not a service
that answers as the process, it is a service that does not compile.
**It also holds the spend ledger, unbounded unless a composition root opts in.** `Self::start`
and `Self::start_composed` build one with `SpendLedger::no_budget` - today's behaviour, before
this counter existed - and `Self::with_spend_ledger` is how a root that read a configured
ceiling out of its settings replaces it. Not a constructor argument, unlike every other field
here: those are what a service cannot exist without, and an unbounded ledger is a real, working
default rather than an omission this type should refuse to start without.

#### Methods

```rust
pub fn start<K>(catalog: &K, warehouses: Warehouses<W>, sink: S, broker: B, combiner: C, working_set_bytes: u64) -> Result<Self, ServiceNotStarted>
```

Loads one catalog through its port, re-runs every anchor against `warehouse`, and returns a
service only if all of them held.

This is `Self::start_composed` over a single declared catalog - the metadata assembler is
what makes several compose, and a single-catalog deployment is that function's one-entry
case, so there is one code path to keep honest rather than a second one that happens to
serve.

```rust
pub fn start_composed<K>(catalogs: &[K], warehouses: Warehouses<W>, sink: S, broker: B, combiner: C, working_set_bytes: u64) -> Result<Self, ServiceNotStarted>
```

Loads every declared catalog through its port, composes them into one bundle, re-runs every
anchor against `warehouse`, and returns a service only if all of them held.

Every port is consumed here, which is what lets a transport be transport-only: it never
reads a catalog directory, never opens a data system and never decides where a record goes.
The buttons the serve/schema each press are the same, which is what keeps "the bundle this
validates is the bundle this serves" true for N sources rather than for one.

`K::Error: Send + Sync` for the same reason `W::Error` is - the cause is kept, owned, and a
startup failure is reported from wherever the composition root happens to be. The catalog's
type parameter is `K` and not `C` because `C` is the combiner now: the two used to be the
same letter, and the combiner is a property of the SERVICE while a catalog is a property of
one call.

```rust
pub const fn with_row_ceiling(self, row_ceiling: RowCeiling) -> Self
```

Replaces the `top` row ceiling, for a composition root that read one out of its settings -
`github.com/telekom/sutura#777`. `Self::with_spend_ledger`'s reason applies unchanged:
every existing caller of `Self::start` and `Self::start_composed` keeps `RowCeiling::DEFAULT`,
which is `sutura_domain::plan::MAX_ROWS` and the behaviour every deployment already had.

```rust
pub fn with_spend_ledger(self, spend_ledger: SpendLedger) -> Self
```

Replaces the spend ledger, for a composition root that read a configured per-replica
ceiling out of its settings.

A setter rather than a constructor argument, so every existing caller of `Self::start` and
`Self::start_composed` - most of which configure no ceiling at all - keeps its original
argument list. `docs/adr/0030` is the record; `governance.per_replica_spend_ceiling` absent
is the state every one of those callers is already in.

#### Implements

`Debug`, `Surface`

### `fn cause_chain`

```rust
pub fn cause_chain(error: &dyn core::error::Error + 'static) -> Vec<String>
```

Every cause beneath `error`, outermost first.

**A logging concern, and it lives here because this is where the erasure happens.** `Display` on
a `thiserror` enum prints the outermost message only, so a line that formatted the error would
say "the data system did not answer" and drop the driver's own complaint - which is the half that
names the table, the column or the file. This is called by whoever writes the line, not by
whoever builds the error, which is the difference between an error that can be inspected and one
that has already been turned into prose.

### `type_alias ErasedCause`

A typed error, owned, with its type erased and its `#[source]` chain intact.

`Send + Sync` because a transport may answer on a blocking pool, so a failure crosses a thread
boundary on the way back.

## Module `prompt`

The agent-facing system prompt: derived from the tool surface, never written by hand.

# Why this exists at all

An agent that has not been told what this surface is will treat it as a database. It will look
for a field to put SQL in, find none, put a metric name it remembers from somewhere else into
`Query::metric`, get a refusal, read the refusal as a transport
failure, and retry. Every one of those steps is a reasonable thing for a general-purpose agent to
do, and every one of them is the behaviour this repository's types are arranged to prevent. The
types stop the *damage*; they cannot stop the loop. A prompt can.

# What it is derived from, and what that buys

Three inputs, and the first two are not text somebody keeps in step by hand:

1. **The tool list** - `PromptInputs::tools`. The workflow is composed from the operations that
   are actually exposed, so a deployment that does not expose the catalog listing gets a prompt
   that does not tell an agent to call it. A prompt naming an operation that is not there is
   worse than a shorter prompt: the agent spends its turns discovering the absence.
2. **The pinned bundle** - every metric, its grains, its dimensions and the values a filter may
   use, read off the same `PinnedDefinitions` every answer is computed from. It cannot describe
   a metric this deployment does not serve, because there is nowhere for such a metric to come
   from.
3. **The operator's own text** - `PromptInputs::instructions`, appended as the last section.
   Appended and never substituted: see the note on layering below.

# What it deliberately does NOT say

**Nothing about composing SQL.** The reference implementation this is modelled on spends most of
its length teaching an agent to write SQL against model names, to avoid raw database tables, and
to dry-plan a complex statement before running it. None of that transfers, because
`Query` has no field for SQL, a table, a filter expression or a
list of row ids and `deny_unknown_fields` makes an attempt an error naming the field. Repeating
the guidance here would teach an agent to attempt something the surface refuses by construction,
which costs a turn and teaches it the wrong model of what it is talking to. What replaces it is
one short section saying the field does not exist and that there is no way to widen it.

**No column, no table, no model and no measure expression.** This is the same content `GET
/v1/catalog` already returns and deliberately not one field more: a metric's name, its prose,
its grains, its dimensions and their permitted values. A caller needs those to ask a valid
question; it needs no column name to do it, and a column name in an agent's context is a name it
will eventually try to use. The `tests` module below asserts that no model name, table name or
column name from the bundle appears in the output.

**Nothing about identity.** There is none - the deployment token authenticates the deployment and
not the caller - and a prompt that mentioned per-caller scoping would describe a control that does
not exist. `AGENTS.md` and `SECURITY.md` record why.

# Tone

The defaults are deliberately strong, and that is borrowed rather than invented: the reference
implementation measured soft phrasing - "for non-trivial questions", "when useful" - being read
as "skip" almost every time. So the workflow says "every time" and the refusal section says "do
not retry" rather than "consider whether to retry".

# Layering, and why an operator cannot replace the derived part

`PromptInputs::instructions` is appended as the LAST section, under a heading that says it is
the operator's. There is deliberately no way to substitute it for the derived text. The refusal
guidance is the single most load-bearing paragraph in the whole document - an agent that treats a
refusal as an outage retries until something works, which is precisely what the refusal exists to
prevent - and a configuration key that could delete it would be a key whose worst setting is
silent. Last place rather than first is also deliberate: a preamble ahead of the rules reads as
the governing frame, and the governing frame is not the operator's to set.

# Catalog prose is untrusted content

A metric's description is written by whoever authored the catalog, and this repository's threat
model treats catalog content as untrusted - see `SECURITY.md`, and the symlink-bounded walk in
`sutura-catalog-local` for the precedent. A description containing a sentence aimed at the agent
rather than at a human is prompt injection through the catalog.

Three things are done about it, and the first is the honest limit:

* **A delimiter cannot separate instruction from data, because the content can contain the
  delimiter.** `docs/concepts.md` already says so. So the mitigation here is not a fence: it is a
  per-line prefix that WE apply. `quote` puts `> ` at the start of every line of prose, so no
  line of catalog text can reach the output at column zero. It cannot emit a heading, close a
  block, or start what looks like a new section of this document.
* **The trust boundary is named in the text**, immediately above the quoted block, in terms an
  agent can act on: the block is data, a sentence inside it that reads as an instruction is
  content and not an instruction, and encountering one is something to report rather than obey.
* **An operator who does not trust their catalog authors can drop the prose entirely** -
  `CatalogProse::Omitted`. The section then says the descriptions exist and are not included,
  which is a fact an agent can act on, rather than silently rendering a catalog with no meaning
  attached to any metric.

What none of that solves is prose that *persuades* without escaping. No mechanism here can catch
it. What bounds it is that a catalog is reviewed, authored content whose digest moves when a
description changes, and that this text is generated by an operator command rather than pasted
from a caller.

### `enum Tool`

```rust
pub enum Tool
```

One operation a transport exposes.

Three variants: the certified surface's own two operations, and `docs/adr/0013`'s raw tool -
`Surface::run_sql`. The raw tool is not in `Tool::ALL`:
unlike `Catalog` and `Query`, no transport mounts it unconditionally, so a composition root adds
`Tool::RunSql` to the list it passes only when `tools.run_sql.enabled` is true for the deployment
it is rendering for. It is a list rather than a constant because the point is that a caller
passes the subset it actually mounts: `Tool::ALL` is what a transport serving the whole certified
surface passes, and a deployment that mounts only one passes only that one.

**Two certified entries make this cheap insurance rather than a large win, and it is worth saying
so.** The property it buys is narrow: the rendered workflow cannot instruct an agent to call an
operation that is not there. With two operations that is one branch. It is here because the
branch costs a match arm and the alternative - a hand-written workflow that is right until the
day a deployment stops mounting the listing - costs a debugging session. `RunSql` reuses the same
mechanism for the opposite direction: an agent is told about the raw tool only where it can
actually be called.

#### Variants

- `Catalog` - Reading what this deployment defines. `GET /v1/catalog`, `sutura catalog`, and whatever an MCP transport would call it. `Surface::definitions`.
- `Query` - Asking one certified question. `Surface::answer`.
- `RunSql` - Running one literal, ungoverned SQL statement - `docs/adr/0013`'s tool, off by default. `Surface::run_sql`. Present here only when a deployment turned it on; see this type's own documentation for why it is not in `Tool::ALL`.

#### Methods

```rust
pub const fn name(self) -> &'static str
```

The one name this operation answers to.

One word, chosen so that it is simultaneously the CLI subcommand, the path segment under the
version prefix, and what an MCP tool would be called. Three spellings that cannot disagree
beats a table mapping between them.

```rust
pub const fn summary(self) -> &'static str
```

What it does, in one line, for the operations list.

`RunSql`'s wording is `docs/adr/0022`'s framing for this tool, restated for an agent rather
than an operator: ungoverned, runs under the deployment's own role rather than the asking
subject's, and its result carries none of the provenance a `query` answer carries. It never
calls the raw tool's own result "certified" in any form, including a negated one - the word
belongs to the certified path alone, and
`tests::run_sql::the_run_sql_summary_names_the_ungoverned_boundary_and_never_calls_it_certified`
holds the sentence to that.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `Ord`, `PartialEq`, `PartialOrd`

### `enum CatalogProse`

```rust
pub enum CatalogProse
```

Whether the catalog's own prose reaches the agent, and how.

A separate type from the configuration spelling of the same decision, which lives in
`sutura_config::prompt::CatalogProse`. The split is the one `telemetry.filter` already uses: the
configuration crate parses the word an operator wrote, and the crate that does the work owns the
type that does it. It also keeps this crate's dependency table at `sutura-domain`,
`sutura-semantic` and `thiserror`, which is what `AGENTS.md` cites as holding up the rule that a
driving port is not owned by one of its callers.

#### Variants

- `Quoted` - Included, with `> ` at the start of every line and the trust boundary named above it.
- `Omitted` - Left out. The section says the descriptions exist and were not included, which is a fact an agent can act on; silence would leave it guessing at what a metric means.

#### Methods

```rust
pub const fn as_str(self) -> &'static str
```

The operator's own spelling, so a surface carrying the prose can say which way the setting
points and *this deployment ships none* is not *this catalog has none*. Equal to
`sutura_config::CatalogProse::as_str` and held equal by a test in the composition root.

```rust
pub const fn is_quoted(self) -> bool
```

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct PromptInputs`

```rust
pub struct PromptInputs<'a>
```

Everything the prompt is derived from that is not the bundle.

A value rather than four arguments, so a caller that gains a fifth input does not silently get
the wrong one in the third position.

#### Methods

```rust
pub const fn instructions(&self) -> Option<&'a str>
```

```rust
pub const fn new(tools: &'a [Tool], prose: CatalogProse, instructions: Option<&'a str>) -> Self
```

The operations exposed, how catalog prose is treated, and the operator's own text.

`instructions` is already-read text rather than a path, deliberately: this crate performs no
I/O and the composition root is where a configured file that cannot be read has to become a
loud failure. A configured-and-missing file silently omitted would be exactly the failure
`AGENTS.md` warns about in another place - a control that reads as being in place.

```rust
pub const fn prose(&self) -> CatalogProse
```

```rust
pub const fn tools(&self) -> &'a [Tool]
```

#### Implements

`Clone`, `Copy`, `Debug`

### `fn render`

```rust
pub fn render(pinned: &sutura_domain::pinned::PinnedDefinitions, inputs: &PromptInputs<'_>) -> String
```

The whole prompt, as markdown.

Deterministic in its inputs: every collection walked here is a `BTreeMap` or a `BTreeSet`, and
the grains are sorted explicitly. Two calls with the same bundle produce the same bytes, which is
what lets the rendering be pinned by a snapshot rather than described.

### `use guidance`

What a refusal means and what to do about it: `(meaning, remedy)`, in the order the prompt renders
them.

**The accessor a composition root prints from, and it publishes no new prose.** `sutura-cli` used
to hand a person the Rust `Debug` of a governance decision, which names the variant and says
nothing about what to do; the wording it needed was already written twice - here for the
agent-facing prompt, and on the HTTP surface for a client - so this is a third READER of the first
table rather than a third table.

`&'static str` because `GUIDES` owns the wording: nothing here composes a message and nothing
here reads the refusal's own fields. A caller that wants those still has the `RefusalReason` it
passed in.

### Module `refusal`

Everything about a refusal, in one file.

The table, the total match that will not compile when the domain gains a variant, the accessor a
composition root prints from, and the section the prompt renders.

**Its own module because `prompt.rs` was two lines under the thousand-line limit
`cargo xtask max-lines` enforces and cannot exempt**, at the cut `knowledge.rs` already made
once, and the seam is not the line count: this is the one place `RefusalReason` is read. Nothing
else in the prompt looks at a governance decision at all - the rest of the document is derived
from the tool list and the pinned bundle - so the file that holds the refusal wording is the file
that holds every reader of that enum, and a variant added to the domain lands here and nowhere
else.

Why the wording is strong, why an operator cannot replace it, and why a refusal lives inside the
`Ok` at all are argued in `prompt.rs`'s own header.

#### `fn guidance`

```rust
pub const fn guidance(reason: &sutura_domain::query::RefusalReason) -> (&'static str, &'static str)
```

What a refusal means and what to do about it: `(meaning, remedy)`, in the order the prompt renders
them.

**The accessor a composition root prints from, and it publishes no new prose.** `sutura-cli` used
to hand a person the Rust `Debug` of a governance decision, which names the variant and says
nothing about what to do; the wording it needed was already written twice - here for the
agent-facing prompt, and on the HTTP surface for a client - so this is a third READER of the first
table rather than a third table.

`&'static str` because `GUIDES` owns the wording: nothing here composes a message and nothing
here reads the refusal's own fields. A caller that wants those still has the `RefusalReason` it
passed in.

## Module `untrusted`

The shared prompt-injection corpus every surface walks.

`#128`'s two forgeries are the same defect twice: a value that enters a rendering
from outside the transport spells a structural token the transport believes only it
writes. A cell is a string from the data system and a description is prose from a
catalog author - a `\t` cell crosses a column boundary, a `\n` cell crosses a row,
and either can spell the provenance trailer, the exact channel `Provenance` exists
to make trustworthy. On a text surface (the agent tool and the prompt) a value must
therefore be escaped or quoted per line; on a structured surface (JSON) the encoder
owns the boundary and a value must stay one opaque string.

These are the inputs BOTH transports run, so a third transport inherits the tests
rather than the mistake. Each entry is a deliberate escape attempt; each surface's
test walks the list and asserts the property its own encoder provides, so the
corpus does not carry an expected output - only the hostile input.

The honest limit, and it is the whole of what this module does NOT claim:
`docs/agent-prompt.md` says no mechanism catches prose that persuades without
escaping. A value that never escapes can still instruct. What the quoting rejects
is the escape, never the instruction.

### `constant CELLS`

Row cells that try to take over the answer's text half.

The text half is tab-joined columns and newline-joined rows with a provenance
trailer, all of which the fix escapes. Each cell here would, unescaped, either
fabricate structure or forge the identity claim.

### `constant PROSE`

Catalog descriptions that try to reach the agent at column zero.

The descriptions the tool and the prompt render are quoted per line (`> `) or
omitted. Each entry here, unquoted, would open a line an encoder did not write - a
heading, a fence, a bare instruction, or a fake `definitions:` trailer.

## Module `warehouses`

The data systems this process opened, keyed by the name a plan selects them with.

# Why a registry rather than one warehouse

A plan resolves to a single source for one answer - a question spanning two is federated, and
answered where every registered adapter declares `Warehouse::EXECUTES_LEGS` and refused as
`FederationNotExecutable` where one does not (three or more are refused at plan time) - but a
*deployment* holds as many as its catalog names, and until now the service held exactly one. That
made two facts indistinguishable: "this question is for a data system nobody configured" and "this
question is for the other one of the two we opened". The first is a refusal an operator has to fix
and the second is an ordinary question.

So the lookup moves out of the adapter and into a registry keyed by `SourceName`, and two things
follow from that rather than being added:

- `answer` selects the warehouse the *plan* named instead of comparing the plan against the one
  adapter it was handed, so `RefusalReason::SourceUnavailable` now means what its name says: no
  data system is configured under that name.
- the anchor pass runs each metric's anchor against the warehouse for *that metric's* source, so a
  bundle spanning two configured sources verifies rather than reporting every anchor on the second
  one as a source mismatch.

# The limit, and what a caller does about it

**Every entry of ONE `Warehouses<W>` is the same adapter type.** `Warehouse` carries a required
associated constant (`IMPERSONATION`), so it is not object-safe and `dyn Warehouse` is
unavailable - a heterogeneous set has to be a closed enum over the registered adapter types,
decided where that constant is declared. This crate still holds no adapter type, by the same
`[dependencies]` this header always described: `Warehouses::into_mapped` is generic in TWO
adapter types and imports neither, so a composition root builds the concrete registry each
source's own posture check needs and then erases it into whichever closed enum that root
declares over the adapters it linked - one normal edge outward, never one in.

What this shape buys is the whole of what the boot checks need: more than one source configured,
of more than one kind if the caller's own enum covers it, each declaring its own posture, and an
answer that says which posture produced it.

### `struct Warehouses`

```rust
pub struct Warehouses<W>
```

The data systems this process opened.

Keyed by each adapter's own `Warehouse::source` rather than by a name the caller passes
alongside it, so the key and the adapter cannot disagree about which source this is - the same
reason `PinnedDefinitions::pin` computes its digest from the definitions it stores.

#### Methods

```rust
pub fn and(self, another: W) -> Result<Self, SourceAlreadyOpen>
```

A second data system, or a refusal naming the source that was already open.

Consumes and returns, so a registry is built in one expression and there is no half-built state
for something else to read.

```rust
pub fn count(&self) -> usize
```

How many data systems are open. At least one, because `Self::of` is the only way in.

```rust
pub fn each(&self) -> impl Iterator<Item>
```

Every open data system, in source order.

```rust
pub fn executed_on(&self, source: &SourceName) -> Option<ExecutedAs>
```

The execution record for an answer that ran on `source` and nowhere else.

The one place a mono-source answer's provenance comes from, so the posture in an answer is the
posture the adapter that executed it was holding. `None` when nothing is open for that source,
which is the case the caller has already turned into a refusal by the time it asks.

One leg, so one entry: `ExecutedAs` is non-empty by construction and has no `remove`. The
federated path builds its own two-leg record from both adapters - which may name two
different postures since `docs/adr/0040`.

```rust
pub fn get(&self, source: &SourceName) -> Option<&W>
```

The data system a plan naming `source` runs on, if this process opened one.

`Option` rather than a refusal, because who turns an absence into a refusal depends on what is
asking: the query path answers `RefusalReason::SourceUnavailable`, and the anchor pass records
`NotExecutedReason::SourceNotConfigured` against the metric. Deciding here would make one of
those two the other's wording.

```rust
pub fn into_mapped<U, F>(self, wrap: F) -> Warehouses<U>
```

Every adapter, wrapped by `wrap` into a second registry over a second type.

**Adapter-agnostic, and that is the whole reason it belongs here rather than at a
composition root.** A build that links more than one kind erases each source's own adapter
behind a closed enum it declares - `sutura_app::warehouses`'s own header names the enum as
the remedy for the limit this file states - and that enum lives OUTSIDE this crate, one
normal edge away, because "which adapters a process holds is a property of the BUILD". This
method is what lets a root build the concrete registry it already knows how to build (one
call per source, one `deliverable_by` check against that source's own constant) and THEN
erase it, rather than threading the enum through every step that constructs an adapter.

Total rather than fallible: `self`'s keys are already distinct by construction (every entry
passed through `Self::of` or `Self::and`, both of which refuse a collision), and `wrap`
changes no key - so the second registry cannot collide either.

```rust
pub fn merge(self, other: Self) -> Result<Self, SourceAlreadyOpen>
```

Every entry of `other`, added to `self` - `Self::and`'s whole-registry sibling.

A composition root that opened more than one KIND builds one registry per kind (each still
concrete, so `deliverable_by` still checks a real adapter constant) and erases each into the
SAME closed enum before reaching here - this is the step that turns "several registries of
one erased type" into the one registry a heterogeneous build serves.

```rust
pub fn of(one: W) -> Self
```

One data system. The canonical constructor, and the only way a registry comes into existence.

Infallible, because one adapter cannot collide with itself. Every deployment that answers
anything has at least one, so there is deliberately no empty form: a registry with nothing in
it would refuse every question with `SourceUnavailable`, which is a running service that
cannot work - and the composition root refuses that before a listener is bound instead.

```rust
pub fn postures(&self) -> impl Iterator<Item>
```

The posture each open data system was handed, as a startup log reads it.

Read off the adapters rather than off a settings tree, for the reason
`sutura_domain::warehouse::Warehouse::posture` gives: a summary derived from configuration
reports what was configured rather than what was built.

#### Implements

`Clone`, `Debug`

### `struct SourceAlreadyOpen`

```rust
pub struct SourceAlreadyOpen
```

Two adapters were registered for one source.

Its own type rather than a silent overwrite, because whichever adapter lost would then be the one
nobody opened and nothing would say so. It is not the same failure as a duplicate *alias* in the
settings tree - that one is refused before any adapter is built - it is a composition root that
built two adapters naming one source.

#### Methods

```rust
pub const fn at(&self) -> &SourceName
```

Which source was registered twice.

Named `at` rather than `source` for the reason `PostureNotDeliverable::at` gives: `thiserror`'s
derive gives this type an `Error::source`, and `clippy::same_name_method` is denied.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

## Module `assemble`

The metadata assembler: N catalog contributions become one pinned bundle.

This is the *"metadata sources compose"* half of `docs/adr/0011` and of #115's step 4.
`sutura_domain::pinned::SemanticCatalog::load` reads one source; a deployment may declare
several. The assembler is where they stop being several:

- **Application code in this crate, not an adapter over adapters** - ADR 0011's three reasons
  are recorded on the `assemble` function, and this crate is the one that owns the two driving
  ports.
- **One source may provide a given kind for a given entity.** Two sources defining one metric is
  refused, always, and refused naming both sources - "guessing which wins is how a metric
  silently means something different after a configuration change", which is the exact sentence
  ADR 0011 refuses.
- **The declaration-fidelity check runs per contributor, not per bundle.** `checked_against`
  on the merged result would say nothing once two sources are merged - a narrow source's
  undeclared kind could be hidden by what another source produced. Each contributor is held to
  its own declaration against its own content.
- **The contribution manifest is built from the contributors' own records.** Each source
  `PinnedDefinitions` carries a one-entry manifest naming itself and its declared capabilities;
  the composed manifest is those entries, keyed by name, so the digest of the composed bundle
  covers the composition.

One limit is stated here because it decides what the wave-one deployment looks like: a metric
may reference only a model its own source also declares, because each contribution arrives
already assembled. The cross-source-reference case - the literal "`DataHub`'s model, metrics
certified here" - lands with a raw-content port, which is a separate decision recorded in
`docs/adr/0011`.

### `enum CompositionError`

```rust
pub enum CompositionError
```

Why N contributions will not compose.

Every collision variant names both sources and the entity, which is ADR 0011's *"refuses the
load, naming both and the entity"* - a precedence rule nobody stated is a precedence rule nobody
reviewed, so the refusal is what carries the names.

#### Variants

- `Empty` - Nothing was contributed; a deployment serves at least one metadata source.
- `Manifest` - The composed contributions are not a manifest: nothing to record, or two of them naming one source. Distinct from `CompositionError::Empty`, which is this function's own check on its input - this one is the manifest refusing to record a composition it cannot represent. Only `NoContributors` is unreachable from here: `CompositionError::Empty` above wins first on any input that would otherwise produce it. `DuplicateSource` IS reachable through this public function - `assemble` runs no source-name uniqueness check of its own, only the per-element checks below - and is exercised directly by a test on this function, not only on `ContributionManifest::parse`. What keeps a duplicated name off the *served* path is `Catalogs::parse` in `sutura-config` refusing it at configuration time, before assembly runs.
- `NotASingleContribution` - A contribution's own manifest did not name exactly one source, so this bundle cannot say who contributed it. The `count` is what a reader needs: the manifest is supposed to be the per-source record, and a value that failed to be one has nothing to merge under.
- `VersionMismatch` - Two contributors certify different snapshots. A bundle is one version, and `docs/adr/0011`'s amendment records the decision: two sources certified at different times is the "answers that differ across a refresh boundary" shape, refused rather than papered over.
- `MetricCollision` - The one interpretation has no precedence, declared or otherwise: two definitions of one number is the failure this system exists to prevent.
- `ElementCollision` - Any other element two sources supply: a model, a relationship, a glossary term, a caveat, an absence or a worked example. `kind` is the closed vocabulary the message renders and the other fields are typed. It is not a `CompositionError::MetricCollision` because the rule for metrics is the stronger one - no precedence at all - while other elements could in principle be titled, and neither is today.
- `Unfaithful` - A contributor's declaration disagrees with its own content. Per contributor, not per bundle: on the merged result this check would say nothing once two sources are merged.
- `Definitions` - The composed definitions do not hold together - most often a metric naming a model some contributor should have provided and none did, and the domain's own refusal names it.
- `Knowledge` - The composed knowledge does not hold together against the composed definitions.
- `Digest` - The composed content could not be pinned.

#### Implements

`Debug`, `Display`, `Error`

### `enum ElementKind`

```rust
pub enum ElementKind
```

The six kinds of catalog element two sources may collide over.

`CompositionError::ElementCollision`'s `kind` field used to be a `&'static str`: a free-text
field on a variant a caller matches by name is a contradiction, because nothing stopped a sixth
call site from spelling one of the five existing kinds differently, or a seventh call site from
naming a kind `check_no_element_collisions` does not actually check. Neither `DefinitionKind`
nor `Capability` in `sutura_domain` fits: the first does not distinguish a model from a
relationship (both are `Structure`), and the second has no variant for either. A small closed
enum local to this composition step is what the issue's "existing typed vocabulary or a small
closed enum if needed" resolves to here.

**The limit, next to the claim.** Closure - a seventh call site naming a kind this type has no
variant for is a compile error - is held by the compiler, for all six variants. WHICH kind a
given call site in `check_no_element_collisions` names is pinned by a test for two of them,
`Model` and `Relationship`; the other four (`GlossaryTerm`, `Caveat`, `Absence`, `WorkedExample`)
have no cell of their own, so a call site there naming the wrong (but still valid) variant is
caught by nothing but review.

#### Variants

- `Model`
- `Relationship`
- `GlossaryTerm`
- `Caveat`
- `Absence`
- `WorkedExample`

#### Implements

`Clone`, `Copy`, `Debug`, `Display`, `Eq`, `PartialEq`

### `fn assemble`

```rust
pub fn assemble(bundles: &[sutura_domain::pinned::PinnedDefinitions]) -> Result<sutura_domain::pinned::PinnedDefinitions, CompositionError>
```

Composes N contributions into one bundle, refusing a composition ADR 0011 says cannot exist.

**Application code, not an adapter over adapters - the shape ADR 0011 picked, for three
reasons.** The rules being decided here are DOMAIN rules, not one adapter's; `load` stays free
of a request context in either shape, so that property does not choose between them; and an
assembling *adapter* implements the port over N others and would eventually depend on every one
of them, which is the crate-graph shape this avoids.

# Errors

`CompositionError`, for any of: empty input, a contribution whose own manifest names more than
one source, two contributions certifying different versions, two sources providing the same
element, a contributor whose content disagrees with its declaration, or definitions/knowledge
that do not assemble once merged.

Takes a slice, not an owned `Vec`: every `Contributor` below borrows its bundle's
`Definitions`/`Knowledge` rather than cloning them, so this function never needs to own a bundle
to begin with - a caller that already has a `Vec` passes `&bundles`.

## Module `capability`

What this surface can be asked to do, named once for every transport that offers it.

# Why the vocabulary is here rather than in a transport

`crate::surface::Surface` has three operations - read the pinned bundle, answer one governed
question, and (`docs/adr/0013`, off by default) run one raw statement - and those three *are* the
tool set. A transport renames them for its own protocol: the agent surface calls them tools and
the HTTP surface calls them routes. Neither owns the set.

That is not a preference. `sutura-mcp` and `sutura-http` cannot see each other - *an adapter never
calls another adapter* - so a set owned by one of them is a set the other has to reach through it,
which is the same argument that moved the driving port itself out of `sutura-http`. One source in
the crate that declares the port is the only shape in which two transports **cannot** disagree
about what this deployment offers, and `docs/implementation-plan.md`'s
`both_transports_describe_the_same_tools` is the property that needs it.

**What is shared is the SET, the identifier and the scope. Not the prose.** Each transport writes
its own description, because they are written for different readers - a model deciding whether to
call a tool, and a person reading an interface description - and `AGENTS.md` already draws that
line for the two refusal vocabularies: *"Nothing compares the two sentences, and nothing should."*

# What a scope gates, stated before anything reads one

**A scope decides which capabilities a caller may use. It decides nothing about which rows a
question reaches.** Every capability reads the same pinned bundle and every question executes
under the same identity, because no source executes as the asking subject: `docs/adr/0014`'s leg 1
establishes *who is asking* and leg 2 does not exist. So a deployment that grants one caller
`sutura:catalog.read` and not `sutura:metrics.ask` has narrowed what that caller may *do*, and has
not narrowed what any answer would contain.

And within that: **filtering advertisement is presentation, and the control is at invocation.** A
caller that names a capability it was not granted is refused whether or not it was ever told the
capability exists. Both halves are built - see the row `.agents/skills/sutura/invariants` gained with this
module - and if they ever disagree it is the refusal that is the control.

# A scope names a capability and never a metric

`docs/adr/0014`'s *What is not decided* left this open with a leaning: *"A scope naming a metric
couples the authorization server to the catalog, and a scope naming a capability does not. The
second is almost certainly right and it is not yet argued."* This module takes the second, and the
argument is that a catalog edit must not be able to change what a token means. A scope naming
`revenue` would put the authorization server's vocabulary under the catalog's version, so adding a
metric would silently grant it to every token holding a wildcard and renaming one would revoke a
grant nobody edited - an authorization change made by a definition author, in a repository the
authorization server does not read. The scopes here are two fixed strings no catalog can move.

`Capability::scope` is therefore part of the deployed contract rather than an implementation
detail: an authorization server is configured with those literals by hand. A test pins them by
value for that reason.

### `enum Capability`

```rust
pub enum Capability
```

One thing this surface can be asked to do.

Closed, and closed on purpose: a third capability is a third exhaustive match to satisfy - the
walk in `Capability::next`, the scope in `Capability::scope` and the identifier in
`Capability::id` - plus whatever each transport's own match needs. There is no wildcard arm in
any of them, so a variant added here does not compile until every one of those has been answered.

**Declaration order is `Ord`, and it runs from the least to the most a caller can get out of this
deployment**: describing what is measured comes before asking for a number. A
`BTreeSet<Capability>` therefore iterates in that order, which is what makes a rendered tool list
deterministic.

#### Variants

- `DescribeCatalog` - Read the pinned bundle: which metrics exist, at which grains, with which dimensions and which filter values.

  `crate::surface::Surface::definitions`. Descriptive content only - the catalog port takes no
  request context and cannot be given one - but a listing of what a deployment measures is
  business information even with no row of data in it, which is why it is a capability at all
  rather than something public.
- `AskMetric` - Answer one governed question about one certified metric.

  `crate::surface::Surface::answer`.
- `RunSql` - Run one literal SQL statement against the configured source, off by default and refused where the deployment cannot execute it as the asking subject or is not declared single-user.

  `crate::surface::Surface::run_sql`. `docs/adr/0013` is the record: its result carries no
  `sutura_domain::pinned::Provenance` and no field a definition digest could occupy, so a raw
  answer cannot be rendered as a certified one.

#### Methods

```rust
pub fn every() -> impl Iterator<Item>
```

Every capability this surface has, in declaration order.

Built from `FIRST` and `Capability::next` rather than written out as an array, so there is
no second list to keep in step with the enum.

```rust
pub const fn id(self) -> &'static str
```

The stable identifier both transports name this capability by.

The agent surface uses it as the tool name; the HTTP surface uses it as the operation
identifier in the generated interface description. **It is part of the deployed contract**, so
it is a fixed literal here rather than derived from the variant name: a `Debug` rendering would
rename a client's tool the day somebody renamed a variant.

```rust
pub const fn scope(self) -> &'static str
```

The scope that licenses this capability.

A fixed literal, stated independently of `Capability::id` rather than derived from it. That
is the whole point: deriving one from the other would mean renaming a tool silently renamed a
scope, and every authorization server configured with the old one would stop granting anything
- an authorization change made by a rename.

Prefixed, so a token minted for another resource server that happens to carry `catalog.read`
does not read as a grant here. The audience check is what actually keeps such a token out -
`docs/adr/0014` Decision 2 - and this is defence in depth rather than the control.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `Hash`, `Ord`, `PartialEq`, `PartialOrd`

### `struct Permitted`

```rust
pub struct Permitted
```

What one caller may do on this surface.

**Two named constructors and no third way in**, because the two cases are the two honest answers to
*who is asking* and a reader has to name which one a deployment is in:

* `Permitted::every_capability` - nothing established a caller identity, so there is no verified
  claim to narrow by. A single-player deployment, and the agent surface over standard input and
  output.
* `Permitted::granted_by` - a verified token's scopes decide, and **only** they do.

# It fails closed, and the consequence is named rather than softened

A verified caller whose token names no capability scope is permitted **nothing**: it sees no tools
and every call is refused. That follows from `AGENTS.md`'s *fail closed on the query path* and from
`docs/implementation-plan.md`'s own *"a caller without a scope cannot see the tool it lacks"*, and
it means a deployment that switches `security.inbound` on without authoring scopes at its
authorization server has switched every caller off.

**What keeps that discoverable rather than mysterious is the refusal**: the HTTP surface answers
`403` with `code: insufficient_scope` and a sentence naming the exact scope string to grant, so an
operator reads the fix out of the response rather than out of this comment.

# Why this takes strings rather than a parsed scope type

`sutura_http::inbound::Scopes` is where a scope claim is parsed and bounded, and it stays there.
Moving it would be taking a decision `docs/adr/0014`'s closing section explicitly reserves - *"how
[the agent surface] is reached at all, and then which crate the validator moves to ... is an
architecture decision, not a refactor"* - and nothing here needs the parse: this compares against
two fixed literals, and a string that could not be a scope simply matches neither.

#### Methods

```rust
pub fn advertised(&self) -> impl Iterator<Item> + '_
```

What to advertise, in declaration order.

Presentation. A capability absent from here is one `Permitted::includes` also refuses, which
is what keeps the two from being able to disagree: both read the same set.

```rust
pub fn count(&self) -> usize
```

How many capabilities are permitted. For a startup or per-request log line.

The count and not the names, for the same reason `sutura_http::inbound::Scopes` puts a count on
a log line: the names are a caller's authorization detail and they multiply a log's
cardinality.

```rust
pub fn every_capability() -> Self
```

Every capability this surface has.

**What a deployment that establishes no caller identity permits, and the name says so.** It is
not a bypass and not a default: it is the correct answer when there is no verified claim to
narrow by, and a filter over an unverified claim is the thing `docs/implementation-plan.md`
calls *worse than no filter - it looks like a control*.

Two callers today: `sutura_http`'s capability layer, for a deployment with no
`security.inbound` block, and `sutura_mcp::serve_stdio`, where the process boundary is the
boundary and there is no header a token could arrive in.

```rust
pub fn granted_by<'scope>(scopes: impl IntoIterator<Item>) -> Self
```

Exactly the capabilities these scopes name.

**The one comparison, so there is not one per transport.** A scope this surface does not know
is ignored rather than refused: a token is minted by an authorization server that may serve
other resources too, and refusing a caller for holding an unrelated grant would be refusing
them for somebody else's configuration.

```rust
pub fn includes(&self, capability: Capability) -> bool
```

Whether this caller may use one capability.

**This is the control.** `Permitted::advertised` decides what a caller is shown; this decides
what it may do, and a transport calls it on every invocation whether or not it filtered the
advertisement.

```rust
pub fn without(self, capability: Capability) -> Self
```

Removes one capability, whatever granted it.

**A deployment-level narrowing, and deliberately independent of a caller's own scopes.** A
tool that is off for this DEPLOYMENT - `docs/adr/0013`'s off-by-default raw SQL tool is the
first one - has to be absent for every caller including one presenting every scope this
surface knows, and including the no-authentication single-player case
`Self::every_capability` answers. Applying this after either constructor is what makes "a
tool this deployment never turned on" and "a tool this caller was not granted" two different
reasons a caller sees the same absence for, without `Permitted` itself growing a second
notion of what a scope is.

#### Implements

`Clone`, `Debug`, `Eq`, `PartialEq`

## Module `asked`

Everything one call was established to be, on either transport - one value, not two.

# Why one value rather than two independently-inserted ones

`crate::surface::Surface::answer` needs *who is asking* (`RequestContext`) to reach
`sutura_domain::identity::CredentialBroker::mint`, and it needs *what this caller may invoke*
(`Permitted`) to reach the capability gate. Both are established by the same verification, at the
same instant, from the same header - so a transport that inserted them as two separate values
would have two places for a request in flight to carry caller A's context beside caller B's
grant, if the two insertions were ever reordered or one forgotten. One value with one constructor
makes that pairing a type rather than a convention two call sites happen to keep straight.

# A caller may not state its own identity

No `Deserialize`, for the same reason `sutura_http::inbound::VerifiedCaller` and
`sutura_domain::identity::RequestContext` have none: there is no code that could turn
caller-supplied bytes into one of these.

```compile_fail
// A transport that tried to read one off the wire does not compile.
let asked: sutura_app::Asked = serde_json::from_str(r#"{"subject":"someone"}"#).expect("no");
drop(asked);
```

The compiling twin, so the failure above cannot be passing for a typo - what a caller of
`Asked::established` can do with one is read the two halves back:

```
use sutura_app::{Asked, Permitted};
use sutura_domain::identity::{PrincipalChain, RequestContext, Subject};

let asked = Asked::established(
    RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself)),
    Permitted::every_capability(),
);
assert!(asked.permitted().includes(sutura_app::Capability::DescribeCatalog));
```

### `struct Asked`

```rust
pub struct Asked
```

Everything one call was established to be: who is asking, and what it may invoke.

**One constructor, `Asked::established`, and it takes the two halves already produced by a
verification - it performs no verification of its own.** `sutura_http::capability::establish_asked`
is the one place that calls it today, handing over what leg 1 already derived - a
`VerifiedCaller`'s chain and scopes, or the deployment's own when there is none. The agent
surface's own call arrives with PR2 of `telekom/sutura#378`; until then this type adds no third
way to decide either half.

`Clone` because a transport may need to hand the same value to a blocking-pool closure that
outlives the request extension it was read from - `sutura_runtime::spawn_carrying_span` is the
reason `RequestContext` is already `Clone`, and `Permitted` clones a `BTreeSet` of at most three
elements today.

#### Methods

```rust
pub const fn context(&self) -> &RequestContext
```

Who this call is attributed to, and the credential it presented, if any.

```rust
pub const fn established(context: RequestContext, permitted: Permitted) -> Self
```

The only way to one of these: hand over what a transport's own verification already produced.

```rust
pub const fn permitted(&self) -> &Permitted
```

What this caller may invoke.

#### Implements

`Clone`, `Debug`

## Module `preflight`

Asking every open data system whether it holds the tables the bundle names.

**The decision sequence, once, for every composition root that has one** - and it is here rather
than copied into each because review measured the copy: each helper underneath was
byte-identical between `sutura-serve` and `sutura-cli`, and none of them contains a word an
operator reads. `models_by_table` is a pure query over `PinnedDefinitions`, which is a
`sutura-domain` type, and `AbsentBehind`'s rendering is a list of names rather than a sentence.

**What is NOT here is the sentence and the sink**, and that is the seam rather than an omission.
A root has to say what an operator should do about each outcome, in the words that fit its own
transport, through the sink that transport actually delivers on - `tracing` behind a subscriber
for a server, standard error for a process launched on a pipe with no subscriber installed. So
`ask` returns one `Verdict` per source and prints nothing, which also makes each root's
rendering a pure function its own suite can assert on. Before this the soft outcome was printed
from inside the decision and no test could see it - so the mechanism the argument for that sink
rests on was held by review, which `AGENTS.md` does not accept as held.

`sutura_domain::warehouse::Warehouse::preflight` is the port, and its own documentation carries
why an answered inventory and *could not verify* are different outcomes.

**The limit, stated with the claim:** what a pre-flight establishes is that a table EXISTS. Not
that the columns a model names are on it, and not that a question's identity may read it - a
listing grant and a read grant are two grants. An anchor covers both, for the metrics that have
one.

### `enum Verdict`

```rust
pub enum Verdict<E>
```

What one data system answered about the tables one bundle names in it.

A root treats two failures differently, and an incomplete or unreadable inventory is not a
finding about the catalog. The port answers five things and fails in one way,
and that one failure splits on `Warehouse::preflight_was_refused`: a data system that REFUSED to
be listed will refuse identically on every launch and the fix is one grant, while one that could
not be reached is a condition that passes. A root that collapsed them would either stop a
deployment that would have worked or hide the check being off in the deployment least likely to
read a startup log.

**`Self::Unaccounted` is a REFUSAL, not a failure to get an answer.** The
data system answered; its answer did not account for its own inventory. Reading that as
`Self::Absent` is what `telekom/sutura#275` is - a shortfall rounded down to zero and charged
to the catalog - and reading it as `Self::Unverified` would be worse still: that is the warning
half, so the one shape the cross-check exists to catch would end in a deployment that serves.

Generic in the adapter's error so the cause travels: nothing here can read `W::Error`, and the
root that composed the adapter is the one that can flatten it.

#### Variants

- `Present` - Asked, and every table is there. Carries how many, for a line that says so.
- `NotReported` - The adapter did not report - `TablesPresent::NotAsked`, the port's default.

  **Not readable as verified**, which is the property the port's own answer type exists to
  keep: a root that printed nothing here would make the one outcome meaning *nothing checked
  this* indistinguishable from a data system that really looked.
- `Absent` - Asked, and these tables are not there. A refusal, and the models to name in it.
- `UnreadableInventory` - An unreadable inventory established neither presence nor absence for these tables. A refusal without a count or model names: no catalog declaration was shown wrong.
- `Unaccounted` - Asked, answered, and the answer did not account for every table the data system said it holds - so these tables are neither established present nor established absent.

  **It names tables and not the models behind them, which is the one place this verdict
  deliberately says less than `Self::Absent`.** A model is what an operator opens to fix a
  `table:` that is wrong, and nothing here says a `table:` is wrong: the catalog may be
  entirely right and the data system's own answer incomplete. Naming models would send an
  operator to exactly the file `telekom/sutura#275` is about them being sent to wrongly.
- `Refused` - The data system refused to be asked: this identity may not list it.
- `Unverified` - The data system could not be asked, for a reason that is not a refusal.

#### Methods

```rust
pub fn boot_policy(self) -> BootPolicy<E>
```

Splits this verdict the one way both composition roots split it.

Exhaustive over `Verdict`, so a variant added to the port's answer is a compile error
here - at the one place that has to decide which side of the boot policy it falls on -
rather than a silently-served outcome in whichever root forgot it.

# Errors

The four outcomes this deployment does not start on: a refused listing, a table the data
system does not hold, an unreadable inventory, and an inventory that did not account for
itself.

#### Implements

`Debug`

### `struct AbsentBehind`

```rust
pub struct AbsentBehind
```

The tables a data system does not hold, each with the models that named it.

**Keyed by the TABLE and carrying the models, because that is the direction a refusal reads in:**
the data system answered about a table, and the operator has to open a model to fix it. Two
models over one table is ordinary - a bundle may declare several over one fact table - so the
value is a set, and `Display` renders every one of them.

Non-empty by construction: it is built only from a `TablesPresent::AllBut`, whose own newtype
refuses an empty set, so a refusal that names nothing is unrepresentable rather than checked.

#### Methods

```rust
pub fn is_empty(&self) -> bool
```

Always `false`, and it exists because `clippy::len_without_is_empty` asks for it.

The type is non-empty by construction, so this is a constant with a name rather than a
question worth asking - which is itself the honest reading of the invariant.

```rust
pub fn len(&self) -> usize
```

How many tables are absent.

```rust
pub const fn named(&self) -> &BTreeMap<QualifiedTable, BTreeSet<ModelName>>
```

The absent tables and the models behind each, for a root that renders its own shape.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `PartialEq`

### `enum Refusal`

```rust
pub enum Refusal<E>
```

The boot policy over one `Verdict`: this deployment refuses, or it carries on and says so.

**The partition is the thing that was held by recall in two composition roots.** Each root
matched all seven verdicts and decided per arm which ones return an error, and the two agreed
only because someone kept them agreeing - so a source whose outcome one root refused and the
other served was a two-file edit away. Stating it once makes the two roots' boot behaviour the
same fact rather than the same intention.

**`Err` for a refusal here, and that is not the query path's rule inverted.** A governance
refusal lives inside the `Ok` where a CALLER could mistake it for a hiccup and retry; this is
boot, the outcome is that the process does not start, and both roots already answered `Err` for
exactly these four outcomes, carrying a rendered sentence. What changed is that the four are now
a type rather than prose a caller would have had to parse.

**The limit, stated with the claim.** This is a type saying which outcomes refuse. It does not
confine a root to asking: `Verdict` is still public, because *what the data system answered*
and *what this deployment does about it* are two questions, and the port's own answer is what an
adapter's suite asserts on. A future root that matches `Verdict` directly and re-decides the
split is what review has to catch; no type here stops it.

Which outcome belongs on which side is `Verdict`'s own documentation, and changing it is
`telekom/sutura#141`'s decision rather than a call site's.

#### Variants

- `Refused` - The data system refused to be listed: this identity may not ask.
- `Absent` - Asked, answered, and these tables are not there - with the models that named them.
- `UnreadableInventory` - An unreadable inventory established neither presence nor absence for these tables.
- `Unaccounted` - The answer did not account for every table the data system said it holds.

#### Implements

`Debug`

### `enum Notice`

```rust
pub enum Notice<E>
```

The boot policy's other side: this deployment serves, and a root says what was established.

**Every one of these is a line a root emits, including the two clean ones.** `NotReported` is
the outcome meaning *nothing verified this*, so a root that printed nothing for it would make it
indistinguishable from a data system that really looked - which is why silence is not one of the
shapes here.

#### Variants

- `Present` - Asked, and every table is there. Carries how many, for a line that says so.
- `NotReported` - The adapter did not report - `TablesPresent::NotAsked`, the port's default.
- `Unverified` - The data system could not be asked, for a reason that is not a refusal.

#### Implements

`Debug`

### `struct Asked`

```rust
pub struct Asked<'source, E>
```

One data system's name and what it answered.

The name is BORROWED from the registry rather than cloned: the registry outlives the answer at
every call site, and a clone here would be one taken to satisfy a signature rather than to own
anything.

#### Methods

```rust
pub fn into_verdict(self) -> Verdict<E>
```

The answer, owned, for a root that has to move the cause out of it.

```rust
pub const fn source(&self) -> &'source SourceName
```

Which data system answered.

```rust
pub const fn verdict(&self) -> &Verdict<E>
```

What it answered.

#### Implements

`Debug`

### `struct TablesChanged`

```rust
pub struct TablesChanged
```

The bundle being served names tables that are not the ones attached behind it.

**A type rather than the `String` both roots built**, and the two sets rather than a rendered
sentence: a caller that wants to act on which tables moved can read them, and the sentence is
`Display` for the roots that only want to print it. Both roots printed the SAME
sentence - measured byte-identical - so it is not wording that belongs to a transport, and it
moved with the comparison instead of being copied a third time.

Non-empty by construction: `refuse_unattached` is the only constructor and returns `Ok` when
both sets are empty, so a mismatch that names nothing is unrepresentable rather than checked.

**The limit, stated with the claim.** What this compares is TABLE NAMES between two loads of one
catalog directory. It does not establish that a table which is attached holds the columns a
model names, and it says nothing about a data system a root never attached anything for - the
pre-flight above is that half, for the sources that can answer it.

#### Methods

```rust
pub const fn extra(&self) -> &BTreeSet<TableName>
```

Tables attached for a model the bundle being served no longer names.

```rust
pub const fn missing(&self) -> &BTreeSet<TableName>
```

Tables the bundle being served names with no table attached behind them.

#### Implements

`Clone`, `Debug`, `Display`, `Eq`, `Error`, `PartialEq`

### `fn ask`

```rust
pub fn ask<'engines, W>(pinned: &sutura_domain::pinned::PinnedDefinitions, engines: &'engines crate::warehouses::Warehouses<W>) -> Vec<Asked<'engines, <W as >::Error>>
```

Asks each open data system once whether it holds the tables the bundle names in it.

**One call per data system, and per DATASET underneath, never per model** - which is what makes
this affordable at startup: the tables go over as a set, so a bundle of forty models on one
dataset costs one metadata read. An adapter reading more than one dataset makes one call per
dataset, which is still a set rather than a model.

A data system the bundle names no model in is skipped rather than asked about nothing: it would
otherwise cost a round trip to be told about an empty set, and the port's own empty-set arm
answers `NotAsked`, which a root would then have to explain.

**It prints nothing and refuses nothing.** Both are the caller's, for the reason this module's
own documentation gives: the words and the sink belong to the transport, and a decision that
printed would not be assertable.

The order is the registry's, which is the source name's - so two roots asking the same question
report it in the same order, and a test can name the answer it expects rather than search for it.

### `fn served_tables`

```rust
pub fn served_tables(served: &sutura_domain::pinned::PinnedDefinitions) -> std::collections::BTreeSet<sutura_domain::model::TableName>
```

Every table the bundle's models sit behind, whichever data system holds it.

A pure query over a `sutura-domain` type, here rather than in each composition root for
`models_by_table`'s reason: it names nothing an operator reads. Both roots compared the result
against what their engine attached, from a body that was byte-identical in the two of them.

### `fn refuse_unattached`

```rust
pub fn refuse_unattached(serving: &std::collections::BTreeSet<sutura_domain::model::TableName>, attached: &std::collections::BTreeSet<sutura_domain::model::TableName>) -> Result<(), TablesChanged>
```

The tables the bundle being served names, against the tables the engine actually holds.

**One home for a comparison both composition roots made from byte-identical bodies.** The two
sets come from two `load()` calls on the same catalog directory; a model added between them is
refused here rather than served with no table behind it, which would fail the first question
against it at query time.

Two sets rather than a bundle and an engine, so the comparison is unit-testable without a digest,
a knowledge declaration or a data system - `served_tables` is the other half and is one map
over a public accessor.

Both directions are refused, and the second is not pedantry: a table attached for a model the
served bundle no longer names means the catalog directory changed between two loads seconds
apart, and whatever else moved with it is the part nobody has looked at.

# Errors

Either set holding a table the other does not, as a `TablesChanged` carrying both differences.

### `type_alias BootPolicy`

The two sides of the boot policy: a notice this deployment serves with, or a refusal it stops on.

A named alias because `clippy::type_complexity` refuses the bare `Result` at this arity, and the
name is the better half of that trade rather than a suppression: the split IS the decision, so a
signature that says *boot policy* reads as the thing being returned and not as two halves a
caller has to recombine. It stays a `Result` so `?` in a composition root keeps working.

## Module `spend`

The per-replica spend counter: in-process, windowed, keyed by the asking subject.

`docs/adr/0030-where-a-budget-lives.md` decides every shape here; this module is the mechanism.
Three decisions worth restating because a reader of the code alone could miss them:

**Fixed windows, not sliding ones.** A subject's spend resets to zero the first time this
ledger is consulted after the window has elapsed, rather than decaying continuously. Simpler to
reason about - "how much has this subject spent since their window started" needs one `Instant`
and one running total, not a queue of timestamped charges to prune - and the cost a sliding
window would avoid (a subject who spends right at a boundary can spend up to twice the ceiling
across the seam) is not a cost `docs/adr/0030` asked this record to close: the record's own
scope is a per-replica counter that resets on restart in addition to its own window, so a seam
effect inside one window is not the precision this shape is buying.

**Keyed on `Subject`, never the whole `PrincipalChain`.**
An agent acting for a subject spends that subject's own budget - see the ADR for the argument and
its cost.

**`None` configured is unlimited, not zero.** `SpendLedger::no_budget` is what every deployment
ran before this existed, and it is a real state a caller may still choose rather than a value
nothing constructs.

### `struct SpendBudget`

```rust
pub struct SpendBudget
```

A byte ceiling and the window it resets on, already validated.

A plain pair rather than a re-export of `sutura_config::SpendBudget`: this crate depends on
nothing outside `sutura-domain`, `sutura-semantic` and `thiserror` - see this crate's own module
documentation - and a composition root reads the validated ceiling and window out of its
settings and hands the two primitives here, the same shape `working_set_bytes: u64` already
uses for `RuntimeSettings::working_set`.

#### Methods

```rust
pub const fn new(ceiling_bytes: u64, window: Duration) -> Self
```

**A raw constructor, not a parse - the zero fence lives one crate over.** This type takes
whatever `ceiling_bytes` and `window` it is given, `Duration::ZERO` included: under a zero
window every charge resets immediately, and a priced question over the ceiling mints
`reset_after == Duration::ZERO` again. The only production caller is
`sutura_config::SpendBudget::parse` (`crates/sutura-config/src/governance.rs`), which
refuses a zero window (and a zero ceiling) before this constructor ever sees one - this
type's own doc comment above states why this crate cannot depend on that one, so the fence
sits there rather than here.

#### Implements

`Clone`, `Copy`, `Debug`, `Eq`, `PartialEq`

### `struct SpendLedger`

```rust
pub struct SpendLedger
```

The counter: one instance per process, consulted by every question this replica answers.

**`&self`, not `&mut self`** - one ledger is shared by every request without a lock in this
type's own signature, the same shape `sutura_domain::audit::AuditSink::record` uses for the same
reason. The mutable state is inside a `Mutex` guarding the per-subject map.

#### Methods

```rust
pub fn headroom_bytes(&self, now: Instant) -> Option<u64>
```

The tightest remaining headroom across every subject this ledger is currently tracking, or
`None` where no ceiling is configured (`docs/adr/0030`'s "absent means no budget").

**Deployment-wide, never per-subject** - ADR-0015 Decision 5 types every metric label
parameter as `&'static str` precisely so request-owned text (a `Subject`'s own identifier
included) cannot become one, so this reports the worst case across every subject rather than
naming which one it is. A subject not yet in the map, or whose window has elapsed, has its
full ceiling as headroom - so an empty or fully-expired map reports the ceiling itself, never
`None` and never zero, the same "absent rather than zero" discipline `sutura-http`'s own
metrics module already applies to a gauge with no meaningful value.

```rust
pub fn new(budget: Option<SpendBudget>) -> Self
```

A ledger bounded by `budget`, or unbounded if `None`.

```rust
pub fn no_budget() -> Self
```

No ceiling configured. Every question is admitted and nothing is counted - `docs/adr/0030`'s
"absent means no budget, which is today's behaviour" read back as a constructor.

```rust
pub fn spent_bytes_total(&self) -> Option<u64>
```

Every byte this replica has admitted since it started, or `None` where no ceiling is
configured - the same "absent rather than zero" `Self::headroom_bytes` already applies.

**Monotonic, and that is the whole reason this exists beside a gauge that already reports
headroom.** `sutura_spend_headroom_bytes` (`#884`) cannot be summed across replicas: it
resets to the full ceiling on every restart, and `sum()` over N replicas is `N * ceiling -
total_spend`, a number that moves whenever N does. This total only grows, so
`sum(rate(sutura_spend_bytes_total[5m]))` is correct across a restart (a monitoring
system's counter-reset handling) and across a changing replica count - `docs/adr/0030`'s
owner decision, 2026-09-18: aggregation belongs to the monitoring system, never to
enforcement, which stays per-replica either way.

**Exported as `sutura_spend_bytes_total` by both transports.** `sutura_http::ServiceState::new`
(`sutura-http`) registers the counter beside the `sutura_spend_headroom_bytes` gauge,
`POST /v1/query`'s post-answer push and the agent surface's `Serving` wrapper raise it to
this reading through one `SpendHeadroomPush` handle, and the `/metrics` scrape renders it.
**Registered only where a ceiling is configured:** `Some` here is the registration
condition, so an unconfigured deployment exports neither series - absent rather than zero,
the same discipline the gauge applies.
