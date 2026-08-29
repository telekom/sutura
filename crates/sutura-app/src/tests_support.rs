//! The fakes this crate's own unit tests share: a data system, and the credential brokers.
//!
//! In its own file for the reason `crate::prompt`'s suite is: `lib.rs` reached the 1000-line gate,
//! and the gate's answer to that is to split the file rather than to shorten what is documented.
//!
//! A module rather than a copy per test module, because [`crate::warehouses`] and `crate::tests` both
//! need a `Warehouse` that declares a posture and executes nothing interesting - and two copies of
//! one fake is two things to keep in step with the port.

use std::collections::BTreeMap;

use sutura_domain::identity::{CredentialBroker, Expiry, LegCredentials, Minted, Presented, RequestContext, Secret, SourceSet};
use sutura_domain::model::SourceName;
use sutura_domain::plan::Executable;
use sutura_domain::source::{AcknowledgementReason, ImpersonationCapability, SharedIdentityDeclared, SourcePosture};
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
}

impl FixedWarehouse {
    /// One that fails every statement, with a cause worth reading.
    pub(crate) const fn new(source: SourceName, posture: SourcePosture) -> Self {
        Self {
            source,
            posture,
            result: None,
        }
    }

    /// One that answers every statement with `result`.
    pub(crate) const fn answering(source: SourceName, posture: SourcePosture, result: RowSet) -> Self {
        Self {
            source,
            posture,
            result: Some(result),
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
        match self.result {
            Some(_) => Ok(PreFlight::NotAsked),
            None => Err(AdapterFailure::Statement { cause: DriverFailure }),
        }
    }

    fn execute(&self, _executable: Executable<'_>, presented: &Presented) -> Result<RowSet, Self::Error> {
        self.deliverable(presented)?;
        self.result.clone().ok_or(AdapterFailure::Statement { cause: DriverFailure })
    }

    fn verify_anchor(&self, _plan: &sutura_domain::plan::QueryPlan) -> Result<AnchorRows, Self::Error> {
        self.result
            .clone()
            .map(AnchorRows::of)
            .ok_or(AdapterFailure::Statement { cause: DriverFailure })
    }
}

/// The credential brokers this crate's own tests mint with.
///
/// **Four behaviours rather than one fake with flags**, because each is a different thing to
/// assert and a boolean-configured fake makes the test read as a configuration rather than as a
/// case. Together they provoke every outcome `answer` can reach through the port: an answer, the
/// one refusal, the broker's own failure, a broker whose grant does not cover the plan, and a
/// broker whose grant an adapter cannot use.
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
        let (minted_for, material): Grant = match *self {
            Self::Unreachable => return Err(BrokerUnreachable),
            Self::RefusesEverything => {
                let Some(first) = sources.iter().next() else {
                    return Err(BrokerUnreachable);
                };
                return Ok(Minted::Refused { source: first.clone() });
            }
            Self::GrantsShared => (sources.clone(), shared_leg),
            Self::GrantsSubjectMaterial => (sources.clone(), subject_leg),
            Self::GrantsTheWrongSource => (SourceSet::of(elsewhere), shared_leg),
        };
        let mut presented = BTreeMap::new();
        for name in minted_for.iter() {
            drop(presented.insert(name.clone(), material()));
        }
        // The `map_err` arm is unreachable: the map is built from `minted_for`, so it covers it.
        // Answered rather than unwrapped, because this is not a test body.
        let credentials = LegCredentials::minted(asked_by, Expiry::NothingExpires, &minted_for, presented)
            .map_err(|_uncoverable| BrokerUnreachable)?;
        Ok(Minted::Granted { credentials })
    }
}

/// What one arm of [`FixedBroker::mint`] decided: which sources to mint for, and what to present.
///
/// Named because the tuple is over this workspace's `type_complexity` threshold, and naming it is the
/// better half of that trade - the pair IS the decision each arm makes.
type Grant = (SourceSet, fn() -> Presented);

/// The deployment's own identity for a source, with a reason a fixture wrote.
fn shared_leg() -> Presented {
    Presented::SharedServiceUser {
        declared: SharedIdentityDeclared::of(
            AcknowledgementReason::parse("a fake over no data system, in this process").expect("a fixture reason is a reason"),
        ),
    }
}

/// The asker's own credential, which no adapter in this workspace can use.
fn subject_leg() -> Presented {
    Presented::SubjectToken {
        material: Secret::new("an-exchanged-token"),
    }
}
