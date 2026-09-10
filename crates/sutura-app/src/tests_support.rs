//! The fakes this crate's own unit tests share: a data system, and the credential brokers.
//!
//! In its own file for the reason `crate::prompt`'s suite is: `lib.rs` reached the 1000-line gate,
//! and the gate's answer to that is to split the file rather than to shorten what is documented.
//!
//! A module rather than a copy per test module, because [`crate::warehouses`] and `crate::tests` both
//! need a `Warehouse` that declares a posture and executes nothing interesting - and two copies of
//! one fake is two things to keep in step with the port.

use std::collections::BTreeMap;

use sutura_domain::identity::{
    CredentialBroker, Expiry, LegCredentials, Minted, Presented, RequestContext, Secret, SourceSet, Subject,
};
use sutura_domain::model::SourceName;
use sutura_domain::plan::Executable;
use sutura_domain::source::{AcknowledgementReason, ImpersonationCapability, SharedIdentityDeclared, SourcePosture};
use sutura_domain::warehouse::cardinality::{DeclaredKey, KeyCounts, KeyUniqueness};
use sutura_domain::warehouse::{AnchorRows, PreFlight, RowSet, Warehouse};

/// The driver's own complaint, one level below the adapter's.
#[derive(Debug, thiserror::Error)]
#[error("no such file: orders.csv")]
pub(crate) struct DriverFailure;

/// What an adapter returns: its own message, with the driver's underneath it.
#[derive(Debug, thiserror::Error)]
pub(crate) enum AdapterFailure {
    #[error("the data system rejected the statement")]
    Statement {
        #[source]
        cause: DriverFailure,
    },
    /// The credential this fake was handed is one it has nowhere to put.
    ///
    /// **The shape both shipped adapters have**, reproduced here because this is the crate where
    /// the consequence is assertable: a wiring defect between the broker and the source
    /// declaration has to leave as an `Err` rather than as a refusal, and only a fake pair - a
    /// broker that mints subject material and an adapter that cannot use it - can provoke it
    /// without a data system.
    #[error("source `{at}` was handed {presented}, and this adapter has nowhere to put it")]
    NoPlaceForASubject { at: String, presented: &'static str },
    /// The data system would not return the whole result at once.
    #[error("the data system would not return the whole result at once")]
    TooMuchData,
    /// The identity the statement ran as is not permitted to ask it.
    #[error("the data system refused the statement at the identity/authorization level")]
    RefusedBySource,
}

/// A data system with a declared source and posture, which either answers one fixed result or
/// fails every statement.
///
/// The posture is a constructor argument and not a default, which is the port's own rule: an
/// adapter is *handed* the posture the deployment declared, and a fake that invented one would be
/// asserting this file's opinion back to the test.
pub(crate) struct FixedWarehouse {
    source: SourceName,
    posture: SourcePosture,
    result: Option<RowSet>,
    /// How long this fake's pre-flight takes.
    ///
    /// **For one test, and it is the only way to reach the second deadline check.** `answer` compares
    /// the credential's deadline against the clock again between the pre-flight and the execution,
    /// because a pre-flight against a networked data system is a round trip. A fake that returns
    /// instantly cannot make that comparison fail, so the one test that provokes it hands a fake that
    /// takes longer than the credential's remaining life. `Duration::ZERO` for every other fixture,
    /// which sleeps not at all.
    pre_flight_takes: std::time::Duration,
    /// What this fake says when it is asked whether a declared join key is unique.
    ///
    /// Defaults to [`CountsBack::NotAsked`] on every constructor, which is the port's own default -
    /// so every fixture that predates the check answers exactly as it did before, and only the
    /// tests that are about the check say otherwise.
    counts: CountsBack,
}

/// What a fake answers when the boot path asks whether a declared key is really unique.
///
/// Three values because the boot path treats three outcomes differently, and only one of them is a
/// refusal - `crate::declared_keys` is where that is argued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CountsBack {
    /// This fake did not count. The port's default, and what every other fixture answers.
    NotAsked,
    /// It counted, and this is the pair.
    Counted { rows: u64, distinct: u64 },
    /// It could not count. A data system that is briefly unreachable looks like this.
    Failed,
}

impl FixedWarehouse {
    /// One that fails every statement, with a cause worth reading.
    pub(crate) const fn new(source: SourceName, posture: SourcePosture) -> Self {
        Self {
            source,
            posture,
            result: None,
            pre_flight_takes: std::time::Duration::ZERO,
            counts: CountsBack::NotAsked,
        }
    }

    /// One that answers every statement with `result`.
    pub(crate) const fn answering(source: SourceName, posture: SourcePosture, result: RowSet) -> Self {
        Self {
            source,
            posture,
            result: Some(result),
            pre_flight_takes: std::time::Duration::ZERO,
            counts: CountsBack::NotAsked,
        }
    }

    /// The same, answering `counts` when the boot path asks about a declared key.
    pub(crate) const fn answering_and_counting(
        source: SourceName,
        posture: SourcePosture,
        result: RowSet,
        counts: CountsBack,
    ) -> Self {
        Self {
            source,
            posture,
            result: Some(result),
            pre_flight_takes: std::time::Duration::ZERO,
            counts,
        }
    }

    /// The same, with a pre-flight that takes `pre_flight_takes` before it answers.
    pub(crate) const fn answering_after(
        source: SourceName,
        posture: SourcePosture,
        result: RowSet,
        pre_flight_takes: std::time::Duration,
    ) -> Self {
        Self {
            source,
            posture,
            result: Some(result),
            pre_flight_takes,
            counts: CountsBack::NotAsked,
        }
    }

    /// Refuses credential material this fake has nowhere to put, the way both shipped adapters do.
    fn deliverable(&self, presented: &Presented) -> Result<(), AdapterFailure> {
        match *presented {
            Presented::SharedServiceUser { .. } => Ok(()),
            Presented::SubjectToken { .. } | Presented::SubjectPrincipal { .. } => Err(AdapterFailure::NoPlaceForASubject {
                at: String::from(self.source.as_str()),
                presented: presented.as_str(),
            }),
        }
    }
}

impl Warehouse for FixedWarehouse {
    type Error = AdapterFailure;

    // A fake over no data system at all, so there is nowhere for a subject credential to arrive -
    // the same answer the in-process engine gives, for the same reason.
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn dry_run(&self, _executable: Executable<'_>, presented: &Presented) -> Result<PreFlight, Self::Error> {
        self.deliverable(presented)?;
        // Zero for every fixture but one. See the field's own documentation: a pre-flight that takes
        // no time cannot make the deadline check between it and the execution fail.
        std::thread::sleep(self.pre_flight_takes);
        match self.result {
            Some(_) => Ok(PreFlight::NotAsked),
            None => Err(AdapterFailure::Statement { cause: DriverFailure }),
        }
    }

    fn execute(&self, _executable: Executable<'_>, presented: &Presented) -> Result<RowSet, Self::Error> {
        self.deliverable(presented)?;
        self.result.clone().ok_or(AdapterFailure::Statement { cause: DriverFailure })
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        self.result
            .clone()
            .map(AnchorRows::of)
            .ok_or(AdapterFailure::Statement { cause: DriverFailure })
    }

    /// Whatever this fixture was built to say, without looking at the key it was handed.
    ///
    /// A fake over no data system cannot count anything, so what it is for is the boot path's own
    /// branches: the default, a clean pair, a violated pair, and a data system that could not answer.
    fn declared_key(&self, _key: DeclaredKey<'_>) -> Result<KeyUniqueness, Self::Error> {
        match self.counts {
            CountsBack::NotAsked => Ok(KeyUniqueness::NotAsked),
            CountsBack::Counted { rows, distinct } => KeyCounts::parse(rows, distinct)
                .map(KeyUniqueness::Counted)
                .map_err(|_impossible_pair| AdapterFailure::Statement { cause: DriverFailure }),
            CountsBack::Failed => Err(AdapterFailure::Statement { cause: DriverFailure }),
        }
    }
}

/// A fake that can run one half of a federated answer.
///
/// Unlike [`FixedWarehouse`] it declares [`Warehouse::EXECUTES_LEGS`], so the federated path will
/// not refuse it : that is the whole difference, and it is why the port exposes the capability
/// rather than letting `answer` assume. Each instance holds one scripted result and answers any
/// statement with it, which is enough to exercise the orchestrator above real legs - the combiner's
/// own correctness is proven in the domain suite against the same plan shapes this feeds it.
pub(crate) struct LegsWarehouse {
    source: SourceName,
    posture: SourcePosture,
    result: RowSet,
}

impl LegsWarehouse {
    pub(crate) fn answering(source: SourceName, posture: SourcePosture, result: RowSet) -> Self {
        Self { source, posture, result }
    }
}

impl Warehouse for LegsWarehouse {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn dry_run(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<PreFlight, Self::Error> {
        Ok(PreFlight::NotAsked)
    }

    fn execute(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
        Ok(self.result.clone())
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Ok(AnchorRows::of(self.result.clone()))
    }
}

/// A leg-executing fake whose `execute` fails because the data system would not return the whole
/// result at once.
///
/// The instrument for the federated half of the volume bound: the same reach
/// [`LegsWarehouse`] gives (it declares [`Warehouse::EXECUTES_LEGS`], so the federated path runs it),
/// but its `execute` returns `Err` and its [`Warehouse::result_did_not_fit`] answers `true`, so
/// `execute_leg` must turn it into a [`RefusalReason::ResultTooLarge`] carrying
/// [`ResultBound::Volume`] and never into the `503` a dead data system produces. `federated.rs`'s
/// `a_federated_leg_that_hits_the_volume_bound_is_refused_not_a_503` pins that.
pub(crate) struct PageBoundLegsWarehouse {
    source: SourceName,
    posture: SourcePosture,
}

impl PageBoundLegsWarehouse {
    pub(crate) fn new(source: SourceName, posture: SourcePosture) -> Self {
        Self { source, posture }
    }
}

impl Warehouse for PageBoundLegsWarehouse {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn dry_run(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<PreFlight, Self::Error> {
        Ok(PreFlight::NotAsked)
    }

    fn execute(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
        Err(AdapterFailure::TooMuchData)
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(AdapterFailure::TooMuchData)
    }

    fn result_did_not_fit(&self, error: &Self::Error) -> bool {
        matches!(error, AdapterFailure::TooMuchData)
    }
}

/// A mono fake whose `execute` fails because the DATA SYSTEM refused the statement at the
/// identity/authorization level.
///
/// The instrument for the query-time [`Warehouse::source_refused`] predicate on the mono path: it
/// registers under [`source()`](Self::source), answers the pre-flight, and then `execute` returns
/// `Err` whose [`Warehouse::source_refused`] answers `true`, so `answer` must turn it into a
/// [`RefusalReason::SourceRefused`] and never into the `503` a dead data system produces -
/// `sutura_app`'s suite pins that the same way `working_set_exhausted` and `result_did_not_fit`
/// pin their predicates.
pub(crate) struct RefusingSourceWarehouse {
    source: SourceName,
    posture: SourcePosture,
}

impl RefusingSourceWarehouse {
    pub(crate) fn new(source: SourceName, posture: SourcePosture) -> Self {
        Self { source, posture }
    }
}

impl Warehouse for RefusingSourceWarehouse {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn dry_run(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<PreFlight, Self::Error> {
        Ok(PreFlight::NotAsked)
    }

    fn execute(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
        Err(AdapterFailure::RefusedBySource)
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(AdapterFailure::RefusedBySource)
    }

    fn source_refused(&self, error: &Self::Error) -> bool {
        matches!(error, AdapterFailure::RefusedBySource)
    }
}

/// The federated half of [`RefusingSourceWarehouse`]: a leg-executing fake whose `execute` refuses
/// the statement at the identity/authorization level.
///
/// The same reach [`PageBoundLegsWarehouse`] gives (it declares `EXECUTES_LEGS`, so the federated
/// path runs it), but its error answers [`Warehouse::source_refused`] `true`, so `execute_leg` must
/// turn it into a [`RefusalReason::SourceRefused`] rather than a [`LegError::Failure`] - the `503`
/// an outage produces. `federated.rs`'s `a_federated_leg_the_source_refuses_is_refused_not_a_503`
/// pins that.
pub(crate) struct RefusingLegsWarehouse {
    source: SourceName,
    posture: SourcePosture,
}

impl RefusingLegsWarehouse {
    pub(crate) fn new(source: SourceName, posture: SourcePosture) -> Self {
        Self { source, posture }
    }
}

impl Warehouse for RefusingLegsWarehouse {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const EXECUTES_LEGS: bool = true;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn dry_run(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<PreFlight, Self::Error> {
        Ok(PreFlight::NotAsked)
    }

    fn execute(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
        Err(AdapterFailure::RefusedBySource)
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(AdapterFailure::RefusedBySource)
    }

    fn source_refused(&self, error: &Self::Error) -> bool {
        matches!(error, AdapterFailure::RefusedBySource)
    }
}

/// A mono fake whose pre-flight passes but whose `execute` fails with a failure the data system did
/// NOT refuse.
///
/// The reverse-direction instrument for [`Warehouse::source_refused`]: the failure reaches the same
/// classification branch as a source refusal (it gets past `dry_run`), but `source_refused` answers
/// `false` - the port's own default - so `answer` must let it leave as the retryable
/// [`crate::ServiceError::Warehouse`] and never turn a transient failure into a refusal.
pub(crate) struct TransientlyBrokenWarehouse {
    source: SourceName,
    posture: SourcePosture,
}

impl TransientlyBrokenWarehouse {
    pub(crate) fn new(source: SourceName, posture: SourcePosture) -> Self {
        Self { source, posture }
    }
}

impl Warehouse for TransientlyBrokenWarehouse {
    type Error = AdapterFailure;

    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    fn dry_run(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<PreFlight, Self::Error> {
        Ok(PreFlight::NotAsked)
    }

    fn execute(&self, _executable: Executable<'_>, _presented: &Presented) -> Result<RowSet, Self::Error> {
        Err(AdapterFailure::Statement { cause: DriverFailure })
    }

    fn verify_anchor(&self, _plan: sutura_domain::plan::AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(AdapterFailure::Statement { cause: DriverFailure })
    }
}

/// The credential brokers this crate's own tests mint with.
///
/// **One behaviour per variant rather than one fake with flags**, because each is a different thing
/// to assert and a boolean-configured fake makes the test read as a configuration rather than as a
/// case. Together they provoke every outcome `answer` can reach through the port: an answer, the
/// one refusal, the broker's own failure, a broker whose grant an adapter cannot use, and each of
/// the four ways a grant can disagree with the request it came back for.
///
/// **The last three are the reproductions a review handed us**, and they are fixtures rather than
/// hypotheticals for that reason: each one was demonstrated to be answered before the guard existed.
pub(crate) enum FixedBroker {
    /// Grants the deployment's own identity for whatever it is asked about.
    GrantsShared,
    /// Grants nothing for any source, which is the refusal.
    RefusesEverything,
    /// Cannot be reached at all - the `Err` side, which must not become a refusal.
    Unreachable,
    /// Grants the asker's own credential material, which a shared-only adapter cannot use.
    GrantsSubjectMaterial,
    /// Grants a perfectly good credential for a source nobody asked about.
    ///
    /// It builds its own `SourceSet`, which is what makes this reachable: `LegCredentials`
    /// refuses a set that does not cover the sources it was minted FOR, and this one covers the
    /// set it invented rather than the one it was handed.
    GrantsTheWrongSource,
    /// Grants a credential minted for somebody else than the subject that asked.
    ///
    /// **The confused deputy.** `LegCredentials::minted` is `pub` and takes any `Subject`, so this
    /// is a value a real broker can return by defect or under compromise - and before the guard the
    /// question was answered with it, while the audit record named the asker.
    GrantsAnotherSubjectsCredential,
    /// Grants a credential whose deadline is the Unix epoch: already expired, for every clock.
    GrantsSomethingAlreadyExpired,
    /// Grants a credential that expires within the second, so a slow pre-flight outlives it.
    ///
    /// **The only way to reach the SECOND deadline check**, which is the one that can fire in
    /// production: the check after minting runs microseconds later, and this one runs after a
    /// pre-flight. Paired with `FixedWarehouse::answering_after`, whose pre-flight takes longer than
    /// the second this grant has left.
    GrantsSomethingExpiringWithinTheSecond,
    /// Refuses a source that was never in the set it was asked about.
    ///
    /// Before the guard this became a caller-facing `CredentialUnavailable` naming a source the
    /// caller never asked for - a broker defect reported as the caller's own lack of access.
    RefusesASourceNobodyAsked,
}

/// The broker's own failure, which is not a refusal and not a data system's.
#[derive(Debug, thiserror::Error)]
#[error("the authorization server could not be reached")]
pub(crate) struct BrokerUnreachable;

impl CredentialBroker for FixedBroker {
    type Error = BrokerUnreachable;

    #[expect(
        clippy::unwrap_in_result,
        reason = "every literal here is a fixture constant, so a failure to parse one is a broken test \
                  rather than an input to handle"
    )]
    fn mint(&self, context: &RequestContext, sources: &SourceSet) -> Result<Minted, Self::Error> {
        let asked_by = context.chain().subject().clone();
        let elsewhere = SourceName::parse("elsewhere").expect("a fixture source is a source");
        let grant = match *self {
            Self::Unreachable => return Err(BrokerUnreachable),
            Self::RefusesEverything => {
                let Some(first) = sources.iter().next() else {
                    return Err(BrokerUnreachable);
                };
                return Ok(Minted::Refused { source: first.clone() });
            }
            // A source that is not in `sources`, which is the whole of the defect: the name reaching
            // the caller as a refusal was never asked about.
            Self::RefusesASourceNobodyAsked => return Ok(Minted::Refused { source: elsewhere }),
            Self::GrantsShared => Grant::of(asked_by, sources.clone(), shared_leg),
            Self::GrantsSubjectMaterial => Grant::of(asked_by, sources.clone(), subject_leg),
            Self::GrantsTheWrongSource => Grant::of(asked_by, SourceSet::of(elsewhere), shared_leg),
            // Somebody else's grant. The deployment itself rather than a second person, because it
            // is the value a real mapping defect produces - a broker that fell back to its own
            // identity - and because the suite's context is always a verified person, so the two
            // subjects differ in the way that matters.
            Self::GrantsAnotherSubjectsCredential => Grant::of(Subject::TheDeploymentItself, sources.clone(), shared_leg),
            Self::GrantsSomethingAlreadyExpired => Grant {
                not_after: Expiry::At { unix_seconds: 0 },
                ..Grant::of(asked_by, sources.clone(), shared_leg)
            },
            // The next whole second. It has not passed when this returns, and it has passed once a
            // pre-flight longer than a second has run - which is what makes the second check
            // provokable without a clock a test can advance.
            Self::GrantsSomethingExpiringWithinTheSecond => Grant {
                not_after: Expiry::At {
                    unix_seconds: now_in_unix_seconds().saturating_add(1),
                },
                ..Grant::of(asked_by, sources.clone(), shared_leg)
            },
        };
        let mut presented = BTreeMap::new();
        for name in grant.minted_for.iter() {
            drop(presented.insert(name.clone(), (grant.material)()));
        }
        // The `map_err` arm is unreachable: the map is built from `minted_for`, so it covers it.
        // Answered rather than unwrapped, because this is not a test body.
        let credentials = LegCredentials::minted(grant.asked_by, grant.not_after, &grant.minted_for, presented)
            .map_err(|_uncoverable| BrokerUnreachable)?;
        Ok(Minted::Granted { credentials })
    }
}

/// What one arm of [`FixedBroker::mint`] decided.
///
/// **A struct rather than the tuple this used to be**, because the arms now differ in the subject and
/// the deadline as well as in the source set and the material - and a four-tuple is over this
/// workspace's `type_complexity` threshold as well as unreadable at the call site. [`Grant::of`] is
/// the ordinary case, and the one arm that differs says which field it is changing.
struct Grant {
    asked_by: Subject,
    not_after: Expiry,
    minted_for: SourceSet,
    material: fn() -> Presented,
}

impl Grant {
    /// A grant for the asker, with no deadline: what every arm but one wants.
    const fn of(asked_by: Subject, minted_for: SourceSet, material: fn() -> Presented) -> Self {
        Self {
            asked_by,
            not_after: Expiry::NothingExpires,
            minted_for,
            material,
        }
    }
}

/// The one acknowledgement witness this crate's fixtures use.
///
/// **One definition, read by both the posture a fake warehouse is opened with and the leg the fake
/// broker mints**, because `answer` now compares them: a shared leg carrying a *different* operator
/// acknowledgement is a wiring defect, and two fixtures writing two sentences would make every test
/// here provoke it. It is the same reason the golden matrix reads its leg off its posture.
pub(crate) fn acknowledged() -> SharedIdentityDeclared {
    SharedIdentityDeclared::of(
        AcknowledgementReason::parse("a fake over no data system, in this process").expect("a fixture reason is a reason"),
    )
}

/// The posture every fake warehouse in this crate's suite is opened with, unless a test is about the
/// other one.
pub(crate) fn shared_posture() -> SourcePosture {
    SourcePosture::SharedServiceUser {
        declared: acknowledged(),
    }
}

/// The deployment's own identity for a source, carrying the witness above.
fn shared_leg() -> Presented {
    Presented::SharedServiceUser {
        declared: acknowledged(),
    }
}

/// Now, in whole seconds, for the one fixture that mints a credential with a lifetime.
///
/// A fixture may read a clock where the domain may not: what it is standing in for is a broker that
/// exchanged a token, and such a broker holds one.
fn now_in_unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The asker's own credential, which no adapter in this workspace can use.
fn subject_leg() -> Presented {
    Presented::SubjectToken {
        material: Secret::new("an-exchanged-token"),
    }
}

/// A broker that counts how many times it was asked, and grants the shared posture.
///
/// **For the one assertion a granting broker cannot make: that it was NOT called.** `mint` is on the
/// request path, once per accepted question, and it is free only for the implementor that reads a
/// settings tree - a broker that exchanges a token spends an authorization-server round trip. So what
/// is worth pinning is that a question this deployment declines never gets that far, and a fake that
/// only answers cannot say whether it was asked.
///
/// `Cell` rather than a lock: `answer` puts no `Sync` bound on the broker, this test writes from one
/// thread, and the workspace bans `std::sync::Mutex`.
#[derive(Debug, Default)]
pub(crate) struct CountingBroker {
    asked: std::cell::Cell<usize>,
}

impl CountingBroker {
    /// How many questions reached the authorization server this stands in for.
    pub(crate) fn asked(&self) -> usize {
        self.asked.get()
    }
}

impl CredentialBroker for CountingBroker {
    type Error = BrokerUnreachable;

    fn mint(&self, context: &RequestContext, sources: &SourceSet) -> Result<Minted, Self::Error> {
        self.asked.set(self.asked.get().saturating_add(1));
        let mut presented = BTreeMap::new();
        for name in sources.iter() {
            drop(presented.insert(name.clone(), shared_leg()));
        }
        // The `map_err` arm is unreachable: the map is built from `sources`. Named rather than `_`,
        // because `map_err_ignore` is denied and discarding a cause is what that lint is for.
        LegCredentials::minted(context.chain().subject().clone(), Expiry::NothingExpires, sources, presented)
            .map(|credentials| Minted::Granted { credentials })
            .map_err(|_uncoverable| BrokerUnreachable)
    }
}

/// A broker that grants each source the shared witness that source's OWN operator wrote.
///
/// **The fake a two-source shared deployment needs, and the reason is a mechanism rather than
/// convenience.** `Presented::agrees_with` compares the witness against the posture the adapter was
/// handed by *equality on the prose*, and every other fake here mints one fixed witness for every
/// source - which is correct while one acknowledgement is in play and refuses the moment two sources
/// carry two sentences. `StaticCredentialBroker` is per-source for the same reason.
///
/// Built from the postures the registry was given, so a test cannot arrange for the broker and the
/// adapters to disagree by accident. A source it was not told about gets the deployment's own
/// identity, which is what `LegCredentials::minted` needs to cover the set it was asked for.
pub(crate) struct AcknowledgingBroker {
    /// The WITNESS per source rather than the `Presented` it becomes. `Presented` is deliberately
    /// not `Clone` - it carries a `Secret` in another variant - so the leg is built inside `mint`,
    /// which is where a real broker builds one anyway.
    by_source: BTreeMap<SourceName, SharedIdentityDeclared>,
}

impl AcknowledgingBroker {
    /// One entry per source, from that source's own declared posture.
    ///
    /// An impersonating source contributes nothing, so this fake stays the *shared* broker it says
    /// it is and such a source falls through to the fixture's own witness - which is what its leg
    /// is then refused for by `agrees_with`, not by this.
    pub(crate) fn over(postures: &[(SourceName, SourcePosture)]) -> Self {
        let mut by_source = BTreeMap::new();
        for (source, posture) in postures {
            if let SourcePosture::SharedServiceUser { ref declared } = *posture {
                drop(by_source.insert(source.clone(), declared.clone()));
            }
        }
        Self { by_source }
    }
}

impl CredentialBroker for AcknowledgingBroker {
    type Error = BrokerUnreachable;

    fn mint(&self, context: &RequestContext, sources: &SourceSet) -> Result<Minted, Self::Error> {
        let mut presented = BTreeMap::new();
        for name in sources.iter() {
            let leg = self
                .by_source
                .get(name)
                .map_or_else(shared_leg, |declared| Presented::SharedServiceUser {
                    declared: declared.clone(),
                });
            drop(presented.insert(name.clone(), leg));
        }
        // The `map_err` arm is unreachable: the map is built from `sources`, so it covers it.
        LegCredentials::minted(context.chain().subject().clone(), Expiry::NothingExpires, sources, presented)
            .map(|credentials| Minted::Granted { credentials })
            .map_err(|_uncoverable| BrokerUnreachable)
    }
}

/// One model as a catalog document names it: the model, its data system, its table.
pub(crate) type DeclaredModel<'raw> = (&'raw str, &'raw str, &'raw str);

/// A pinned bundle over exactly the models given, and no metrics.
///
/// **Harness and not an assertion**, which is why it is in this file rather than beside the suite
/// that reads it: `cargo xtask test-causality` never reverts a file that adds a `#[test]`, so moving
/// a builder here keeps the assertions in the file whose behaviour they are about.
///
/// Models are all a pre-flight reads - `crate::preflight::ask` maps over them and asks per table -
/// so a metric would add nothing it looks at. Two entries naming one table is the shape
/// `crate::preflight::AbsentBehind`'s value being a set exists for, and this builder permits it.
pub(crate) fn bundle_over(models: &[DeclaredModel<'_>]) -> sutura_domain::pinned::PinnedDefinitions {
    use std::collections::BTreeSet;

    use sutura_domain::capabilities::MetadataCapabilities;
    use sutura_domain::catalog::{Definitions, Description, Model};
    use sutura_domain::knowledge::Knowledge;
    use sutura_domain::model::{ColumnName, ModelName, TableName};
    use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};

    let declared: Vec<Model> = models
        .iter()
        .map(|&(model, source, table)| {
            Model::new(
                ModelName::parse(model).expect("a test model is a model"),
                SourceName::parse(source).expect("a test source is a source"),
                TableName::parse(table).expect("a test table is a table"),
                BTreeSet::from([ColumnName::parse("customer_key").expect("a test column is a column")]),
                Description::default(),
            )
        })
        .collect();
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a test version is a version"),
        Definitions::assemble(declared, vec![], vec![]).expect("the test bundle is consistent"),
        Knowledge::none(),
        ContributionManifest::single(
            SourceName::parse("local").expect("a test source is a source"),
            Contribution::of(MetadataCapabilities::nothing()),
        ),
    )
    .expect("the test definitions hash")
}
