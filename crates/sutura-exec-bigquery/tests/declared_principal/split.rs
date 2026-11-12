//! A split leg at a real dataset (`docs/adr/20261102082308-workers-read-sources.md`, W2): a pool
//! source's lookup leg run once as caller A, and its streams read with A's forwarded token on a
//! second opened source, the way a reading worker reads them - the token is its only credential.
//!
//! **What a green run observes**: the streams together answer what the whole read answers;
//! `BigQuery`, not sutura, refuses caller B a table it grants caller A alone; the forwarded token
//! names the source's own pool and ends when Google's token service said; and a token Google did not
//! issue reads no stream. One cell asserts no outcome: whether Storage Read serves a stream to a
//! principal other than the one whose job opened its session is printed to the run log, and the
//! cell passes either way.
//!
//! **What it does not observe**: the reading worker's refusal of a token for another pool, or of a
//! grant that has ended, happens in `sutura serve` before `BigQuery` is asked, so only its offline
//! cells measure it - this venue has one pool and cannot mint an expired token. No planning node,
//! no grant and no served binary take part: the cells call the adapter's two halves directly.
//!
//! Each table is an environment value written `project.dataset.table`, with an `id` column. The
//! split table holds 500,001 to 1,000,000 distinct ids, so the transport asks for two streams and
//! the whole read fits its row bound.

use core::num::NonZeroUsize;
use core::time::Duration;
use sha2::Digest as _;
use sutura_domain::warehouse::execution::Execution;
use std::time::{Instant, SystemTime};

use sutura_domain::definitions::DefinitionDigest;
use sutura_domain::identity::{AssertionDigest, Presented, PrincipalChain, RequestContext, Secret, Subject};
use sutura_domain::model::{ColumnName, DimensionName, MetricName, QualifiedTable};
use sutura_domain::pinned::MetricDigest;
use sutura_domain::plan::{Executable, LegPlan, PlanBindings, PlanColumn, PlanKey, ResultLabel, StatementTables};
use sutura_domain::warehouse::agreement::{RealTolerance, agree_on_content};
use sutura_domain::warehouse::deadline::{Budget, Deadline};
use sutura_domain::warehouse::labels::JobLabels;
use sutura_domain::warehouse::{ResultBatches, RowSet, Warehouse as _};
use sutura_exec_bigquery::adbc::{AdbcBigQuery, AdbcError, SplitLeg, WorkloadPool};
use sutura_exec_bigquery::{BigQueryError, BigQueryWarehouse};

use super::support::live_env;
use super::{opened, source, through_the_hop};

/// The table both callers read, which splits into two or more streams.
const SPLIT_TABLE: &str = "SUTURA_BQ_SPLIT_TABLE";

/// The table `BigQuery` grants caller A alone.
const A_ONLY_TABLE: &str = "SUTURA_BQ_A_ONLY_TABLE";

type Split = Result<SplitLeg, BigQueryError<AdbcError>>;
type Read = Result<RowSet, BigQueryError<AdbcError>>;

/// A lookup leg projecting the `id` column of the table `variable` names.
fn leg_over(variable: &str) -> LegPlan {
    let table = QualifiedTable::parse(live_env(variable)).expect("the venue table is written project.dataset.table");
    let column = PlanColumn::new(table.name().clone(), ColumnName::parse("id").expect("a column name"));
    LegPlan::Lookup {
        source: source(),
        metric: MetricDigest::new(
            MetricName::parse("split_venue").expect("a metric name"),
            DefinitionDigest::parse("0".repeat(64)).expect("a digest"),
        ),
        table: StatementTables::only(table),
        keys: vec![PlanKey::new(
            ResultLabel::dimension(&DimensionName::parse("id").expect("a dimension name")),
            column,
        )],
        bindings: PlanBindings::none(),
    }
}

/// The subject whose inbound token `variable` holds, as the hop hands it to the source.
fn caller(variable: &str) -> Presented {
    Presented::SubjectToken {
        material: through_the_hop(&live_env(variable)),
    }
}

fn deadline() -> Deadline {
    Deadline::opened_at(
        Instant::now(),
        Budget::parse(Duration::from_secs(600)).expect("ten minutes is a budget"),
    )
}

fn in_ten_minutes() -> SystemTime {
    SystemTime::now()
        .checked_add(Duration::from_secs(600))
        .expect("ten minutes from now is a time")
}

fn labels() -> JobLabels {
    JobLabels::of(&RequestContext::of(PrincipalChain::of(
        Subject::verified("split-venue").expect("a subject"),
    )))
}

fn readers() -> NonZeroUsize {
    NonZeroUsize::new(4).expect("four is not zero")
}

/// `leg` split by `caller` at `at`, into at most four streams.
fn split(at: &BigQueryWarehouse<AdbcBigQuery>, leg: &LegPlan, caller: &Presented, ends: SystemTime) -> Split {
    at.split_leg(leg, caller, deadline(), readers(), ends, &labels())
}

fn rows_of(batches: &ResultBatches) -> RowSet {
    batches.to_rows().expect("a stream's cells are readable")
}

/// The stream `descriptor` names, read on a source opened apart from the one that ran the job.
#[expect(
    clippy::disallowed_methods,
    reason = "the venue reads a stream as a reading worker does, with the forwarded token as its only credential"
)]
fn read(leg: &LegPlan, descriptor: &[u8], token: &Secret) -> Read {
    opened()
        .read_partition(leg, descriptor, token, deadline())
        .map(|batches| rows_of(&batches))
}

#[test]
#[ignore = "needs a real BigQuery project, the driver .so, and a table caller A reads that splits into two or more streams"]
#[expect(
    clippy::disallowed_methods,
    reason = "the live adapter venue reads each actual descriptor with the forwarded token and checks its receipt"
)]
fn a_split_pool_leg_read_with_the_forwarded_token_answers_what_the_whole_read_answers() {
    let (leg, a) = (leg_over(SPLIT_TABLE), caller("SUTURA_BQ_INBOUND_A"));
    let warehouse = opened();
    let whole = rows_of(
        &warehouse
            .execute(Executable::Leg(&leg), &a, deadline())
            .expect("caller A reads the split table whole"),
    );
    let (descriptors, token) = split(&warehouse, &leg, &a, in_ten_minutes())
        .expect("caller A's split of the split table runs")
        .into_parts();
    eprintln!("the split table answered {} streams", descriptors.len());
    assert!(
        descriptors.len() >= 2,
        "the split table answered fewer than two streams, so no stream was read apart from the job"
    );
    let (mut columns, mut rows) = (Vec::new(), Vec::new());
    for descriptor in &descriptors {
        let mut batches = opened().read_partition(&leg, descriptor, token.secret(), deadline())
            .expect("a stream reads with caller A's forwarded token");
        let expected: [u8; 32] = sha2::Sha256::digest(descriptor).into();
        assert_eq!(batches.take_executions(), vec![Execution::BigQueryRead {
            source: source(), descriptor_sha256: expected,
        }]);
        let (labels, read) = rows_of(&batches).into_parts();
        columns = labels;
        rows.extend(read);
    }
    let streamed = RowSet::new(columns, rows).expect("every stream answers the leg's columns");
    agree_on_content(&whole, &streamed, RealTolerance::DIFFERENTIAL)
        .expect("the streams together answer what the whole read answers");
}

#[test]
#[ignore = "needs a real BigQuery project, the driver .so, and a table BigQuery grants caller A alone"]
fn a_split_of_a_table_bigquery_grants_caller_a_alone_is_refused_to_caller_b_by_bigquery() {
    let (only_a, both) = (leg_over(A_ONLY_TABLE), leg_over(SPLIT_TABLE));
    let (a, b) = (caller("SUTURA_BQ_INBOUND_A"), caller("SUTURA_BQ_INBOUND_B"));
    let warehouse = opened();
    // The controls: the table answers caller A's split, and caller B splits a table it is granted,
    // so the refusal below is neither a broken table nor a caller who can split nothing.
    drop(split(&warehouse, &only_a, &a, in_ten_minutes()).expect("caller A's split of its own table runs"));
    drop(split(&warehouse, &both, &b, in_ten_minutes()).expect("caller B's split of the split table runs"));
    let refused =
        split(&warehouse, &only_a, &b, in_ten_minutes()).expect_err("caller B split a table BigQuery grants caller A alone");
    eprintln!("caller B was refused: {refused:?}");
    assert!(
        matches!(
            refused,
            BigQueryError::Endpoint {
                cause: AdbcError::Adbc(_)
            }
        ),
        "the refusal did not come from the driver, so BigQuery was not what refused caller B"
    );
    assert!(
        warehouse.source_refused(&refused),
        "BigQuery's refusal of caller B does not read as source_refused, so a caller would see an outage"
    );
}

#[test]
#[ignore = "needs a real BigQuery project, the driver .so, and the pool's audience"]
fn a_split_forwards_a_token_for_the_sources_own_pool_that_ends_when_googles_token_service_said() {
    let leg = leg_over(SPLIT_TABLE);
    let material = through_the_hop(&live_env("SUTURA_BQ_INBOUND_A"));
    let presented = AssertionDigest::of(&material);
    let warehouse = opened();
    let pool = WorkloadPool::parse(&live_env("SUTURA_BQ_AUDIENCE")).expect("the declared pool audience is usable");
    let started = SystemTime::now();
    let ends = started
        .checked_add(Duration::from_secs(60))
        .expect("a minute from now is a time");
    let leg_split =
        split(&warehouse, &leg, &Presented::SubjectToken { material }, ends).expect("caller A's split of the split table runs");
    let until = leg_split.until();
    let (_, token) = leg_split.into_parts();
    let lifetime = token
        .not_after()
        .duration_since(started)
        .expect("the forwarded token ends after the split began");
    eprintln!("the forwarded token lasts {} s", lifetime.as_secs());
    assert_eq!(token.pool(), &pool, "the token names a pool other than the source's own");
    assert_eq!(warehouse.federates_at(), Some(&pool));
    assert!(
        token.exchanged_from() == &presented,
        "the token fingerprints a credential other than the one caller A presented"
    );
    assert!(
        lifetime <= Duration::from_hours(12),
        "the forwarded token ends later than any Google access token does, so its expiry was misread"
    );
    assert_eq!(
        until,
        ends.min(token.not_after()),
        "the split ends later than its token or than it was asked to"
    );
}

#[test]
#[ignore = "needs a real BigQuery project, the driver .so, and a split table caller A reads"]
fn a_stream_read_with_a_token_google_did_not_issue_is_refused() {
    let (leg, a) = (leg_over(SPLIT_TABLE), caller("SUTURA_BQ_INBOUND_A"));
    let (descriptors, token) = split(&opened(), &leg, &a, in_ten_minutes())
        .expect("caller A's split of the split table runs")
        .into_parts();
    let descriptor = descriptors.first().expect("the split answered a stream");
    drop(read(&leg, descriptor, token.secret()).expect("the stream reads with caller A's own forwarded token"));
    // The limit: this job's deployment key is in the environment, and whether it may read caller
    // A's stream is that key's grant, so a refusal here rules out a fallback only to a key that may.
    let refused = read(&leg, descriptor, &Secret::new("not-a-token-google-issued"))
        .expect_err("a stream was read with a token Google never issued");
    eprintln!("a token Google did not issue was refused: {refused:?}");
}

#[test]
#[ignore = "needs a real BigQuery project, the driver .so, both callers' tables; it records an outcome and asserts none"]
fn whether_storage_read_serves_a_stream_to_a_principal_other_than_its_creator_is_recorded() {
    let (both, only_a) = (leg_over(SPLIT_TABLE), leg_over(A_ONLY_TABLE));
    let (a, b) = (caller("SUTURA_BQ_INBOUND_A"), caller("SUTURA_BQ_INBOUND_B"));
    let warehouse = opened();
    let (_, b_token) = split(&warehouse, &both, &b, in_ten_minutes())
        .expect("caller B's split of the split table runs")
        .into_parts();
    for (table, leg) in [("the split table", &both), ("the table only A reads", &only_a)] {
        let (descriptors, a_token) = split(&warehouse, leg, &a, in_ten_minutes())
            .expect("caller A's split runs")
            .into_parts();
        let descriptor = descriptors.first().expect("caller A's split answered a stream");
        // The control: the stream reads for the caller whose job opened it, so a refusal of
        // caller B below is not a stream nobody can read.
        let own = read(leg, descriptor, a_token.secret()).expect("caller A reads a stream of its own split");
        match read(leg, descriptor, b_token.secret()) {
            Ok(served) => eprintln!(
                "observed, {table}: Storage Read SERVED caller B {} rows of caller A's stream ({} for caller A)",
                served.rows().len(),
                own.rows().len()
            ),
            Err(refused) => eprintln!("observed, {table}: Storage Read REFUSED caller B caller A's stream: {refused:?}"),
        }
    }
}
