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
use sutura_domain::identity::{CredentialBroker, Minted, RequestContext, SourceSet};
use sutura_domain::model::{Grain, MetricName, SourceName};
use sutura_domain::pinned::{AnchorCheck, AnchorReport, NotExecutedReason, PinnedDefinitions};
use sutura_domain::plan::Executable;
use sutura_domain::query::{Query, RefusalReason, ToolOutcome};
use sutura_domain::warehouse::{RowSet, Warehouse};
use sutura_semantic::{BundleInconsistent, Compiled, compile};

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
    /// The broker granted credentials that do not cover the source this plan reads.
    ///
    /// **A wiring defect between the broker and the plan, so an `Err` and not a refusal** - the
    /// question was fine. `sutura_domain::identity::LegCredentials` refuses a set that does not
    /// cover the sources it was minted FOR, so this is the case where a broker was asked about one
    /// set and answered about another: the two disagree about what is being answered, and either
    /// half may be the wrong one.
    ///
    /// Executing anyway is the alternative this variant exists to remove, and it is the one that
    /// would have run the leg as the process.
    #[error("the credentials that came back do not cover this plan")]
    Credentials {
        #[source]
        cause: NoCredentialForThePlan,
    },
}

/// A broker granted credentials that say nothing about a source the plan reads.
///
/// Its own type rather than a variant carrying a bare name, so the cause survives `#[source]` when
/// the service's generic parameters are erased at the driving port - the same reason every other
/// failure that crosses that boundary is a typed error rather than a sentence.
///
/// The field is `at` rather than `source`, and that is not a naming preference: `thiserror` reads a
/// field called `source` as the `Error::source` chain, and a `SourceName` there does not compile.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "nothing was granted for source `{at}`, which this plan reads - the broker and the plan disagree about what is being answered"
)]
pub struct NoCredentialForThePlan {
    at: SourceName,
}

impl NoCredentialForThePlan {
    /// Which source had no credential.
    ///
    /// Named `at` rather than `source` for the reason above: `clippy::same_name_method` is denied, and
    /// an inherent `source` beside the trait's own is a call site whose meaning depends on which
    /// traits are in scope.
    #[inline]
    #[must_use]
    pub const fn at(&self) -> &SourceName {
        &self.at
    }
}

/// What answering produced, or why it could not.
///
/// A named alias because the inline form is over the complexity threshold in `clippy.toml`, and
/// naming it is the better half of that trade: the generic parameter is a warehouse, not a result.
pub type Answered<W, B> = Result<ToolOutcome, ServiceError<<W as Warehouse>::Error, <B as CredentialBroker>::Error>>;

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
pub fn answer<W, B>(
    definitions: &Validated<PinnedDefinitions>,
    query: &Query,
    context: &RequestContext,
    broker: &B,
    warehouses: &Warehouses<W>,
) -> Answered<W, B>
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
        Compiled::Refused { reason } => return Ok(ToolOutcome::Refusal { reason }),
        Compiled::Planned { plan } => plan,
    };
    let (Some(warehouse), Some(executed_as)) = (warehouses.get(plan.source()), warehouses.executed_on(plan.source())) else {
        return Ok(ToolOutcome::Refusal {
            reason: RefusalReason::SourceUnavailable {
                source: plan.source().clone(),
            },
        });
    };
    // The credential, minted once for every source this answer reads. A refusal comes back in the
    // `Ok` and leaves as one: "this subject has no credential at that source" is a governance
    // outcome, and a broker that could not be reached is an `Err` - see `ServiceError::Broker`.
    let credentials = match broker
        .mint(context, &SourceSet::of(plan.source().clone()))
        .map_err(|cause| ServiceError::Broker { cause })?
    {
        Minted::Refused { source } => {
            return Ok(ToolOutcome::Refusal {
                reason: RefusalReason::CredentialUnavailable { source },
            });
        }
        Minted::Granted { credentials } => credentials,
    };
    // Unreachable for a broker that answered about the set it was asked about - `LegCredentials`
    // refuses one that does not cover its own source set - so this arm is the broker and the plan
    // disagreeing about what is being answered. An `Err`, because executing anyway is what would
    // run the leg as this process.
    let Some(presented) = credentials.presented_for(plan.source()) else {
        return Err(ServiceError::Credentials {
            cause: NoCredentialForThePlan {
                at: plan.source().clone(),
            },
        });
    };
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
                return Ok(ToolOutcome::Refusal {
                    reason: RefusalReason::ResourcesExhausted { ceiling_bytes },
                });
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
        return Ok(ToolOutcome::Refusal {
            reason: RefusalReason::ResultTooLarge { limit: plan.max_rows() },
        });
    }
    // The posture travels with the answer, read off the adapter that just executed rather than off a
    // settings tree - `executed_as` was taken from the registry above, beside the warehouse this
    // question actually ran on. A field derived from configuration would report what was configured
    // rather than what ran, and the two disagreeing is the case the field exists for.
    Ok(ToolOutcome::Answer {
        provenance: pinned.provenance(executed_as),
        rows,
    })
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
fn exceeds_row_cap(returned: usize, max_rows: u32) -> bool {
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
    // `verify_anchor` and not `execute`, and the difference is the identity rather than the method
    // name. There is no caller at boot, so there is no credential in scope and nothing here could
    // pass one - which is what stops this path from being the door the service-identity fallback
    // comes back through. What it runs as is whatever the deployment configured this adapter with,
    // and `docs/adr/0008` part 1 is why that is the only honest answer available: under row-level
    // security a per-subject anchor is a function rather than a number.
    let rows = match warehouse.verify_anchor(&plan) {
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

/// The fakes this crate's own unit tests share, in their own file.
///
/// One module rather than a copy per test module, because [`warehouses`] and the suite below both need
/// a `Warehouse` that declares a posture and executes nothing interesting, and two copies of one fake
/// is two things to keep in step with the port. It holds the credential brokers too, one per
/// behaviour, so a test reads as a case rather than as a configuration.
#[cfg(test)]
mod tests_support;

/// This crate's own unit suite, in its own file.
///
/// Moved out of this one when it reached the 1000-line gate. `cargo xtask max-lines` cannot exempt
/// anything under `crates/`, which is what makes a split the only answer.
#[cfg(test)]
mod tests;
