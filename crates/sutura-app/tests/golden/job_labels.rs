//! **A `BigQuery` job carries its caller's labels, and is recorded under the principal it ran as**,
//! read back from `INFORMATION_SCHEMA.JOBS`: the live half of what the adapter's offline cells hold
//! against a fake statement.
//!
//! The label and shared-principal cells run in the `bigquery-conformance` CI job. The reader is the
//! job's one CI identity, which needs `bigquery.jobs.listAll` on the billing project to see every
//! principal's jobs, and the job's environment names the view's region in `SUTURA_BQ_REGION`. The
//! `domain_delegation` principal cell is deliberately ignored until owner-provided delegation
//! infrastructure exists; its adapter option construction and principal switch remain covered
//! offline.

use std::time::{Duration, SystemTime};

use sutura_domain::identity::{
    Actor, ActorChain, BillTo, ClientClaim, ClientClaims, CredentialBroker, Expiry, LegCredentials, Minted, Presented,
    PrincipalChain, PrincipalName, RequestContext, SourceSet, Subject,
};
use sutura_domain::query::ToolOutcome;
use sutura_domain::source::SourcePosture;
use sutura_domain::warehouse::labels::JobLabels;
use sutura_domain::warehouse::{RowSet, Value};
use sutura_exec_bigquery::adbc::{BytesBilledCeiling, DelegatingAccount, DriverLocation, Impersonation};
use sutura_exec_bigquery::{JobBilling, JobsRegion};

use crate::adapters::{load_on, open, questions, read_question, runs_here, stem};

type BigQuery = sutura_exec_bigquery::BigQueryWarehouse<sutura_exec_bigquery::adbc::AdbcBigQuery>;

/// The value of `name`, or a panic naming it and what it is for.
fn required(name: &str, for_what: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| panic!("{name} is unset: the bq-test environment names no {for_what}"))
}

fn region() -> JobsRegion {
    let region = required(
        "SUTURA_BQ_REGION",
        "region for INFORMATION_SCHEMA.JOBS, so the job read-back cannot run",
    );
    JobsRegion::parse(&region).unwrap_or_else(|e| panic!("SUTURA_BQ_REGION: {e}"))
}

/// A context for this run alone: the low sixteen hex digits of the clock as its correlation id, so
/// no earlier run's job matches.
fn this_run() -> RequestContext {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("the clock is past the epoch")
        .as_nanos();
    let mut drawn = [0_u8; 16];
    for (slot, digit) in drawn.iter_mut().zip(format!("{nanos:016x}").bytes().rev()) {
        *slot = digit;
    }
    // The deployment as subject, as every matrix cell asks, so the whole corpus is in scope.
    let chain = PrincipalChain::of(Subject::TheDeploymentItself)
        .acting(ActorChain::of(Actor::parse("user@example.com").expect("an actor")));
    RequestContext::of(chain)
        .naming_client(ClientClaims::of(Some(ClientClaim::read(Some("sutura-live-cell"))), None))
        .correlated(drawn)
}

/// Answers the corpus's active-subscriptions question as `context` on `warehouse` through `broker`.
fn answered_on<B>(warehouse: BigQuery, context: &RequestContext, broker: &B)
where
    B: CredentialBroker,
    B::Error: Send,
{
    let pinned = load_on::<BigQuery>();
    let warehouse = sutura_app::Warehouses::of(warehouse);
    let validated = sutura_app::verify_and_validate(pinned, &warehouse).expect("the anchors hold");
    let path = questions()
        .into_iter()
        .find(|path| stem(path) == "active-subscriptions-by-month")
        .expect("the corpus asks for active subscriptions by month");
    let answered = sutura_app::answer(
        &validated,
        &read_question(&path),
        context,
        broker,
        &warehouse,
        &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
        1 << 30,
        crate::adapters::deadline(),
        &sutura_app::SpendLedger::no_budget(),
        sutura_domain::plan::Ceilings::DEFAULT,
    )
    .expect("the question is answered")
    .into_outcome();
    assert!(matches!(answered, ToolOutcome::Answer { .. }), "{answered:?}");
}

/// `read`'s rows once a job carries the label, within a minute: the view is near real time.
fn read_back(read: impl Fn() -> RowSet) -> RowSet {
    for _ in 0..12 {
        let rows = read();
        if !rows.rows().is_empty() {
            return rows;
        }
        std::thread::sleep(Duration::from_secs(5));
    }
    panic!("no job in INFORMATION_SCHEMA.JOBS carries this run's correlation label within a minute");
}

fn text(cell: &Value) -> String {
    match *cell {
        Value::Text(ref text) => text.clone(),
        ref other => panic!("a JOBS cell read back as {other:?}"),
    }
}

/// Each job's `user_email`, for the job `context` labelled.
fn principals(context: &RequestContext, region: &JobsRegion) -> Vec<String> {
    let reader = crate::adapters::bigquery::bigquery_warehouse(crate::adapters::source());
    let labels = JobLabels::of(context);
    let [_, _, (_, correlation)] = labels.pairs();
    read_back(|| {
        reader.job_principals(region, correlation).unwrap_or_else(|e| {
            panic!("INFORMATION_SCHEMA.JOBS could not be read; the CI identity needs bigquery.jobs.listAll: {e}")
        })
    })
    .rows()
    .iter()
    .flat_map(|row| row.iter().map(text))
    .collect()
}

#[test]
fn a_bigquery_job_carries_its_callers_labels_back_out_of_information_schema_jobs() {
    if !runs_here::<BigQuery>() {
        return;
    }
    let region = region();
    let context = this_run();
    let labels = JobLabels::of(&context);
    let [(_, client), (_, actor), (_, correlation)] = labels.pairs();
    answered_on(
        open::<BigQuery>(&load_on::<BigQuery>()),
        &context,
        &crate::adapters::shared_credential(),
    );
    let reader = crate::adapters::bigquery::bigquery_warehouse(crate::adapters::source());
    let rows = read_back(|| {
        reader.job_labels(&region, correlation).unwrap_or_else(|e| {
            panic!("INFORMATION_SCHEMA.JOBS could not be read; the CI identity needs bigquery.jobs.listAll: {e}")
        })
    });
    for row in rows.rows() {
        assert_eq!(
            row.iter().map(text).collect::<Vec<_>>(),
            [client, actor],
            "the job's client and actor labels"
        );
    }
}

/// The control for the delegated cell below: a shared job is recorded under the identity the
/// deployment holds, which `SESSION_USER()` reads under the same credential. No value is printed,
/// because the identity names the masked project.
#[test]
fn a_shared_bigquery_job_is_recorded_under_the_deployments_own_principal() {
    if !runs_here::<BigQuery>() {
        return;
    }
    let region = region();
    let context = this_run();
    answered_on(
        open::<BigQuery>(&load_on::<BigQuery>()),
        &context,
        &crate::adapters::shared_credential(),
    );
    let deployment = crate::adapters::bigquery::bigquery_warehouse(crate::adapters::source())
        .session_user(&crate::adapters::presented(), &BillTo::Unstated)
        .unwrap_or_else(|e| panic!("SESSION_USER() could not be read: {e}"));
    let recorded = principals(&context, &region);
    assert!(
        recorded.iter().all(|email| email.eq_ignore_ascii_case(deployment.as_str())),
        "{} of {} shared jobs are recorded under another principal than SESSION_USER()",
        recorded
            .iter()
            .filter(|email| !email.eq_ignore_ascii_case(deployment.as_str()))
            .count(),
        recorded.len()
    );
}

/// Presents every source `address`, on the deployment's word: `domain_delegation`'s shape.
struct Delegates(PrincipalName);

#[derive(Debug, thiserror::Error)]
#[error("the delegated address does not cover the sources")]
struct Uncovered;

impl CredentialBroker for Delegates {
    type Error = Uncovered;

    fn mint(&self, context: &RequestContext, sources: &SourceSet) -> Result<Minted, Self::Error> {
        let presented = sources
            .iter()
            .map(|at| (at.clone(), Presented::SubjectPrincipal { name: self.0.clone() }))
            .collect();
        let credentials = LegCredentials::minted(context.chain().subject().clone(), Expiry::NothingExpires, sources, presented)
            .map_err(|_uncovered| Uncovered)?;
        Ok(Minted::Granted { credentials })
    }
}

/// **The source's own record of a delegated call**: a job run through `domain_delegation` is
/// recorded under the delegated user's address, not under the delegating account or the CI
/// identity that signed for it. It needs a domain-wide delegation the bq-test environment does not
/// currently provide, so this hosted cell remains ignored until `SUTURA_BQ_DWD_ACCOUNT` and
/// `SUTURA_BQ_DWD_USER` are supplied. No value is printed: both are environment secrets.
#[test]
#[ignore = "requires owner-provided BigQuery domain-delegation infrastructure"]
fn a_delegated_bigquery_job_is_recorded_under_the_delegated_address() {
    if !runs_here::<BigQuery>() {
        return;
    }
    let account = required(
        "SUTURA_BQ_DWD_ACCOUNT",
        "domain-wide delegation (test-infra/README.md, mode c), so a delegated job cannot run",
    );
    let user = required(
        "SUTURA_BQ_DWD_USER",
        "domain-wide delegation user (test-infra/README.md, mode c), so a delegated job cannot run",
    );
    let account = DelegatingAccount::parse(&account).unwrap_or_else(|_| panic!("SUTURA_BQ_DWD_ACCOUNT is not a service account"));
    let address = PrincipalName::parse(&user).unwrap_or_else(|_| panic!("SUTURA_BQ_DWD_USER is not a principal name"));
    let region = region();
    let context = this_run();
    // Every job runs as the address it is handed, through `account`, signed by the CI credential,
    // and bills the one project the corpus's tables are in.
    let delegating = BigQuery::over_adbc(
        crate::adapters::source(),
        SourcePosture::ImpersonationAtSource,
        JobBilling::CallerRun { caller_may_name: false },
        DriverLocation::parse(&required("SUTURA_BIGQUERY_ADBC_DRIVER", "ADBC driver"))
            .unwrap_or_else(|_| panic!("SUTURA_BIGQUERY_ADBC_DRIVER is not an absolute path")),
        Impersonation::DomainDelegation(account),
        BytesBilledCeiling::parse(1 << 30).expect("a gibibyte is a ceiling"),
    );
    answered_on(delegating, &context, &Delegates(address));
    let recorded = principals(&context, &region);
    assert!(
        recorded.iter().all(|email| email.eq_ignore_ascii_case(&user)),
        "{} of {} delegated jobs are recorded under another principal than SUTURA_BQ_DWD_USER",
        recorded.iter().filter(|email| !email.eq_ignore_ascii_case(&user)).count(),
        recorded.len()
    );
}
