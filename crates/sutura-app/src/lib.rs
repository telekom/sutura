#![forbid(unsafe_code)]
//! The service: what happens between a question arriving and an answer leaving.
//!
//! Generic over the ports and holding no framework types, so it can be exercised against a fake
//! warehouse in a test and a real one in production without either of them knowing. The composition
//! root decides which; this crate never names an adapter.
//!
//! Two entry points, and the order between them is the point:
//!
//! [`verify_and_validate`] re-executes every metric that declares a certified number, against the
//! `Warehouse` it is handed, and hands back the bundle as a [`Validated`] one only if every anchor
//! reproduced its number. [`answer`] takes nothing else. So **a bundle whose anchors were never
//! checked cannot reach the query path** - not by discipline, and not because a caller was asked to
//! call the two in order: [`Validated`] has no other constructor, and the one it has takes a
//! warehouse and calls it.
//!
//! That is the correction to what this crate used to claim. The proof used to be
//! `sutura_domain::pinned::Validated::new(pinned, &report)`, and `AnchorReport::new`,
//! `AnchorReport::record` and `AnchorCheck::Matched` are all public - so any caller could enumerate
//! the bundle's anchors, record `Matched` for each without opening a data system, and get a bundle
//! the service would serve. The golden suite did exactly that. The wrapper attested to the caller's
//! own assertion and read like proof, which is worse than no wrapper. [`verify_anchors`] survives
//! because a report is worth rendering to an operator; what it cannot do any more is mint the proof.
//!
//! [`surface`] is those same two entry points with the ports' generic parameters erased, for a
//! transport whose request handler is a concrete function. It is a *driving* port and it lives here
//! rather than in a transport crate, which is a correction: it used to be `sutura-http`'s, and a
//! second transport would have had to depend on the first to reach it. Nothing in that module names
//! a framework type, so this crate still holds none.

use std::time::Instant;

use sutura_domain::identity::{
    Agreed, Attribution, BoundToTheRequest, CredentialBroker, CredentialsDoNotFitTheRequest, Expiry,
    PresentedDisagreesWithPosture, RequestContext, SourceSet, Subject,
};
use sutura_domain::pinned::PinnedDefinitions;
use sutura_domain::pinned::view::ScopedView;
use sutura_domain::plan::{Executable, FederationCombiner, RowCeiling};
use sutura_domain::query::{Query, RefusalReason, ResultBound, ToolOutcome};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{PreFlight, UnreadableCell, Warehouse};
use sutura_semantic::{CompileFailure, Compiled, compile};

pub(crate) use crate::bounds::{exceeds_response_bound, exceeds_row_cap};
use crate::federated::answer_federated;
pub use crate::warehouses::{SourceAlreadyOpen, Warehouses};

// The two "too much data" checks `answer` and `federated::answer_federated` both apply to a result
// AFTER it executes. Its own file for `cargo xtask max-lines`'s cap, not for thematic tidiness - the
// same reason `federated` is its own file.
mod bounds;

// The application-facing interface a transport consumes, with the ports' generics erased: a
// DRIVING port and its one implementor. The argument for it being here rather than in `sutura-http`
// is the module's own documentation - a plain comment here rather than a doc comment, because
// rustdoc resolves the links in a merged module doc against the file the `mod` line is in, and
// every name in that argument lives in the other file.
pub mod surface;

// The agent-facing system prompt, derived from the tool surface and the pinned bundle. Here for the
// same reason `sources` and `grains_coarsest_first` below are: a shape the application can offer
// for free, that every one of its callers needs, belongs with the application rather than with one
// transport. It adds no dependency to this crate's manifest, which is what keeps `cargo tree -p
// sutura-app -e normal` at `sutura-domain`, `sutura-semantic` and `thiserror` - the fact `AGENTS.md`
// cites as holding up the rule that a driving port is not owned by one of its callers.
pub mod prompt;

// The shared prompt-injection corpus both transports walk - the hostile cells and the hostile
// catalog prose `#128`'s forgeries are built from, so a third surface inherits the tests rather than
// the mistake. Here rather than in either transport because `sutura-mcp` and `sutura-http` cannot
// see each other, and both already depend on this crate.
pub mod untrusted;

// The data systems this process opened, keyed by the name a plan selects them with. Here rather than
// in a composition root because the LOOKUP is application logic - which warehouse answers a plan, and
// what an absence means - while which adapters exist is the root's.
pub mod warehouses;

// The metadata assembler: how N catalog contributions become one bundle. Application code, over the
// `SemanticCatalog` port - ADR 0011's *"the assembler is application code in `sutura-app`, not an
// adapter over adapters"* - which is why it lives here rather than in an adapter or a composition
// root.
pub mod assemble;

// What this surface can be asked to do, and what one caller may do of it. Here for the same reason
// `surface` is: the tool set IS the driving port's operation set, `sutura-mcp` and `sutura-http`
// cannot see each other, and a set owned by one transport is a set the other has to reach through
// it. The module's own documentation carries the argument and the limits.
pub mod capability;

// The one value both transports read: who is asking, and what it may invoke, as the single pair a
// verification produces. Here rather than in either transport for `capability`'s own reason - the
// two cannot see each other - applied one step on: this is the PAIRING of that module's `Permitted`
// with a `RequestContext`, and a pairing owned by one transport is a pairing the other has to reach
// through it.
pub mod asked;

// Asking every open data system whether it holds the tables the bundle names, once, for both
// composition roots that ask it - and comparing the bundle being served against what was actually
// attached behind it. Here for `warehouses`' reason applied one step on: the DECISION is
// application logic - which data systems to ask, what a set of absent tables means, and which of
// two failures is a refusal - while the sentence an operator reads and the sink it goes to belong
// to the root, which is why nothing in this module prints. Review measured the alternative every
// time something moved in: each helper underneath was byte-identical in the two roots first.
pub mod preflight;

// The per-replica spend counter: `docs/adr/0030` decides the key, the window and the refusal;
// this module is the ledger `answer` and `answer_federated` consult after a dry run prices a plan
// and before anything executes. Here rather than in `sutura-domain` because it is mutable,
// in-process state shared across every question this replica answers - a resource this crate
// already owns one of, in `warehouses::Warehouses`, though that one has no lock because it is built
// once and never mutated after boot.
pub mod spend;

mod proof;

pub use crate::asked::Asked;
pub use crate::capability::{Capability, Permitted};
pub use crate::proof::{Validated, verify_and_validate};
use crate::spend::Charge;
pub use crate::spend::{SpendBudget, SpendLedger};

/// Why the service could not produce an outcome.
///
/// Neither variant is a refusal. A refusal is something the caller asked for and may not have; these
/// are the data system being unreachable and our own bundle or generator being wrong, and offering
/// either as a refusal would invite a caller to retry a different question forever.
///
/// Generic in the warehouse error rather than boxing it, so the adapter that failed keeps its own
/// typed error all the way out. A `Box<dyn Error>` here would be the same loss of information the
/// boundary gate bans `anyhow` for, arrived at by a different route.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError<E, M, C> {
    /// The pinned bundle would not compile this question, or the splitter built a two-source plan
    /// this workspace could not then assemble.
    ///
    /// **Both are our own side being wrong, which is what keeps them out of a refusal.**
    /// `telekom/sutura#338` is the second one's report: it used to arrive as
    /// [`RefusalReason::FederationNotExecutable`], which is what a build whose adapter type does not
    /// declare `Warehouse::EXECUTES_LEGS` is told, so a wiring defect was indistinguishable from a
    /// build that cannot run a leg. `sutura_semantic::CompileFailure` keeps them apart and keeps the
    /// typed cause.
    #[error("the question could not be compiled")]
    Compile {
        #[source]
        cause: CompileFailure,
    },
    #[error("the data system did not answer")]
    Warehouse {
        #[source]
        cause: E,
    },
    /// The federated combiner could not assemble the two legs' results.
    ///
    /// **Typed in the COMBINER's own error, which is what the port being a port buys.** `docs/adr/0039`
    /// step 3 moved the combine into an adapter, so the cause is that implementor's type exactly as
    /// [`Self::Warehouse`]'s is a data adapter's - and `sutura-app` still names no engine.
    ///
    /// **An internal defect rather than a refusal, and only because the two governance outcomes are
    /// taken off it FIRST.** `answer_federated` asks
    /// [`FederationCombiner::working_set_exhausted`](sutura_domain::plan::FederationCombiner::working_set_exhausted)
    /// and then
    /// [`answer_not_well_formed`](sutura_domain::plan::FederationCombiner::answer_not_well_formed),
    /// so what reaches here is a leg result that does not carry a label the plan named, or a plan
    /// that would not build - this workspace's own wiring, which no caller caused and none can fix.
    #[error("the combined answer could not be assembled")]
    Combine {
        #[source]
        cause: C,
    },
    /// The federated path's own wiring produced a shape it cannot answer for.
    ///
    /// Every arm is unreachable through the one production splitter - see [`FederationMiswired`].
    #[error("the federated answer path is mis-wired")]
    Miswired {
        #[source]
        cause: FederationMiswired,
    },
    /// The credential broker could not mint. Nothing about the question was wrong.
    ///
    /// **Its own variant rather than a refusal, and its own variant rather than sharing the one
    /// above.** A refusal would let a client library retry a governance decision until something
    /// works, which is what `sutura_domain::query::ToolOutcome` exists to prevent. And sharing
    /// `Warehouse` would collapse two causes a caller has to act on differently: `docs/adr/0014`
    /// makes the point that a caller told "unavailable, retry" against an authorization-server
    /// outage will retry successfully, while one told the same against a bound that fires again
    /// retries forever.
    #[error("the credential broker did not answer")]
    Broker {
        #[source]
        cause: M,
    },
    /// The broker's answer does not agree with the request it was made for.
    ///
    /// **A wiring defect between the broker and the request, so an `Err` and not a refusal** - the
    /// question was fine. Four things can be wrong and the domain's own enum names them: the grant
    /// was minted for a different subject, it covers a different set of sources, its deadline had
    /// already passed, or the refusal named a source nobody asked about.
    ///
    /// **It used to carry one of them**, a bare "nothing was granted for this source", and the other
    /// three were not checked at all. Widening the cause rather than adding three variants is the
    /// shape of the fix: they are one question asked once, and a transport that had to tell them
    /// apart would be a transport making a judgement about our own wiring.
    ///
    /// Executing anyway is the alternative this variant exists to remove, and it is the one that
    /// would have run the leg as somebody other than the asker.
    /// The leg the broker minted disagrees with the posture the adapter was opened with.
    ///
    /// **Its own variant because the two values come from different places, and that is the whole of
    /// what the comparison is worth.** The broker read the settings tree; the registry holds what the
    /// composition root opened. A leg that says "the deployment's own identity" against a source
    /// declared `impersonation-at-source` means one of those two is wrong about this deployment, and
    /// executing anyway is the case that is silent: provenance is read off the ADAPTER's posture, so
    /// the answer would have been reported as impersonated while it ran as the process.
    ///
    /// Both shipped adapters make this comparison too, and this variant does not replace theirs - an
    /// adapter is the last thing before a driver and may not assume who called it. What it replaces is
    /// the assumption that every FUTURE adapter will remember to.
    #[error("the credential does not agree with the posture the data system was opened with")]
    Posture {
        #[source]
        cause: PresentedDisagreesWithPosture,
    },
    #[error("the credentials that came back do not fit this request")]
    Credentials {
        #[source]
        cause: CredentialsDoNotFitTheRequest,
    },
    /// A result came back as Arrow and one of its columns could not become a domain value.
    ///
    /// **This variant is where the Arrow port's decode moved to, not a new failure mode.**
    /// `docs/adr/0039` step 2 puts the one Arrow-to-[`Value`](sutura_domain::warehouse::Value)
    /// decode in the interior and step 2's second half moves the CALL to the presentation edge, so
    /// the failure that used to arrive wrapped in an adapter's own error - `BigQueryError::
    /// Unreadable`, the engine's `DataFusionError::Unreadable` - arrives here instead, for every
    /// adapter at once.
    ///
    /// **An internal failure rather than a refusal, and that is today's classification kept rather
    /// than chosen afresh:** both adapters mapped it into their own error type, which reaches a
    /// transport as [`Self::Warehouse`] does. What it means is that a data system returned a column
    /// of a type this workspace does not map, or a value no domain cell can hold - a non-finite
    /// double, a day number that is not a date. No caller caused it and narrowing the question does
    /// not avoid it, which is why it is not a refusal a caller is told to act on.
    #[error("a result column could not be read")]
    Unreadable {
        #[source]
        cause: UnreadableCell,
    },
}

/// The federated path's own wiring, in the three shapes it cannot answer for.
///
/// **Every arm is unreachable through `sutura_semantic::plan::federated_plan`, the one production
/// splitter, and each is typed anyway rather than assumed away.** A previous revision reached for
/// a `DuplicateLabels` combine failure for the first of them - a failure about a leg RESULT, minted
/// from a value that has nothing to do with one - which is the shape this enum replaces.
#[derive(Debug, thiserror::Error)]
pub enum FederationMiswired {
    /// The two legs named one data system, so the provenance record could not hold both.
    ///
    /// `FederatedPlan::new` refuses same-source legs, so this is a splitter invariant that changed.
    #[error("both legs of the federated answer name `{at}`, so one provenance record cannot hold both")]
    LegsCollide {
        /// `at` rather than `source`, because `thiserror` reads a field called `source` as the
        /// `Error::source` chain - `SourceAlreadyOpen::at`'s own reason.
        at: sutura_domain::model::SourceName,
    },
    /// The two leg results did not resolve to one of each side.
    ///
    /// `FederatedPlan::legs` returns the fact leg and the lookup leg by construction, so a pair
    /// built from it is one of each.
    #[error("the two leg results are not one fact leg and one lookup leg")]
    LegsAreNotOneOfEach {
        #[source]
        cause: sutura_domain::plan::LegsAreNotOneOfEach,
    },
    /// Ranking a combined answer produced a row set whose rows contradict its own columns.
    ///
    /// `FederatedPlan::rank` re-orders and truncates rows it was handed, so it cannot change a
    /// width.
    #[error("the ranked answer is not rectangular")]
    RankedAnswer {
        #[source]
        cause: sutura_domain::warehouse::MalformedRowSet,
    },
}

/// Now, in whole seconds since the Unix epoch, for the one comparison this crate makes.
///
/// **The clock is read HERE and not in the domain**, which is the split
/// `sutura_domain::identity::Expiry::passed_by` documents from the other side: the interior owns the
/// direction of the comparison and reads no clock, and this crate - an application layer over the
/// ports, not the hexagon's interior - is where the instant comes from. `sutura_http` already reads
/// the same clock the same way for a proof's `exp`.
///
/// **No parameter, deliberately.** An instant a caller passed in is a value a caller can get wrong -
/// stale, or the credential's own deadline handed back to itself - and the guard it feeds exists
/// because a value that arrived from elsewhere was trusted. What that costs is that a test cannot
/// advance it, which is why the two cases the suite pins are the two no clock can change: a deadline
/// of zero is in the past for every clock there has ever been, and `NothingExpires` is in the past
/// for none.
///
/// **`u64::MAX` on a clock that will not read, and the direction is the point.** `duration_since`
/// fails only for a clock before 1970; making *now* the largest instant there is makes every deadline
/// look passed, so an expiring credential is refused rather than presented. A machine whose clock says
/// 1969 should not be executing anything as somebody else. Nothing that carries `Expiry::NothingExpires`
/// is affected, which is every credential the shipping broker mints.
pub(crate) fn now_in_unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(u64::MAX, |since| since.as_secs())
}

/// What answering produced, or why it could not.
///
/// A named alias because the inline form is over the complexity threshold in `clippy.toml`, and
/// naming it is the better half of that trade: the generic parameter is a warehouse, not a result.
pub type Answering<W, B, C> =
    Result<Answered, ServiceError<<W as Warehouse>::Error, <B as CredentialBroker>::Error, <C as FederationCombiner>::Error>>;

/// One call's result: what the caller is told, and what it ran under.
///
/// **Two values rather than one, and the second one never reaches the caller.** The outcome is the
/// answer or the refusal, and it goes back through the transport. The deadline is the `Expiry` the
/// credentials this call executed with carried, and it goes to the audit sink - `docs/adr/0008` fixes
/// the record's content as the chain, the outcome, the posture per leg **and the expiry the
/// credentials carried**, and until this type existed there was no way for the last of those to reach
/// [`surface::LocalService`], which is what writes the record.
///
/// **Why not on the outcome.** `sutura_domain::pinned::Provenance` rides to the caller, so putting a
/// credential's lifetime there would publish, on both wire surfaces, how long this deployment's
/// credential for a data system is good for. That is the deployment's business rather than the
/// asker's, and a widened wire shape is a worse place to learn it.
///
/// `None` means nothing was minted for this call: the question was declined by compilation or by the
/// source lookup, both of which run before the broker is asked - which `answer`'s own suite pins.
#[derive(Debug)]
pub struct Answered {
    outcome: ToolOutcome,
    executed_until: Option<Expiry>,
}

impl Answered {
    /// An outcome decided before any credential was minted.
    const fn declined_before_minting(outcome: ToolOutcome) -> Self {
        Self {
            outcome,
            executed_until: None,
        }
    }

    /// An outcome decided with a checked grant in hand.
    ///
    /// Takes the grant rather than the deadline, so a call site cannot pass one credential's outcome
    /// with another's deadline: the value is read off the thing that was used.
    const fn under(credentials: &BoundToTheRequest, outcome: ToolOutcome) -> Self {
        Self {
            outcome,
            executed_until: Some(credentials.not_after()),
        }
    }

    /// What the caller is told.
    #[inline]
    #[must_use]
    pub const fn outcome(&self) -> &ToolOutcome {
        &self.outcome
    }

    /// What the caller is told, owned, for a transport that is about to render it.
    #[inline]
    #[must_use]
    pub fn into_outcome(self) -> ToolOutcome {
        self.outcome
    }

    /// How long the credential this call ran under was good for. `None` if none was minted.
    #[inline]
    #[must_use]
    pub const fn executed_until(&self) -> Option<Expiry> {
        self.executed_until
    }
}

/// Answers one question, or says why it will not.
///
/// The source lookup is not a formality. A plan names exactly one data system, and running it against
/// a different one would answer a question about other data under the same provenance. So the plan
/// SELECTS its warehouse out of the registry, and a plan naming a source this process did not open is
/// a refusal rather than an error: it is a governance outcome, and `SourceUnavailable` now says what
/// its name says - nothing is configured under that name.
///
/// **The registry is what made that refusal honest.** Under one warehouse the check compared the
/// plan's source against the single adapter's own, so "nobody configured this data system" and "this
/// is the other one of the two we opened" were the same refusal.
///
/// # Nothing here executes without a credential somebody minted
///
/// `context` says who is asking - established by the transport, never stated by the caller - and
/// `broker` is what turns that into what each leg presents. The credential is minted **once, for
/// every source the plan reads**, which is one source today and is the shape a federated answer
/// needs: one asker and one deadline for N legs, rather than N mintings that could disagree.
/// `docs/adr/0008` is the decision and `sutura_domain::identity::LegCredentials` is where the
/// argument lives.
///
/// The order is deliberate: mint **before** the pre-flight and before execution. A pre-flight asked
/// as the wrong identity answers a different question, and a subject with no credential at that
/// source is refused before this deployment has asked the data system anything on their behalf.
///
/// # And what comes back is checked against what was asked
///
/// A broker is an adapter outside the hexagon, so its answer is input. `Minted::agreeing_with` is the
/// one guard: the grant's subject must be the subject this request arrived under, it must cover
/// exactly the sources this plan reads, its deadline must not have passed, and a refusal must name a
/// source that was actually asked about. Any disagreement is a `ServiceError::Credentials` - our own
/// wiring, an internal failure on the wire - and never a refusal, because a refusal is a statement
/// about the caller's access and none of these is one.
///
/// **Three separate findings, one guard, and that is a decision rather than a shortcut.** Each of the
/// three could have been a check of its own next to the value it protects. Three checks are three
/// places the fourth case gets forgotten, and they were all the same question. What makes the single
/// guard un-skippable rather than merely conventional is on the domain side:
/// `sutura_domain::identity::BoundToTheRequest` is the only type that hands out a `Presented`, and
/// `agreeing_with` is the only thing that builds one.
pub fn answer<W, B, C>(
    definitions: &Validated<PinnedDefinitions>,
    query: &Query,
    context: &RequestContext,
    broker: &B,
    warehouses: &Warehouses<W>,
    combiner: &C,
    working_set_bytes: u64,
    deadline: Deadline,
    ledger: &SpendLedger,
    row_ceiling: RowCeiling,
) -> Answering<W, B, C>
where
    W: Warehouse + Sync,
    W::Error: Send,
    B: CredentialBroker,
    B::Error: Send,
    C: FederationCombiner,
    C::Error: Send,
{
    let pinned = definitions.get();
    let view = scoped_for(pinned, context);
    let compiled = compile(query, &view, row_ceiling).map_err(|cause| ServiceError::Compile { cause })?;
    // The PLAN is what the port takes now, not a rendered statement: an adapter that executes
    // without generating SQL is a first-class implementation of it. A SQL-speaking adapter renders
    // the plan itself, for its own dialect.
    let plan = match compiled {
        Compiled::Refused { reason } => return Ok(Answered::declined_before_minting(ToolOutcome::Refusal { reason })),
        Compiled::Federated { plan } => {
            return answer_federated(
                pinned,
                &plan,
                context,
                broker,
                warehouses,
                combiner,
                working_set_bytes,
                deadline,
                ledger,
                row_ceiling,
            );
        }
        Compiled::Planned { plan } => plan,
    };
    let (Some(warehouse), Some(executed_as)) = (warehouses.get(plan.source()), warehouses.executed_on(plan.source())) else {
        return Ok(Answered::declined_before_minting(ToolOutcome::Refusal {
            reason: RefusalReason::SourceUnavailable {
                source: plan.source().clone(),
            },
        }));
    };
    // The set is a LOCAL, and that is load-bearing rather than tidy: it is what the broker is asked
    // about AND what its answer is checked against one line later. Written inline in the `mint` call,
    // as it was, the second use had nothing to compare with and the check below could not exist.
    let requested = SourceSet::of(plan.source().clone());
    // The credential, minted once for every source this answer reads. A refusal comes back in the
    // `Ok` and leaves as one: "this subject has no credential at that source" is a governance
    // outcome, and a broker that could not be reached is an `Err` - see `ServiceError::Broker`.
    let minted = broker
        .mint(context, &requested)
        .map_err(|cause| ServiceError::Broker { cause })?;
    // **THE GUARD, and there is one of it.** A review of this path found three ways a broker's
    // answer was acted on without being compared with the request it was made for: a grant minted
    // for another subject, a deadline nothing read, and a refusal naming a source nobody asked
    // about. `Minted::agreeing_with` asks the one question all three are - does this answer agree
    // with this request - and its `Ok` is the only value in the workspace that yields a `Presented`
    // out of a grant, so the comparison cannot be skipped by reading a leg out directly.
    //
    // A disagreement is an `Err` and never a refusal, for every arm. A refusal says "you have no
    // credential there", which a caller may act on; a broker contradicting the request says nothing
    // about the caller at all - it is this deployment being wrong, and offering it as a refusal would
    // both mislead the asker and invite a client library to retry a wiring defect forever.
    let credentials = match minted
        .agreeing_with(context.chain().subject(), &requested, now_in_unix_seconds())
        .map_err(|cause| ServiceError::Credentials { cause })?
    {
        Agreed::Refused { source } => {
            return Ok(Answered::declined_before_minting(ToolOutcome::Refusal {
                reason: RefusalReason::CredentialUnavailable { source },
            }));
        }
        Agreed::Granted { credentials } => credentials,
    };
    let presented = credentials
        .presented_for(plan.source())
        .map_err(|cause| ServiceError::Credentials { cause })?;
    // **The leg against the posture the adapter was OPENED with, here rather than in each adapter.**
    // Both shipped adapters make this comparison themselves, and a review pointed out what that is
    // worth: `Warehouse` is a trait, so an implementor can simply omit it - and this crate's own fake
    // did, which meant an `impersonation-at-source` adapter handed the deployment's own identity
    // executed, and provenance then reported the leg as impersonated because provenance is read off
    // the adapter's posture. Made here, the rule holds for every adapter this registry can hold,
    // including the next one; left per-adapter it is a convention a security review has to notice.
    //
    // The two values are still independent, which is the whole point of comparing them: the broker
    // read the settings tree and the registry holds what the composition root opened. The adapters
    // keep their own copy of the check - it is their last line before a driver, and an adapter may not
    // assume who called it.
    presented
        .agrees_with(warehouse.posture(), plan.source())
        .map_err(|cause| ServiceError::Posture { cause })?;
    // Prepared before it is run, WHERE THAT IS CHEAPER THAN RUNNING IT. For an adapter across a
    // network it is: a statement that would be rejected is rejected before any data is read, which
    // is the difference between a failed query and a partial one. For the in-process engine it is
    // not - checking builds the logical plan and runs the analyzer and the optimizer, and then
    // execution does all of it again, so the guarantee was bought at the price of two full planning
    // passes per question. `Warehouse::dry_run` is defaulted for that reason: an adapter that cannot
    // make checking cheaper answers this by doing nothing, and says so by not implementing it.
    //
    // The pre-flight's own answer is deliberately not read here. `PreFlight::Accepted` is the data
    // system's opinion at pre-flight time, not an authorization decision, and skipping a check
    // downstream on the strength of it is exactly what that type's documentation warns against.
    // What this call is for is the error it can return.
    // **The refusal a pre-flight can carry, and the reason it is kept apart from the size bounds.**
    // The pre-flight's error is an authorization decision as often as `execute`'s is: a data
    // system that will not run the statement as this identity may refuse while it prepares, before
    // any data is read - which is the whole point of checking first. That refusal has to reach the
    // caller as `SourceRefused`, not as the retryable `ServiceError::Warehouse` a dead data system
    // produces - a caller told to retry an authorization decision is told to retry something that
    // refuses again in the same place. `source_refused` is the same predicate `execute` asks below
    // (see that arm), and it is the same class: a statement refused at the identity/authorization
    // level, whichever call surfaced it. `working_set_exhausted` and `result_did_not_fit` are
    // deliberately not asked here - the port's contract is that a check reads no data, so there is
    // no reservation and no reply for either bound to refuse.
    // The deadline, checked before a call is made and never re-derived: `docs/adr/0029` is the
    // record. A budget already spent here means the pre-flight is refused before the data system is
    // asked at all - the same shape `still_usable_at` below gives the credential's own expiry.
    if deadline.remaining_at(Instant::now()).is_none() {
        return Ok(Answered::under(
            &credentials,
            ToolOutcome::Refusal {
                reason: deadline_exceeded(deadline),
            },
        ));
    }
    let preflight = match warehouse.dry_run(Executable::Query(&plan), presented, deadline) {
        Ok(preflight) => preflight,
        Err(cause) => {
            if warehouse.deadline_exceeded(&cause) {
                return Ok(Answered::under(
                    &credentials,
                    ToolOutcome::Refusal {
                        reason: deadline_exceeded(deadline),
                    },
                ));
            }
            if warehouse.source_refused(&cause) {
                return Ok(Answered::under(
                    &credentials,
                    ToolOutcome::Refusal {
                        reason: RefusalReason::SourceRefused {
                            source: warehouse.source().clone(),
                        },
                    },
                ));
            }
            return Err(ServiceError::Warehouse { cause });
        }
    };
    // The spend ledger, consulted with the dry run's own price and nobody else's - an adapter that
    // did not price charges nothing, "not counted" rather than "free", so this only ever refuses
    // for the one adapter that prices today (BigQuery). See `budget_exhausted`'s own doc.
    if let Some(reason) = budget_exhausted(ledger, context, preflight) {
        return Ok(Answered::under(&credentials, ToolOutcome::Refusal { reason }));
    }
    // **The deadline again, and this is the call that can fire in production.** The check above runs
    // microseconds after the broker minted, so what it catches is a broker minting something already
    // dead. This one runs after a pre-flight, which against a networked data system is a round trip -
    // so a credential with seconds left when it was minted may have none by the time the statement
    // would run. One comparison, two call sites, both at a boundary the credential crosses.
    //
    // The clock is read again rather than reused: reusing the first reading would make this arm a
    // second copy of the first answer, which is a check that cannot fail.
    //
    // **The limit, and it is what an adapter would have to close:** this is the last point on this
    // side. `Warehouse::execute` hands the adapter a `Deadline`, but never the credential's expiry:
    // `Presented` is material, a principal name, or an acknowledgement witness, no validity at all.
    // So an adapter cannot make the before-leg check `docs/adr/0008` part 4 describes, and a
    // credential that ages out during execution is refused by the data system rather than here.
    credentials
        .still_usable_at(now_in_unix_seconds())
        .map_err(|cause| ServiceError::Credentials { cause })?;
    // The time budget, re-checked for the pre-flight round trip's exact reason: a dry run against a
    // networked data system spends part of it, so a budget with time left when this function began
    // may have none by the time execution would start.
    if deadline.remaining_at(Instant::now()).is_none() {
        return Ok(Answered::under(
            &credentials,
            ToolOutcome::Refusal {
                reason: deadline_exceeded(deadline),
            },
        ));
    }
    // The working-set ceiling, on its way out as a refusal rather than as an error. Exhaustion is a
    // governance outcome - the question is well formed and this deployment will not spend more than
    // a configured number of bytes on it - and it used to leave here as `ServiceError::Warehouse`,
    // which the transport answers `503`. That is what a data system being down looks like, so a
    // caller was told to retry against a bound that fires again in the same place.
    //
    // The adapter is asked rather than inspected: `Self::Error` is its own type and nothing here can
    // read it, which is why `working_set_exhausted` is on the port. An adapter with no bounded pool
    // answers `None` by default and this arm never runs for it.
    //
    // `dry_run` above is deliberately not given the same treatment: the port's contract is that a
    // check reads no data, so there is no reservation for a ceiling to refuse.
    //
    // **The order between the two predicates is a decision, and it is `working_set_exhausted` first.**
    // An error that satisfies BOTH is reported as `ResourcesExhausted` (422), not `ResultTooLarge`
    // (413). Exhaustion is the more fundamental bound - a reservation refused is the process saying it
    // will not spend the memory, which no narrower question avoids - and a caller told to narrow a
    // question over a ceiling it cannot satisfy has been told to do the impossible. The order is not
    // the default's to decide: an adapter maps one failure into both predicates on its own, and which
    // refusal a caller sees must not be line order nobody wrote down. Ask the adapters in a real tree
    // whether a single error could genuinely satisfy both; until one does, the order is pinned here by
    // a both-predicate fake and the comment at `sutura_app::tests`.
    let rows = match warehouse.execute(Executable::Query(&plan), presented, deadline) {
        Ok(rows) => rows,
        Err(cause) => {
            if let Some(ceiling_bytes) = warehouse.working_set_exhausted(&cause) {
                return Ok(Answered::under(
                    &credentials,
                    ToolOutcome::Refusal {
                        reason: RefusalReason::ResourcesExhausted { ceiling_bytes },
                    },
                ));
            }
            // The SAME defect one bound further out, and the arm sits here rather than beside the row
            // cap below because there are no rows to count: the data system declined to hand the
            // result over at all. A networked endpoint caps a reply by size, and a result INSIDE the
            // row cap can be over that - a wide result rather than a tall one. It used to leave as
            // `ServiceError::Warehouse` too, so the caller was told `503` and invited to retry
            // against a bound that returns the same reply.
            //
            // One refusal for both bounds, because *too much data* is one answer to a caller and the
            // remedy is the same narrowing. `ResultBound` is what keeps that from being a lie about
            // which bound fired - and the volume arm carries no number, because the bound is the data
            // system's and this deployment was not told it.
            if warehouse.result_did_not_fit(&cause) {
                return Ok(Answered::under(
                    &credentials,
                    ToolOutcome::Refusal {
                        reason: RefusalReason::ResultTooLarge {
                            bound: ResultBound::Volume,
                        },
                    },
                ));
            }
            // The deadline, after the two size bounds and before the identity refusal - the order
            // `docs/adr/0029` states. An adapter's own failure IS the stopped question here, unlike
            // the two checks above this function makes on its own: this one only ever answers what
            // the adapter reports - Postgres now stops on the deadline itself (`SET LOCAL
            // statement_timeout`); the engine and BigQuery still do not.
            if warehouse.deadline_exceeded(&cause) {
                return Ok(Answered::under(
                    &credentials,
                    ToolOutcome::Refusal {
                        reason: deadline_exceeded(deadline),
                    },
                ));
            }
            // The same guard one more step out: the DATA SYSTEM refused the statement because the
            // identity it ran it as may not ask it - an authorization decision, not a transient
            // outage. It used to leave as `ServiceError::Warehouse` and reach a caller as `503`,
            // the status a dead data system produces, so a caller was told to retry an
            // authorization decision that refuses again at the same place. `preflight_was_refused`
            // is the same split asked of the boot path; this predicate is it asked of `execute`.
            if warehouse.source_refused(&cause) {
                return Ok(Answered::under(
                    &credentials,
                    ToolOutcome::Refusal {
                        reason: RefusalReason::SourceRefused {
                            source: warehouse.source().clone(),
                        },
                    },
                ));
            }
            // Anything else is a failure rather than a refusal, and the typed cause travels with it.
            return Err(ServiceError::Warehouse { cause });
        }
    };
    // **The presentation edge, and `docs/adr/0039` step 2 put it here.** The port hands back Arrow;
    // this is the one call that turns it into the rows a caller reads, and it is above every adapter
    // rather than inside each one. The two bounds below count rows and bytes, so they read the
    // decoded set - an Arrow row count would not see the width a caller's cells add up to.
    let rows = rows.to_rows().map_err(|cause| ServiceError::Unreadable { cause })?;
    // The row cap, enforced rather than merely requested. The plan asked for one row more than
    // `plan.max_rows()`, so more than that many coming back means the result was cut short - and a
    // truncated result is a wrong total under a certified name, with provenance attached and nothing
    // saying it is partial. Refused, because "this question is too wide to certify" is an answer the
    // caller can act on and a silent partial one is not.
    if exceeds_row_cap(rows.rows().len(), plan.max_rows()) {
        return Ok(Answered::under(
            &credentials,
            ToolOutcome::Refusal {
                reason: RefusalReason::ResultTooLarge {
                    bound: ResultBound::Rows { limit: plan.max_rows() },
                },
            },
        ));
    }
    // One measurement further out than the row cap, and it is why this check cannot replace that
    // one: a result inside `plan.max_rows()` can still be wide - `MAX_DIMENSIONS` grouped columns
    // of text a data system returns, which no type here bounds the length of - which the row cap
    // cannot see because it counts rows and not the bytes a caller's own cells add up to. Checked
    // here, still inside the closure `sutura_runtime::spawn_carrying_span` already moved onto the
    // blocking pool for `warehouse.execute` above, so this costs no second offload.
    if let Some(limit_bytes) = exceeds_response_bound(&rows) {
        return Ok(Answered::under(
            &credentials,
            ToolOutcome::Refusal {
                reason: RefusalReason::ResultTooLarge {
                    bound: ResultBound::Encoded { limit_bytes },
                },
            },
        ));
    }
    // The posture travels with the answer, read off the adapter that just executed rather than off a
    // settings tree - `executed_as` was taken from the registry above, beside the warehouse this
    // question actually ran on. A field derived from configuration would report what was configured
    // rather than what ran, and the two disagreeing is the case the field exists for.
    Ok(Answered::under(
        &credentials,
        ToolOutcome::Answer {
            provenance: pinned.provenance(executed_as),
            rows,
        },
    ))
}

/// The refusal for a deadline that ran out, naming the budget it was opened with.
///
/// One function so `answer`, `dry_run_leg` and [`crate::federated::run_leg`]
/// build the same reason the same way, whether the cause was a spent budget caught before a call
/// or an adapter's own failure
/// [`Warehouse::deadline_exceeded`](sutura_domain::warehouse::Warehouse::deadline_exceeded)
/// recognised.
pub(crate) const fn deadline_exceeded(deadline: Deadline) -> RefusalReason {
    RefusalReason::DeadlineExceeded {
        budget_seconds: deadline.budget().seconds(),
    }
}

// `docs/adr/0013`'s raw SQL tool - carved out because this file hit the thousand-line limit.
//
// **Private, not `pub`.** `xtask check-boundaries`'s answer-path gate holds `run_sql` at ONE
// spelling - the re-export below - by guarding it at the crate root only; a `pub mod raw` would
// give every caller a second, ungated spelling (`sutura_app::raw::run_sql`) the gate's classifier
// cannot see (`#703` review, finding 1: it compiled clean and the gate printed `ok`). Every item
// this module needs to expose is re-exported here, so nothing outside this crate loses access.
mod raw;
pub use raw::{AnsweredRaw, RunSqlError, RunningRaw, run_sql};

/// Charges `bytes` against `context`'s own subject, and turns a refusal into the domain's own
/// `RefusalReason`.
///
/// **The one place [`Attribution`] collapses to a ledger key and a [`Charge`] becomes a
/// [`RefusalReason`]**, shared by [`budget_exhausted`] (the mono path) and
/// `federated::answer_federated`'s summed charge - two call sites minting the same key and the
/// same rounding two different ways is exactly how mutation #2 in `#684`'s review survived.
/// `docs/adr/0030` decides the key: the subject `PrincipalChain::attribution()` names, never the
/// acting chain, so an agent's charge lands on the human it acted for.
///
/// **`reset_after` rounds UP to whole seconds**, not down: `Duration::as_secs` floors, so a refusal
/// in the last fraction of a window would otherwise mint `reset_after_seconds: 0` - `Retry-After:
/// 0`, which `crates/sutura-http/src/wire.rs` documents as "a promise the next request will be
/// answered". Flooring breaks that promise for anyone refused inside the final second.
///
/// `now` is a parameter rather than read inside, for the reason [`SpendLedger::charge`] already
/// takes one: a test can pin the ledger at an exact offset into its window (this function's own
/// suite does, at 59.5s of a 60s window) without sleeping.
pub(crate) fn charge_subject(ledger: &SpendLedger, context: &RequestContext, bytes: u64, now: Instant) -> Option<RefusalReason> {
    let subject = match context.chain().attribution() {
        Attribution::BareSubject { subject } | Attribution::ActingFor { subject, .. } => subject,
    };
    match ledger.charge(subject, bytes, now) {
        Charge::Admitted => None,
        Charge::Refused { reset_after } => Some(RefusalReason::BudgetExhausted {
            reset_after_seconds: reset_after.as_secs() + u64::from(reset_after.subsec_nanos() != 0),
        }),
    }
}

/// The refusal for a spent per-replica byte ceiling, if this dry run's own price puts `context`'s
/// subject over it.
///
/// `None` for every case that is not a refusal: no ceiling configured, an adapter that did not
/// price (`PreFlight::NotAsked`), one that priced and could not (`Accepted { estimated_bytes: None
/// }`), or a priced dry run the ledger still admits.
pub(crate) fn budget_exhausted(ledger: &SpendLedger, context: &RequestContext, preflight: PreFlight) -> Option<RefusalReason> {
    let PreFlight::Accepted {
        estimated_bytes: Some(estimated_bytes),
    } = preflight
    else {
        return None;
    };
    charge_subject(ledger, context, estimated_bytes.bytes(), Instant::now())
}

/// The view a request context resolves against - `docs/adr/0028`.
///
/// **Here, beside [`Asked`] and [`crate::capability::Permitted`]**, so no transport owns the
/// decision: `answer` below reads it, and so does every route that renders a catalog through
/// `Asked::context`.
///
/// **Derived from the SUBJECT, not from whether the caller presented anything else.** A verified
/// caller's granted set stays on `context` regardless of whether it maps to anything, so an empty
/// grant and no verification at all must not read alike: [`Subject::TheDeploymentItself`] is the
/// explicit single-player posture the ADR's surface table names - every non-verified surface reaches
/// this value and always has - while [`Subject::Verified`] is a caller this deployment
/// authenticated, whose mapped audiences decide what is visible even when that set is empty.
#[must_use]
pub fn scoped_for<'a>(pinned: &'a PinnedDefinitions, context: &RequestContext) -> ScopedView<'a> {
    match context.chain().subject() {
        Subject::TheDeploymentItself => ScopedView::everything(pinned),
        Subject::Verified(_) => ScopedView::granted_by(pinned, context.audiences().clone()),
    }
}

/// Boot-time anchor verification and bundle introspection - see the module for what came out of
/// this file and why.
mod anchors;
pub(crate) use anchors::flatten;
pub use anchors::{grains_coarsest_first, source_of, sources, verify_anchors};

/// The two-source answer path - see the module for what came out of this file and why.
///
/// It carries its own `#[cfg(test)] mod tests` rather than having a suite file beside `tests`, and the
/// module doc says why: a test module declared from HERE is orphaned when `test-causality` reverts
/// this file, so the proof it produced was vacuous.
mod federated;

/// Holding a bundle's cardinality declarations against the data, at boot.
///
/// Private, because its callers are `verify_and_validate` and the composition roots' own reporting,
/// and its refusal is a `NotValidated` both roots already render. Its header states what the
/// remaining outcomes do and do not surface.
///
/// **It is orphaned from `test-causality`'s point of view for the reason the module above states**,
/// and one level worse: this `mod` line is a plain declaration, so the gate reverts it, and the
/// module's own `#[cfg(test)] mod tests` then compiles into nothing. Whether that suite is red
/// against the base behaviour was therefore not proven mechanically - see the handoff for the
/// mutations that stand in its place.
mod declared_keys;
/// This crate's own unit suite, in its own file.
///
/// Moved out of this one when it reached the 1000-line gate. `cargo xtask max-lines` cannot exempt
/// anything under `crates/`, which is what makes a split the only answer.
#[cfg(test)]
mod tests;
/// The fakes this crate's own unit tests share, in their own file.
///
/// One module rather than a copy per test module, because [`warehouses`] and the suite below both need
/// a `Warehouse` that declares a posture and executes nothing interesting, and two copies of one fake
/// is two things to keep in step with the port. It holds the credential brokers too, one per
/// behaviour, so a test reads as a case rather than as a configuration.
#[cfg(test)]
mod tests_support;
