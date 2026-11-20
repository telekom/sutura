//! A search over a column under a text policy at a Cube source, end to end: the real planner, the real
//! Cube adapter over a scripted transport, and the cap Cube's SQL answer is held to.

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Audience, Column, ColumnType, Definitions, Description, Model, TextPolicy};
use sutura_domain::identity::Secret;
use sutura_domain::knowledge::Knowledge;
use sutura_domain::model::{ColumnName, ModelName, TableName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};
use sutura_domain::plan::MAX_ROWS;
use sutura_domain::raw::{RawOutcome, RawRefusalReason, RawStatement};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_exec_cube::{Answer, CubeTransport, CubeWarehouse, Loaded};
use sutura_exec_datafusion::DataFusionCombiner;

use super::{RunSqlScope, answer_sql};
use crate::tests::{asked_by_a_person, shared, source, test_deadline};
use crate::tests_support::{CountingBroker, run_sql_sources};
use crate::{Warehouses, verify_and_validate};

#[derive(Debug, thiserror::Error)]
#[error("this transport answers only the SQL surface")]
struct SqlOnly;

/// Answers every Cube SQL request with `columns` columns and one row past the row cap.
struct PastTheCap {
    columns: usize,
}

impl PastTheCap {
    fn body(&self) -> Vec<u8> {
        let schema: Vec<_> = (0..self.columns)
            .map(|column| serde_json::json!({ "name": format!("c{column}"), "column_type": "String" }))
            .collect();
        let rows = usize::try_from(MAX_ROWS).expect("the cap fits a usize") + 1;
        let mut body = serde_json::to_vec(&serde_json::json!({ "schema": schema })).expect("JSON");
        body.push(b'\n');
        body.extend(serde_json::to_vec(&serde_json::json!({ "data": vec![vec!["x"; self.columns]; rows] })).expect("JSON"));
        body
    }
}

impl CubeTransport for PastTheCap {
    type Error = SqlOnly;

    fn meta(&self, _token: &Secret, _deadline: Deadline) -> Answer<Self::Error> {
        Err(SqlOnly)
    }

    fn load(&self, _query: &serde_json::Value, _token: &Secret, _deadline: Deadline) -> Result<Loaded, Self::Error> {
        Err(SqlOnly)
    }

    fn sql(&self, _query: &str, _token: &Secret, _deadline: Deadline) -> Answer<Self::Error> {
        Ok(self.body())
    }

    fn source_refused(&self, _error: &Self::Error) -> bool {
        false
    }

    fn deadline_exceeded(&self, _error: &Self::Error) -> bool {
        false
    }

    fn result_did_not_fit(&self, _error: &Self::Error) -> bool {
        false
    }
}

fn bundle() -> PinnedDefinitions {
    let masked: TextPolicy = serde_json::from_value(serde_json::json!({
        "max_text_bytes": 64,
        "redact": [{ "pattern": "secret", "replace": "***" }]
    }))
    .expect("a fixture policy");
    let column = |name: &str| {
        Column::new(
            ColumnName::parse(name).expect("a fixture column"),
            Some(ColumnType::parse("string").expect("a Cube string")),
            Description::default(),
            Some(false),
        )
    };
    let docs = Model::new(
        ModelName::parse("docs").expect("a fixture model"),
        source(),
        TableName::parse("docs").expect("a fixture table"),
        [column("id"), column("body").with_text_policy(masked)],
        Description::default(),
    )
    .with_audience(Audience::Open);
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a fixture version"),
        Definitions::assemble(vec![docs], vec![], vec![]).expect("the fixture is consistent"),
        Knowledge::none(),
        ContributionManifest::single(source(), Contribution::of(MetadataCapabilities::nothing())),
    )
    .expect("the fixture pins")
}

fn refused(sql: &str, columns: usize) -> RawRefusalReason {
    let warehouses = Warehouses::of(CubeWarehouse::of(
        source(),
        shared(),
        Secret::new(String::from("fixture-deployment-token")),
        PastTheCap { columns },
    ));
    let pinned = verify_and_validate(bundle(), &warehouses).expect("the declared schemas validate");
    let statement = RawStatement::parse(sql).expect("a bounded query");
    let answered = answer_sql(
        &pinned,
        &asked_by_a_person(),
        &statement,
        &CountingBroker::default(),
        &warehouses,
        &DataFusionCombiner::new().expect("the planner builds"),
        &RunSqlScope::new(run_sql_sources(vec![source()]), false),
        1 << 30,
        test_deadline(),
    )
    .expect("a refusal is an answer");
    match answered.into_outcome() {
        RawOutcome::Refusal { reason } => reason,
        rows @ RawOutcome::Rows { .. } => panic!("expected a refusal, not {rows:?}"),
    }
}

/// The remedy is true: a filter on a column without a policy reaches Cube
/// (`cube_pushdown::a_search_narrowed_by_a_policy_free_conjunct_answers_past_the_cube_row_cap`).
#[test]
fn a_search_over_a_protected_cube_table_past_the_row_cap_says_it_needs_a_filter() {
    assert_eq!(
        refused("select id from docs where body like '%needle%'", 2),
        RawRefusalReason::PolicySearchNeedsNarrowing
    );
}

#[test]
fn a_scan_of_a_cube_table_that_leaves_the_policy_column_out_keeps_its_size_refusal() {
    assert_eq!(refused("select id from docs", 1), RawRefusalReason::ResultTooLarge);
}
