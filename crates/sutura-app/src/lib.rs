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

use sutura_domain::catalog::Anchor;
use sutura_domain::identity::{
    Agreed, BoundToTheRequest, CredentialBroker, CredentialsDoNotFitTheRequest, Expiry, PresentedDisagreesWithPosture,
    RequestContext, SourceSet,
};
use sutura_domain::model::{Grain, MetricName, SourceName};
use sutura_domain::pinned::{AnchorCheck, AnchorReport, NotExecutedReason, PinnedDefinitions};
use sutura_domain::plan::{AnchorPlan, Executable, FederatedFailure};
use sutura_domain::query::{Query, RefusalReason, ToolOutcome};
use sutura_domain::warehouse::{RowSet, Warehouse};
use sutura_semantic::{BundleInconsistent, Compiled, compile};

use crate::federated::answer_federated;
pub use crate::warehouses::{SourceAlreadyOpen, Warehouses};

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

// What this surface can be asked to do, and what one caller may do of it. Here for the same reason
// `surface` is: the tool set IS the driving port's operation set, `sutura-mcp` and `sutura-http`
// cannot see each other, and a set owned by one transport is a set the other has to reach through
// it. The module's own documentation carries the argument and the limits.
pub mod capability;

pub use crate::capability::{Capability, Permitted};
pub use crate::proof::{Validated, verify_and_validate};

/// The proof, and the only operation that can mint it.
///
/// A module rather than two items in `lib.rs`, and a PRIVATE one, because that is the mechanism: the
/// field of [`Validated`] and its tuple constructor are visible exactly here, so
/// [`verify_and_validate`] is the only safe code anywhere that can produce one. Moving either item
/// out of this module, or adding a second `pub fn` to it that does not call a `Warehouse`, is what a
/// reviewer has to notice - and it is a one-item diff in one place rather than a property of every
/// call site.
mod proof {
    use sutura_domain::pinned::{NotValidated, PinnedDefinitions};
    use sutura_domain::warehouse::Warehouse;

    use crate::warehouses::Warehouses;

    /// A `T` that has been shown to hold up.
    ///
    /// **The service accepts only this, so an unvalidated bundle is unrepresentable rather than
    /// merely refused.** The field is private to the module this type is declared in, and
    /// [`verify_and_validate`] is the only thing in that module which builds one.
    ///
    /// Generic in the type it wraps, but obtainable only for [`PinnedDefinitions`], and that
    /// asymmetry is the point: validating means re-running every anchor the bundle declares, so
    /// whatever mints this has to be able to enumerate them and to execute them. A blanket
    /// constructor for any `T` would be a wrapper that proves nothing, which is worse than no
    /// wrapper because it reads like proof.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Validated<T>(T);

    impl<T> Validated<T> {
        #[inline]
        pub const fn get(&self) -> &T {
            &self.0
        }

        #[inline]
        pub fn into_inner(self) -> T {
            self.0
        }
    }

    /// Re-runs every anchor against `warehouse`, and returns the bundle only if all of them held.
    ///
    /// The one operation that produces a [`Validated`] bundle. It takes the `Warehouse` and calls
    /// it, which is the whole of what the type is now allowed to claim: not "somebody asserted these
    /// anchors match", but "these statements were executed against this data system and reproduced
    /// the numbers their author certified".
    ///
    /// **What it still does not claim.** `W` is a port, so a caller may pass a fake - and a fake is
    /// exactly what the golden suite passes, deliberately, because the alternative is a test suite
    /// that needs a database to check a refusal. What the type proves is that a warehouse was
    /// called; that the warehouse was the one holding the business's data is a composition-root
    /// decision no signature can make. `answer` narrows it a little further by refusing a plan whose
    /// source is not the adapter's own.
    ///
    /// The forgery this closes does not compile:
    ///
    /// ```compile_fail
    /// use sutura_app::Validated;
    /// use sutura_domain::model::MetricName;
    /// use sutura_domain::pinned::{AnchorCheck, AnchorReport, PinnedDefinitions};
    ///
    /// // Enumerate the anchors, claim each one matched, hand the claim to the validator.
    /// // No data system is opened and no statement is executed.
    /// fn _forge(pinned: PinnedDefinitions) -> Validated<PinnedDefinitions> {
    ///     let names: Vec<MetricName> = pinned.anchored_metrics().map(|(name, _)| name.clone()).collect();
    ///     let mut report = AnchorReport::new();
    ///     for name in names {
    ///         report.record(name, AnchorCheck::Matched);
    ///     }
    ///     // Neither the constructor that was here nor the tuple constructor is reachable.
    ///     Validated::new(pinned, &report).unwrap()
    /// }
    ///
    /// fn _wrap(pinned: PinnedDefinitions) -> Validated<PinnedDefinitions> {
    ///     Validated(pinned)
    /// }
    /// ```
    ///
    /// The twin of that block, which pins the names so a rename cannot make it pass vacuously:
    ///
    /// ```
    /// use sutura_app::{Validated, Warehouses, verify_and_validate};
    /// use sutura_domain::pinned::{NotValidated, PinnedDefinitions};
    /// use sutura_domain::warehouse::Warehouse;
    ///
    /// fn _served(_bundle: &Validated<PinnedDefinitions>) {}
    ///
    /// fn _mint<W: Warehouse>(
    ///     pinned: PinnedDefinitions,
    ///     warehouses: &Warehouses<W>,
    /// ) -> Result<Validated<PinnedDefinitions>, NotValidated> {
    ///     verify_and_validate(pinned, warehouses)
    /// }
    /// ```
    ///
    /// # It takes the registry, not one warehouse
    ///
    /// Each metric's anchor runs against the data system that metric's own plan names, so a bundle
    /// spanning two configured sources verifies both halves. Under one warehouse every anchor on the
    /// second source came back as a source mismatch, which is a bundle that cannot be validated for a
    /// reason that has nothing to do with its numbers.
    ///
    /// **What it still does not take is an identity**, and that is the honest limit on what an executed
    /// anchor proves. The registry says which posture each adapter was handed; it does not hand the
    /// adapter a credential to re-run the anchor under, because the port has no parameter for one yet.
    /// So the bundle is proven to compute its certified numbers for whatever identity each adapter is
    /// configured with - the process, for the file engine that ships - and the composition root refuses
    /// a bundle with an anchor on a source that declared no verification identity, which is the half
    /// available before the port changes.
    pub fn verify_and_validate<W>(
        pinned: PinnedDefinitions,
        warehouses: &Warehouses<W>,
    ) -> Result<Validated<PinnedDefinitions>, NotValidated>
    where
        W: Warehouse,
    {
        let report = super::verify_anchors(&pinned, warehouses);
        report.verdict(&pinned)?;
        Ok(Validated(pinned))
    }
}

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
pub enum ServiceError<E, M> {
    #[error("the question could not be compiled")]
    Compile {
        #[source]
        cause: BundleInconsistent,
    },
    #[error("the data system did not answer")]
    Warehouse {
        #[source]
        cause: E,
    },
    /// The federated combiner could not assemble the two legs' rows.
    ///
    /// **An internal defect rather than a refusal, for every arm but the two `answer_federated`
    /// maps by name.** A correctly split and certified question should not make the combiner fail: a
    /// missing column or a malformed result is a bug in the splitter, an adapter or the combiner, so
    /// it leaves as a failure the transport answers like a data-system outage. The two the answer
    /// path turns into refusals are the two governance outcomes - [`FederatedFailure::ResourcesExhausted`],
    /// refused as [`RefusalReason::ResourcesExhausted`], and the row cap, refused as
    /// [`RefusalReason::ResultTooLarge`].
    #[error("the combined answer could not be assembled")]
    Federated {
        #[source]
        cause: FederatedFailure,
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
pub type Answering<W, B> = Result<Answered, ServiceError<<W as Warehouse>::Error, <B as CredentialBroker>::Error>>;

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
#[expect(
    clippy::too_many_arguments,
    reason = "six inputs is what a certified answer needs; naming each beats a struct nobody else reads"
)]
pub fn answer<W, B>(
    definitions: &Validated<PinnedDefinitions>,
    query: &Query,
    context: &RequestContext,
    broker: &B,
    warehouses: &Warehouses<W>,
    working_set_bytes: u64,
) -> Answering<W, B>
where
    W: Warehouse,
    B: CredentialBroker,
{
    let pinned = definitions.get();
    let compiled = compile(query, pinned).map_err(|cause| ServiceError::Compile { cause })?;
    // The PLAN is what the port takes now, not a rendered statement: an adapter that executes
    // without generating SQL is a first-class implementation of it. A SQL-speaking adapter renders
    // the plan itself, for its own dialect.
    let plan = match compiled {
        Compiled::Refused { reason } => return Ok(Answered::declined_before_minting(ToolOutcome::Refusal { reason })),
        Compiled::Federated { plan } => {
            return answer_federated(pinned, &plan, context, broker, warehouses, working_set_bytes);
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
    warehouse
        .dry_run(Executable::Query(&plan), presented)
        .map_err(|cause| ServiceError::Warehouse { cause })?;
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
    // side. `Warehouse::execute` takes a `&Presented` and no deadline, so an adapter cannot make the
    // before-leg check `docs/adr/0008` part 4 describes, and a credential that ages out during
    // execution is refused by the data system rather than here.
    credentials
        .still_usable_at(now_in_unix_seconds())
        .map_err(|cause| ServiceError::Credentials { cause })?;
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
    let rows = match warehouse.execute(Executable::Query(&plan), presented) {
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
            // Anything else is a failure rather than a refusal, and the typed cause travels with it.
            return Err(ServiceError::Warehouse { cause });
        }
    };
    // The row cap, enforced rather than merely requested. The plan asked for one row more than
    // `plan.max_rows()`, so more than that many coming back means the result was cut short - and a
    // truncated result is a wrong total under a certified name, with provenance attached and nothing
    // saying it is partial. Refused, because "this question is too wide to certify" is an answer the
    // caller can act on and a silent partial one is not.
    if exceeds_row_cap(rows.rows().len(), plan.max_rows()) {
        return Ok(Answered::under(
            &credentials,
            ToolOutcome::Refusal {
                reason: RefusalReason::ResultTooLarge { limit: plan.max_rows() },
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

/// Whether a result set came back with more rows than its plan capped it at.
///
/// **A governance control, so the direction it fails in is the whole of what this function is for.**
/// The comparison used to be written inline as
/// `rows.len() > usize::try_from(plan.max_rows()).unwrap_or(usize::MAX)`, which reads as a cap and
/// is a cap being lifted: a conversion that came back `Err` produced `usize::MAX`, and no result set
/// is longer than that, so the one refusal that stops a TRUNCATED total from being certified would
/// have been skipped. Unreachable on any target with 32-bit pointers or wider, and still the wrong
/// direction to have written down.
///
/// It compares in `u64` instead, where the plan's `u32` cap widens with `From` and cannot fail at
/// all. The count still needs a conversion, because neither direction between these two types is
/// infallible - `From<usize> for u64` does not exist, since a target with pointers wider than 64
/// bits would lose a count, and `From<u32> for usize` does not either, since a 16-bit target could
/// not hold the cap. What changed is which way the unreachable case falls: a count that does not fit
/// a `u64` is a count larger than any `u32` cap, so `u64::MAX` here is not a fallback that guesses,
/// it is the answer. The control refuses rather than opening.
///
/// Named rather than inline so the boundary is testable without a data system: the case that decides
/// a certification is one row over the cap, and reaching it through [`answer`] means fabricating ten
/// thousand rows through a validated bundle.
pub(crate) fn exceeds_row_cap(returned: usize, max_rows: u32) -> bool {
    u64::try_from(returned).unwrap_or(u64::MAX) > u64::from(max_rows)
}

/// Re-executes every declared anchor and reports what each produced.
///
/// Returns a report rather than a `Result`, because "this one metric no longer computes its number"
/// and "the data system is down" are both outcomes worth recording per metric. Collapsing either into
/// a single error would lose which metric, and the whole point is to name it.
pub fn verify_anchors<W>(pinned: &PinnedDefinitions, warehouses: &Warehouses<W>) -> AnchorReport
where
    W: Warehouse,
{
    let mut report = AnchorReport::new();
    for (name, anchor) in pinned.anchored_metrics() {
        report.record(name.clone(), check_one(pinned, warehouses, name, anchor));
    }
    report
}

/// Every cause beneath an error, outermost first.
///
/// **This is where the anchor failure's cause used to be thrown away.** `Display` on a `thiserror`
/// enum prints the outermost message and stops, so formatting an adapter error into a sentence
/// discarded the driver's own complaint - the part that names the table, the column or the file. The
/// chain cannot be kept as a typed cause either: the port's error is a generic parameter, and the
/// domain must not hold one. Walking it to text here is the lossless option at that boundary, and
/// this is the only place where the typed error is still in scope.
fn causes(error: &dyn core::error::Error) -> Vec<String> {
    let mut chain = Vec::new();
    let mut cursor = error.source();
    while let Some(cause) = cursor {
        chain.push(cause.to_string());
        cursor = cause.source();
    }
    chain
}

/// A typed error, flattened for a [`NotExecutedReason`] variant that cannot name it.
fn flatten(error: &dyn core::error::Error) -> (String, Vec<String>) {
    (error.to_string(), causes(error))
}

/// Checks one anchor.
fn check_one<W>(pinned: &PinnedDefinitions, warehouses: &Warehouses<W>, metric: &MetricName, anchor: &Anchor) -> AnchorCheck
where
    W: Warehouse,
{
    let not_executed = |reason: NotExecutedReason| AnchorCheck::NotExecuted { reason };

    let Some(definition) = pinned.definitions().metric(metric) else {
        return not_executed(NotExecutedReason::BundleMissingMetric);
    };
    // The coarsest grain the metric declares, so that an anchor range covering one period yields one
    // row. A finer grain would return several, and there is no single number to compare against.
    let Some(grain) = definition.grains().iter().copied().max() else {
        return not_executed(NotExecutedReason::NoGrain);
    };

    // No dimensions and no filters: an anchor is the metric's own number, not a slice of it.
    let question = Query::new(metric.clone(), grain, anchor.range(), Vec::new(), Vec::new());
    let compiled = match compile(&question, pinned) {
        Ok(compiled) => compiled,
        Err(cause) => {
            let (message, chain) = flatten(&cause);
            return not_executed(NotExecutedReason::NotCompiled { message, chain });
        }
    };
    let plan = match compiled {
        Compiled::Refused { reason } => {
            return not_executed(NotExecutedReason::Refused { reason });
        }
        // An anchor is asked with no dimensions, so it can only ever be one source. Reaching this
        // arm is a defect here rather than anything about the data.
        Compiled::Federated { .. } => {
            return not_executed(NotExecutedReason::NotCompiled {
                message: String::from("an anchor's question resolved to two data systems"),
                chain: Vec::new(),
            });
        }
        Compiled::Planned { plan } => plan,
    };
    // A governance condition, not prose in a report field: the plan names a data system this process
    // did not open, which is the same thing `answer` refuses a question for. The plan SELECTS its
    // warehouse - it is not compared against one - so this arm is "nobody configured that source"
    // rather than "the one adapter we hold is called something else".
    let Some(warehouse) = warehouses.get(plan.source()) else {
        return not_executed(NotExecutedReason::SourceNotConfigured {
            plan: plan.source().clone(),
        });
    };
    // The plan, checked against the bundle as this anchor's own before anything executes it. Every
    // fact `AnchorPlan::of` compares - that the metric is defined, that it declares an anchor, the
    // range that anchor certifies, and the coarsest grain it is asked at - is read off `pinned` rather
    // than handed in, which is what makes the check a check on THIS function rather than on its
    // arguments. It is a self-check and not an authority: `AnchorPlan`'s own documentation says so,
    // and what keeps the credential-free method to this one call site is the `clippy.toml` ban below.
    // Reaching the `Err` arm means this function compiled something other than the anchor's own
    // question, so it is a defect here rather than a governance outcome, and it is reported as one:
    // `NotExecutedReason::NotAnAnchor` names the metric's report entry rather than failing the boot
    // for every other anchor in the bundle.
    let anchor_plan = match AnchorPlan::of(&plan, pinned, metric) {
        Ok(anchor_plan) => anchor_plan,
        Err(cause) => {
            let (message, chain) = flatten(&cause);
            return not_executed(NotExecutedReason::NotAnAnchor { message, chain });
        }
    };
    // `verify_anchor` and not `execute`, and the difference is the identity rather than the method
    // name. There is no caller at boot, so there is no credential in scope and nothing here could
    // pass one - which is what stops this path from being the door the service-identity fallback
    // comes back through. What it runs as is whatever the deployment configured this adapter with,
    // and `docs/adr/0008` part 1 is why that is the only honest answer available: under row-level
    // security a per-subject anchor is a function rather than a number.
    //
    // THE SINGLE EXPECTATION for the `clippy.toml` ban on this method, and it is the mechanism that
    // makes the credential-free path boot-only: a second call site anywhere in the workspace is an
    // error under `-D warnings` until somebody writes a second `#[expect]` a reviewer sees in the
    // diff. `AnchorPlan` cannot carry that on its own - every value its constructor reads is publicly
    // constructible, and Rust has no cross-crate friend visibility.
    #[expect(
        clippy::disallowed_methods,
        reason = "the boot path is the one caller of the method that executes with no credential; the \
                  ban exists so that this is the only place it is called from"
    )]
    let rows = match warehouse.verify_anchor(anchor_plan) {
        Ok(rows) => rows,
        Err(cause) => {
            let (message, chain) = flatten(&cause);
            return not_executed(NotExecutedReason::Failed { message, chain });
        }
    };
    match measure_of(rows.verified_at_boot(), metric) {
        Ok(actual) if actual == anchor.value() => AnchorCheck::Matched,
        Ok(actual) => AnchorCheck::Mismatch {
            expected: String::from(anchor.value()),
            actual,
        },
        Err(reason) => not_executed(reason),
    }
}

/// The measure column of a single-row anchor result.
///
/// Insists on exactly one row. An anchor range that covers two periods at the metric's coarsest
/// grain comes back as two rows, and reading the first would compare a certified total against one
/// period of it: a mismatch that reads like a broken definition and is a mis-declared range.
///
/// The three ways this fails are variants of [`NotExecutedReason`] rather than a second enum
/// declared here. They are all about the shape of a [`RowSet`], which is a domain type, and one
/// enum whose variants match the branches of the check is what lets the report be read without a
/// translation step in the middle that could lose one.
fn measure_of(rows: &RowSet, metric: &MetricName) -> Result<String, NotExecutedReason> {
    if rows.rows().len() != 1 {
        return Err(NotExecutedReason::NotOneNumber { rows: rows.rows().len() });
    }
    let label = metric.as_str();
    let index = rows.column_index(label).ok_or_else(|| NotExecutedReason::NoMeasureColumn {
        label: String::from(label),
    })?;
    rows.cell(0, index)
        .map(sutura_domain::warehouse::Value::render)
        .ok_or(NotExecutedReason::ResultShapeMismatch)
}

/// The data systems a bundle reads from.
///
/// Exposed because a composition root has to decide which adapters to open before it can answer
/// anything, and reading it off the bundle beats being told twice.
pub fn sources(pinned: &PinnedDefinitions) -> Vec<&SourceName> {
    let mut out: Vec<&SourceName> = pinned
        .definitions()
        .models()
        .values()
        .map(sutura_domain::catalog::Model::source)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The data system one metric's own model sits on.
///
/// Exposed for the composition root's anchor check: an anchor is asked with no dimensions, so it
/// resolves to the metric's own model and therefore to that model's source - which is the source whose
/// declared verification identity would have to run it.
///
/// **Narrower than "every source this metric's plan could read", deliberately.** A question WITH
/// dimensions can reach a joined model, and the plan stage refuses one that spans two sources - so for
/// a plan that compiles at all this is the only source there is. What it is not is a general answer for
/// a federated plan, and it stops being the right function the moment one exists.
pub fn source_of<'bundle>(pinned: &'bundle PinnedDefinitions, metric: &MetricName) -> Option<&'bundle SourceName> {
    let definitions = pinned.definitions();
    let model = definitions.metric(metric)?.model();
    Some(definitions.model(model)?.source())
}

/// The grains a metric declares, coarsest first.
///
/// A small helper the composition root uses to describe a metric, kept here so the ordering is the
/// same one [`verify_anchors`] picks a grain by.
pub fn grains_coarsest_first(pinned: &PinnedDefinitions, metric: &MetricName) -> Vec<Grain> {
    pinned
        .definitions()
        .metric(metric)
        .map(|definition| {
            let mut grains: Vec<Grain> = definition.grains().iter().copied().collect();
            grains.sort_unstable_by(|a, b| b.cmp(a));
            grains
        })
        .unwrap_or_default()
}

/// The two-source answer path - see the module for what came out of this file and why.
///
/// It carries its own `#[cfg(test)] mod tests` rather than having a suite file beside `tests`, and the
/// module doc says why: a test module declared from HERE is orphaned when `test-causality` reverts
/// this file, so the proof it produced was vacuous.
mod federated;
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
