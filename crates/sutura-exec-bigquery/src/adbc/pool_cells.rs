//! The pool exchange as this transport consumes it, over fakes standing in for Google's token
//! service: what a granted exchange puts on the database, that a refused one opens nothing, and how
//! a refusal reaches the caller.

use std::sync::OnceLock;

use sutura_domain::identity::{Delegated, PoolExchange, PoolExchangeFailed, PrincipalName, Secret};

use super::identity::authenticate;
use super::subject::WorkloadPool;
use super::{AdbcBigQuery, AdbcError, BytesBilledCeiling, DelegatingAccount, DriverLocation, Impersonation};
use crate::transport::{JobDeadline, JobIdentity, JobRequest, JobTransport as _, ProjectId};

const POOL: &str = "//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/a/providers/sso";
const ASSERTION: &str = "a.caller.assertion";
const ISSUED: &str = "ya29.the-pools-own-token";

/// What one exchange was shown.
#[derive(Debug, PartialEq, Eq)]
struct Shown {
    assertion: String,
    audience: String,
}

/// Google's token service granting the one exchange a cell makes, and recording what it was shown.
/// A second exchange through one fake is refused, so a cell cannot read a stale record.
#[derive(Debug, Default)]
struct Granting {
    shown: OnceLock<Shown>,
}

impl PoolExchange for Granting {
    #[expect(
        clippy::disallowed_methods,
        reason = "a fake records the assertion it was shown so a cell can compare it"
    )]
    fn exchange(&self, assertion: &Secret, pool: &WorkloadPool) -> Result<Delegated, PoolExchangeFailed> {
        let shown = Shown {
            assertion: assertion.expose_secret().to_owned(),
            audience: pool.audience().to_owned(),
        };
        if self.shown.set(shown).is_err() {
            return Err(PoolExchangeFailed::Malformed {
                what: "a second exchange through one fake",
            });
        }
        Ok(Delegated::new(Secret::new(ISSUED), u64::MAX))
    }
}

/// Google's token service refusing every assertion as `invalid_grant`.
#[derive(Debug)]
struct Refusing;

impl PoolExchange for Refusing {
    fn exchange(&self, _: &Secret, _: &WorkloadPool) -> Result<Delegated, PoolExchangeFailed> {
        Err(PoolExchangeFailed::Refused {
            status: 400,
            error: Some(String::from("invalid_grant")),
        })
    }
}

fn pool() -> Impersonation {
    Impersonation::ThroughPool(WorkloadPool::parse(POOL).expect("a provider resource is usable"))
}

fn project() -> ProjectId {
    ProjectId::parse("acme-analytics").expect("a test project is a project")
}

/// The real transport over a path naming no `.so`, so reaching the driver is a load failure.
fn endpoint() -> AdbcBigQuery {
    AdbcBigQuery::new(
        DriverLocation::parse("/nonexistent/libadbc_driver_bigquery.so")
            .expect("an absolute path parses whether or not a file is there"),
        pool(),
        BytesBilledCeiling::parse(1024 * 1024 * 1024).expect("a gibibyte is a usable ceiling"),
    )
}

/// One subject's question through `transport`, as far as it gets.
fn asked(transport: &AdbcBigQuery) -> AdbcError {
    let assertion = Secret::new(ASSERTION);
    let project = project();
    let request = JobRequest::new(
        "SELECT 1",
        &[],
        &project,
        JobIdentity::AsSubject { assertion: &assertion },
        JobDeadline::Boot,
    );
    transport.run(&request).map(|_| ()).expect_err("no driver lives at this path")
}

#[test]
fn a_subject_at_a_pool_hands_the_driver_the_exchanged_token_and_nothing_else() {
    let granting = Granting::default();
    let assertion = Secret::new(ASSERTION);
    let sent: Vec<(String, String)> = authenticate(JobIdentity::AsSubject { assertion: &assertion }, &pool(), Some(&granting))
        .expect("a granted exchange authenticates the subject")
        .options
        .into_iter()
        .map(|(key, value)| (format!("{key:?}"), format!("{value:?}")))
        .collect();
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert!(
        sent.iter()
            .any(|(key, value)| key.contains("\"bigquery.auth_type\"") && value.contains("\"access_token\"")),
        "{sent:?}"
    );
    assert!(
        sent.iter()
            .any(|(key, value)| key.contains("\"bigquery.auth.access_token\"") && value.contains(ISSUED)),
        "{sent:?}"
    );
    for gone in [
        "json_credential_string",
        "external_account",
        "bigquery.auth.credentials",
        ASSERTION,
    ] {
        assert!(
            !sent.iter().any(|(key, value)| key.contains(gone) || value.contains(gone)),
            "{gone} reaches the driver: {sent:?}"
        );
    }
    assert_eq!(
        granting.shown.get(),
        Some(&Shown {
            assertion: String::from(ASSERTION),
            audience: String::from(POOL),
        })
    );
}

#[test]
fn a_refused_exchange_is_a_refusal_and_no_option_list() {
    let assertion = Secret::new(ASSERTION);
    let refused = authenticate(JobIdentity::AsSubject { assertion: &assertion }, &pool(), Some(&Refusing))
        .map(|authentication| authentication.options.len())
        .expect_err("a refused exchange authenticates nobody");
    assert!(
        matches!(
            refused,
            AdbcError::PoolExchange {
                cause: PoolExchangeFailed::Refused { status: 400, .. }
            }
        ),
        "{refused:?}"
    );
    assert!(!format!("{refused} {refused:?}").contains(ASSERTION), "{refused:?}");
}

#[test]
fn a_refused_exchange_stops_the_question_before_the_driver_loads() {
    let refused = asked(&endpoint().exchanging_through(Box::new(Refusing)));
    assert!(matches!(refused, AdbcError::PoolExchange { .. }), "{refused:?}");
}

#[test]
fn a_granted_exchange_gets_the_question_as_far_as_the_driver() {
    // The control for the cell above: a transport that refused every subject for any reason would
    // pass it.
    let failed = asked(&endpoint().exchanging_through(Box::<Granting>::default()));
    assert!(matches!(failed, AdbcError::Load(_)), "{failed:?}");
}

#[test]
fn only_a_refusal_of_the_assertion_reaches_the_caller_as_source_refused() {
    let transport = endpoint();
    let refused = |status: u16, error: Option<&str>| AdbcError::PoolExchange {
        cause: PoolExchangeFailed::Refused {
            status,
            error: error.map(String::from),
        },
    };
    for error in [refused(401, None), refused(403, None), refused(400, Some("invalid_grant"))] {
        assert!(transport.job_was_refused(&error), "{error:?}");
    }
    for error in [
        refused(400, Some("invalid_request")),
        refused(500, None),
        refused(503, Some("temporarily_unavailable")),
        AdbcError::PoolExchange {
            cause: PoolExchangeFailed::Unreachable {
                cause: Box::new(std::io::Error::other("connection refused")),
            },
        },
        AdbcError::PoolExchange {
            cause: PoolExchangeFailed::TooLarge { cap: 16 },
        },
        AdbcError::PoolExchange {
            cause: PoolExchangeFailed::WrongTokenType,
        },
        AdbcError::PoolExchange {
            cause: PoolExchangeFailed::Malformed { what: "not JSON" },
        },
    ] {
        assert!(!transport.job_was_refused(&error), "{error:?}");
    }
}

/// The location decision, held: whoever a job runs as, its database is opened with no
/// `bigquery.location`, so `BigQuery` takes the location from the tables a statement reads.
#[test]
fn no_database_option_names_a_location() {
    let assertion = Secret::new(ASSERTION);
    let address = PrincipalName::parse("analyst@example.com").expect("a test name is a name");
    let delegating = Impersonation::DomainDelegation(
        DelegatingAccount::parse("dwd@acme-analytics.iam.gserviceaccount.com").expect("a service-account address"),
    );
    let granting = Granting::default();
    for (identity, impersonation) in [
        (JobIdentity::Transport, Impersonation::Disabled),
        (JobIdentity::AsSubject { assertion: &assertion }, pool()),
        (JobIdentity::AsSubject { assertion: &assertion }, Impersonation::GoogleLogin),
        (JobIdentity::AsAddress { address: &address }, delegating),
    ] {
        let runs_as = authenticate(identity, &impersonation, Some(&granting))
            .expect("each pair is one this transport runs")
            .options;
        let keys: Vec<String> = super::database_options(&project(), runs_as)
            .into_iter()
            .map(|(key, _)| format!("{key:?}"))
            .collect();
        assert!(keys.iter().any(|key| key.contains("bigquery.project_id")), "{keys:?}");
        assert!(
            !keys.iter().any(|key| key.contains("location")),
            "{impersonation:?}: a location is sent: {keys:?}"
        );
    }
}

/// What the pool exchange is shown when a delegation chain hands this adapter `token`: this
/// adapter's row in the delegation packs, since the token service is where a subject's token goes.
fn delivered(token: &Secret) -> Vec<u8> {
    let granting = Granting::default();
    authenticate(JobIdentity::AsSubject { assertion: token }, &pool(), Some(&granting))
        .map(drop)
        .expect("a granted exchange authenticates the subject");
    granting
        .shown
        .into_inner()
        .map_or_default(|shown| shown.assertion.into_bytes())
}

sutura_conformance::delegation_packs! {
    adapter: bigquery,
    deliver: super::delivered,
}
