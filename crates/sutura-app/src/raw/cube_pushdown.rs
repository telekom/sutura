//! A narrowed search over a protected Cube table, end to end: the real planner, the real Cube
//! adapter over a scripted transport, and the cap Cube's SQL answer is held to.
//!
//! Cube cannot apply a text policy, so the engine does, over the raw rows the leg answers. Only a
//! conjunct that reads no policy column can narrow what Cube answers; the transport stands in for a
//! Cube that applies the conjunct it is sent, and answers past the cap for a scan nothing narrows.

use core::cell::RefCell;
use std::rc::Rc;

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

/// One more row than Cube's answer may hold.
const PAST_THE_CAP: usize = MAX_ROWS as usize + 1;

#[derive(Debug, thiserror::Error)]
#[error("this transport answers only the SQL surface")]
struct SqlOnly;

/// A Cube holding `MAX_ROWS` and one more rows, of which three are in region `rare`. It applies a
/// `region = 'rare'` it is sent, and answers every other statement with the whole table. Its
/// columns are the planner's order for the scan: `body`, `id`, `region`.
struct Holding {
    received: Rc<RefCell<Vec<String>>>,
}

fn body(rows: &[[&str; 3]]) -> Vec<u8> {
    let schema: Vec<_> = (0..3)
        .map(|column| serde_json::json!({ "name": format!("c{column}"), "column_type": "String" }))
        .collect();
    let mut body = serde_json::to_vec(&serde_json::json!({ "schema": schema })).expect("JSON");
    body.push(b'\n');
    body.extend(serde_json::to_vec(&serde_json::json!({ "data": rows })).expect("JSON"));
    body
}

impl CubeTransport for Holding {
    type Error = SqlOnly;

    fn meta(&self, _token: &Secret, _deadline: Deadline) -> Answer<Self::Error> {
        Err(SqlOnly)
    }

    fn load(&self, _query: &serde_json::Value, _token: &Secret, _deadline: Deadline) -> Result<Loaded, Self::Error> {
        Err(SqlOnly)
    }

    fn sql(&self, query: &str, _token: &Secret, _deadline: Deadline) -> Answer<Self::Error> {
        self.received.borrow_mut().push(String::from(query));
        if query.contains("= 'rare'") {
            Ok(body(&[
                ["a secret needle", "1", "rare"],
                ["plain", "2", "rare"],
                ["needle two", "3", "rare"],
            ]))
        } else {
            Ok(body(&vec![["x", "1", "common"]; PAST_THE_CAP]))
        }
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
        [column("id"), column("region"), column("body").with_text_policy(masked)],
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

/// The outcome of `sql`, and every statement Cube was sent for it.
fn asked(sql: &str) -> (RawOutcome, Vec<String>) {
    let received = Rc::new(RefCell::new(Vec::new()));
    let warehouses = Warehouses::of(CubeWarehouse::of(
        source(),
        shared(),
        Secret::new(String::from("fixture-deployment-token")),
        Holding {
            received: Rc::clone(&received),
        },
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
    let sent = received.borrow().clone();
    (answered.into_outcome(), sent)
}

#[test]
fn a_search_narrowed_by_a_policy_free_conjunct_answers_past_the_cube_row_cap() {
    let (outcome, sent) = asked("select id, body from docs where region = 'rare' and body like '%needle%' order by id");
    let RawOutcome::Rows { rows, .. } = outcome else {
        panic!("a narrowed search answers: {outcome:?}");
    };
    assert_eq!(
        rows,
        [["1", "a *** needle"], ["3", "needle two"]]
            .map(|row| row.map(String::from).to_vec())
            .to_vec(),
        "{sent:?}"
    );
    let [statement] = sent.as_slice() else {
        panic!("one statement reached Cube: {sent:?}")
    };
    assert!(!statement.contains("LIKE"), "the policy conjunct stayed local: {statement}");
}

