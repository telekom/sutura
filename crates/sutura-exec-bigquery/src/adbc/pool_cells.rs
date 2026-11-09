//! The pool exchange as this transport consumes it, over fakes standing in for Google's token
//! service: what a granted exchange puts on the database, that a refused one opens nothing, and how
//! a refusal reaches the caller. Then a split leg's two exchanges, and the one token a partition
//! read hands the driver.

use core::num::NonZeroUsize;
use core::time::Duration;
use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{SystemTime, UNIX_EPOCH};

use sutura_domain::identity::{Delegated, PoolExchange, PoolExchangeFailed, PoolScope, PrincipalName, Secret};

use super::identity::{DatabaseOptions, authenticate, forwarded, split};
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
    scope: PoolScope,
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
    fn exchange(&self, assertion: &Secret, pool: &WorkloadPool, scope: PoolScope) -> Result<Delegated, PoolExchangeFailed> {
        let shown = Shown {
            assertion: assertion.expose_secret().to_owned(),
            audience: pool.audience().to_owned(),
            scope,
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
    fn exchange(&self, _: &Secret, _: &WorkloadPool, _: PoolScope) -> Result<Delegated, PoolExchangeFailed> {
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
            scope: PoolScope::Query,
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

/// The token [`Scoped`] issues for [`PoolScope::Query`]: the job's own.
const QUERY_TOKEN: &str = "ya29.the-jobs-own-token";

/// The token [`Scoped`] issues for [`PoolScope::ReadOnly`]: the forwarded one.
const READ_ONLY_TOKEN: &str = "ya29.the-forwarded-read-only-token";

/// Google's token service issuing each exchange a token named for the scope it was asked for, the
/// read-only one expiring at `not_after` and the job's at `query_not_after`, and refusing
/// [`PoolScope::ReadOnly`] where `read_only_refused`. It sends each scope it is asked for, in order,
/// to the receiver [`scoped_at`] hands back beside it.
#[derive(Debug)]
struct Scoped {
    not_after: u64,
    query_not_after: u64,
    read_only_refused: bool,
    scopes: Sender<PoolScope>,
}

/// A [`Scoped`] whose read-only token expires at `not_after` and whose job token ten minutes later,
/// so a split that read the wrong token's expiry answers differently, and where it reports the
/// scopes it was asked for.
fn scoped_at(not_after: u64) -> (Scoped, Receiver<PoolScope>) {
    let (scopes, reported) = channel();
    (
        Scoped {
            not_after,
            query_not_after: not_after.saturating_add(600),
            read_only_refused: false,
            scopes,
        },
        reported,
    )
}

impl PoolExchange for Scoped {
    fn exchange(&self, _: &Secret, _: &WorkloadPool, scope: PoolScope) -> Result<Delegated, PoolExchangeFailed> {
        self.scopes.send(scope).map_err(|_closed| PoolExchangeFailed::Malformed {
            what: "the cell stopped listening",
        })?;
        let (token, not_after) = match scope {
            PoolScope::Query => (QUERY_TOKEN, self.query_not_after),
            PoolScope::ReadOnly if self.read_only_refused => {
                return Err(PoolExchangeFailed::Refused {
                    status: 403,
                    error: None,
                });
            }
            PoolScope::ReadOnly => (READ_ONLY_TOKEN, self.not_after),
        };
        Ok(Delegated::new(Secret::new(token), not_after))
    }
}

/// When the question a split serves ends, in these cells.
fn question_ends() -> SystemTime {
    UNIX_EPOCH
        .checked_add(Duration::from_secs(2_000_000_000))
        .expect("a 2033 instant is representable")
}

/// `options` as key and value text, the way a cell compares them.
fn option_text(options: DatabaseOptions) -> Vec<(String, String)> {
    options
        .into_iter()
        .map(|(key, value)| (format!("{key:?}"), format!("{value:?}")))
        .collect()
}

#[test]
#[expect(clippy::disallowed_methods, reason = "the cell asserts on the token the split forwards")]
fn a_split_runs_its_job_on_one_token_and_forwards_only_a_read_only_one() {
    let (scoped, reported) = scoped_at(u64::MAX);
    let assertion = Secret::new(ASSERTION);
    let split = split(
        JobIdentity::AsSubject { assertion: &assertion },
        &pool(),
        Some(&scoped),
        question_ends(),
    )
    .expect("a subject at a pool source splits");
    assert_eq!(
        reported.try_iter().collect::<Vec<_>>(),
        [PoolScope::Query, PoolScope::ReadOnly]
    );
    let sent = option_text(split.options);
    assert!(
        sent.iter()
            .any(|(key, value)| key.contains("\"bigquery.auth.access_token\"") && value.contains(QUERY_TOKEN)),
        "{sent:?}"
    );
    assert!(!sent.iter().any(|(_, value)| value.contains(READ_ONLY_TOKEN)), "{sent:?}");
    assert_eq!(split.forwarded.into_token().expose_secret(), READ_ONLY_TOKEN);
}

#[test]
fn a_split_whose_read_only_exchange_is_refused_forwards_nothing() {
    let (scoped, _reported) = scoped_at(u64::MAX);
    let scoped = Scoped {
        read_only_refused: true,
        ..scoped
    };
    let assertion = Secret::new(ASSERTION);
    let refused = split(
        JobIdentity::AsSubject { assertion: &assertion },
        &pool(),
        Some(&scoped),
        question_ends(),
    )
    .map(|_| ())
    .expect_err("a split never forwards the job's own token instead");
    assert!(
        matches!(
            refused,
            AdbcError::PoolExchange {
                cause: PoolExchangeFailed::Refused { status: 403, .. }
            }
        ),
        "{refused:?}"
    );
}

#[test]
fn a_split_reports_the_earlier_of_the_question_end_and_the_forwarded_token_expiry() {
    let assertion = Secret::new(ASSERTION);
    for (not_after, until) in [(1_900_000_000, 1_900_000_000), (2_100_000_000, 2_000_000_000)] {
        let (scoped, _reported) = scoped_at(not_after);
        let split = split(
            JobIdentity::AsSubject { assertion: &assertion },
            &pool(),
            Some(&scoped),
            question_ends(),
        )
        .expect("a subject at a pool source splits");
        assert_eq!(
            split.until,
            UNIX_EPOCH
                .checked_add(Duration::from_secs(until))
                .expect("a test instant is representable"),
            "a token expiring at {not_after}"
        );
    }
}

#[test]
fn a_split_of_anything_but_a_subject_at_a_pool_is_refused_before_any_exchange() {
    let assertion = Secret::new(ASSERTION);
    let address = PrincipalName::parse("analyst@example.com").expect("a test name is a name");
    let delegating = Impersonation::DomainDelegation(
        DelegatingAccount::parse("dwd@acme-analytics.iam.gserviceaccount.com").expect("a service-account address"),
    );
    let granting = Granting::default();
    for (identity, impersonation) in [
        (JobIdentity::Transport, pool()),
        (JobIdentity::AsAddress { address: &address }, pool()),
        (JobIdentity::AsSubject { assertion: &assertion }, Impersonation::Disabled),
        (JobIdentity::AsSubject { assertion: &assertion }, Impersonation::GoogleLogin),
        (JobIdentity::AsAddress { address: &address }, delegating),
    ] {
        let refused = split(identity, &impersonation, Some(&granting), question_ends())
            .map(|_| ())
            .expect_err("only a subject at a pool source splits");
        assert!(matches!(refused, AdbcError::Uncovered(_)), "{impersonation:?}: {refused:?}");
    }
    let refused = split(
        JobIdentity::AsSubject { assertion: &assertion },
        &pool(),
        None,
        question_ends(),
    )
    .map(|_| ())
    .expect_err("a pool no exchange was composed for splits nothing");
    assert!(matches!(refused, AdbcError::Uncovered(_)), "{refused:?}");
    assert_eq!(granting.shown.get(), None, "a refused split exchanged");
}

#[test]
fn a_split_exchanges_twice_before_the_driver_loads() {
    let (scoped, reported) = scoped_at(u64::MAX);
    let transport = endpoint().exchanging_through(Box::new(scoped));
    let assertion = Secret::new(ASSERTION);
    let project = project();
    let request = JobRequest::new(
        "SELECT 1",
        &[],
        &project,
        JobIdentity::AsSubject { assertion: &assertion },
        JobDeadline::Boot,
    );
    let failed = transport
        .split(&request, NonZeroUsize::MIN, question_ends())
        .expect_err("no driver lives at this path");
    assert!(matches!(failed, AdbcError::Load(_)), "{failed:?}");
    assert_eq!(
        reported.try_iter().collect::<Vec<_>>(),
        [PoolScope::Query, PoolScope::ReadOnly]
    );
}

#[test]
fn a_forwarded_token_becomes_the_only_two_auth_options() {
    let token = Secret::new(READ_ONLY_TOKEN);
    let sent = option_text(
        forwarded(&token, &pool())
            .expect("a pool source reads a partition with a forwarded token")
            .options,
    );
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert!(
        sent.iter()
            .any(|(key, value)| key.contains("\"bigquery.auth_type\"") && value.contains("\"access_token\"")),
        "{sent:?}"
    );
    assert!(
        sent.iter()
            .any(|(key, value)| key.contains("\"bigquery.auth.access_token\"") && value.contains(READ_ONLY_TOKEN)),
        "{sent:?}"
    );
}

#[test]
fn a_forwarded_token_is_refused_at_a_source_that_federates_at_no_pool() {
    let token = Secret::new(READ_ONLY_TOKEN);
    let delegating = Impersonation::DomainDelegation(
        DelegatingAccount::parse("dwd@acme-analytics.iam.gserviceaccount.com").expect("a service-account address"),
    );
    for impersonation in [Impersonation::Disabled, Impersonation::GoogleLogin, delegating] {
        let refused = forwarded(&token, &impersonation)
            .map(|_| ())
            .expect_err("only a source that federates at a pool reads a forwarded token");
        assert!(matches!(refused, AdbcError::Uncovered(_)), "{impersonation:?}: {refused:?}");
    }
}

#[test]
fn a_partition_read_at_a_pool_source_gets_as_far_as_the_driver() {
    let failed = endpoint()
        .read_partition(&project(), &Secret::new(READ_ONLY_TOKEN), b"a stream", JobDeadline::Boot)
        .map(|_| ())
        .expect_err("no driver lives at this path");
    assert!(matches!(failed, AdbcError::Load(_)), "{failed:?}");
}
