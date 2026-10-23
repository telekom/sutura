//! **A `BigQuery` job carries its caller's labels**, read back from `INFORMATION_SCHEMA.JOBS`:
//! the live half of what the adapter's offline cells hold against a fake statement.
//!
//! It runs only in the `bigquery-conformance` CI job, under the matrix's one CI identity and the
//! `shared-service-user` posture - the posture where the label is the only link from a job to its
//! caller. That identity needs `bigquery.jobs.listAll` on the billing project to read the view, and
//! the job's environment names the view's region in `SUTURA_BQ_REGION`. A missing region fails the
//! cell by name rather than skipping it green.

use std::time::{Duration, SystemTime};

use sutura_domain::identity::{Actor, ActorChain, ClientClaim, ClientClaims, PrincipalChain, RequestContext, Subject};
use sutura_domain::query::ToolOutcome;
use sutura_domain::warehouse::Value;
use sutura_domain::warehouse::labels::JobLabels;
use sutura_exec_bigquery::JobsRegion;

use crate::adapters::{load_on, open, questions, read_question, runs_here, stem};

type BigQuery = sutura_exec_bigquery::BigQueryWarehouse<sutura_exec_bigquery::adbc::AdbcBigQuery>;

#[test]
fn a_bigquery_job_carries_its_callers_labels_back_out_of_information_schema_jobs() {
    if !runs_here::<BigQuery>() {
        return;
    }
    let region = std::env::var("SUTURA_BQ_REGION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            panic!(
                "SUTURA_BQ_REGION is unset: the bq-test environment names no region for INFORMATION_SCHEMA.JOBS, \
             so the job-label read-back cannot run"
            )
        });
    let region = JobsRegion::parse(&region).unwrap_or_else(|e| panic!("SUTURA_BQ_REGION: {e}"));
    // This run's own id: the low sixteen hex digits of the clock, so no earlier run's job matches.
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
    let context = RequestContext::of(chain)
        .naming_client(ClientClaims::of(Some(ClientClaim::read(Some("sutura-live-cell"))), None))
        .correlated(drawn);
    let labels = JobLabels::of(&context);
    let [(_, client), (_, actor), (_, correlation)] = labels.pairs();

    let pinned = load_on::<BigQuery>();
    let warehouse = sutura_app::Warehouses::of(open::<BigQuery>(&pinned));
    let validated = sutura_app::verify_and_validate(pinned, &warehouse).expect("the anchors hold");
    let path = questions()
        .into_iter()
        .find(|path| stem(path) == "active-subscriptions-by-month")
        .expect("the corpus asks for active subscriptions by month");
    let answered = sutura_app::answer(
        &validated,
        &read_question(&path),
        &context,
        &crate::adapters::shared_credential(),
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

    let reader = crate::adapters::bigquery::bigquery_warehouse(crate::adapters::source());
    let text = |cell: &Value| match *cell {
        Value::Text(ref text) => text.clone(),
        ref other => panic!("a label read back as {other:?}"),
    };
    // The view is near real time, not immediate.
    for _ in 0..12 {
        let rows = reader.job_labels(&region, correlation).unwrap_or_else(|e| {
            panic!("INFORMATION_SCHEMA.JOBS could not be read; the CI identity needs bigquery.jobs.listAll: {e}")
        });
        if !rows.rows().is_empty() {
            for row in rows.rows() {
                assert_eq!(
                    row.iter().map(text).collect::<Vec<_>>(),
                    [client, actor],
                    "the job's client and actor labels"
                );
            }
            return;
        }
        std::thread::sleep(Duration::from_secs(5));
    }
    panic!("no job in INFORMATION_SCHEMA.JOBS carries this run's correlation label within a minute");
}
