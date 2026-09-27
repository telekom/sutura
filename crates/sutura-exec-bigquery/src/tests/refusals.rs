//! Refusals no other cell provokes, each driven through the adapter's own entry against a fake and
//! asserted on the returned `Result`, so a mutation that lets the call succeed fails the assertion
//! rather than an `expect_err` ahead of it.

use std::sync::Arc;

use arrow_array::{ArrayRef, Float32Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use sutura_domain::calendar::TimeRange;
use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, TableName};
use sutura_domain::plan::{
    Executable, PlanBindings, PlanBucket, PlanColumn, PlanMeasure, PlanTerm, QueryPlan, ResultLabel, StatementTables,
};
use sutura_domain::warehouse::{Accumulating, ResultBatches, ResultBudget, Warehouse as _};

use crate::BigQueryError;
use crate::transport::{NamedResource, ProjectId, UnusableResourceName};

use super::{Recording, a_subject_token, day, impersonating_posture, leg_of, open, shared_posture, source, test_deadline};

/// A one-row `Float32` answer: the only float width `to_rows` maps is `Float64`.
fn unmapped_float_column() -> ResultBatches {
    let schema: SchemaRef = Arc::new(Schema::new(vec![Field::new("session_user", DataType::Float32, true)]));
    let array: ArrayRef = Arc::new(Float32Array::from(vec![0.1_f32]));
    let mut accumulating = Accumulating::announcing(Arc::clone(&schema), 1, ResultBudget::of_bytes(core::num::NonZeroUsize::MAX));
    let batch = RecordBatch::try_new(schema, vec![array]).expect("a one-column fixture batch is rectangular");
    accumulating.push(batch).expect("a fixture batch carries its own schema");
    accumulating.finish()
}

/// A plan with no predicate at all, which `generate` refuses as `NoPredicate`.
fn unfiltered_plan() -> QueryPlan {
    let table = TableName::parse("fct_subscription_monthly").expect("a test table is a table");
    let column = |name: &str| PlanColumn::new(table.clone(), ColumnName::parse(name).expect("a test column is a column"));
    let metric = MetricName::parse("mrr").expect("a test metric is a metric");
    QueryPlan::new(
        source(),
        metric.clone(),
        StatementTables::only(table.clone()),
        PlanBucket::new(ResultLabel::bucket(), Grain::Month, column("month")),
        Vec::new(),
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: column("mrr_cents"),
            },
        },
        ResultLabel::measure(&metric),
        PlanBindings::none(),
        TimeRange::new(day("2026-06-01"), day("2026-07-01")).expect("a test range is a range"),
    )
}

#[test]
fn a_plan_that_will_not_render_is_refused_as_render_and_not_as_an_endpoint_failure() {
    let warehouse = open(Recording::empty(), shared_posture());
    let refused = warehouse.execute(
        Executable::Query(&unfiltered_plan()),
        &leg_of(&shared_posture()),
        test_deadline(),
    );
    assert!(matches!(refused, Err(BigQueryError::Render { .. })), "{refused:?}");
    assert!(
        warehouse.transport.seen.borrow().is_empty(),
        "a plan that would not render may not reach the endpoint"
    );
}

#[test]
fn an_identity_read_whose_column_type_is_unmapped_is_refused_as_unreadable() {
    let warehouse = open(Recording::answering(unmapped_float_column()), impersonating_posture());
    let refused = warehouse.session_user(&a_subject_token("exchanged-for-principal-a"));
    assert!(matches!(refused, Err(BigQueryError::Unreadable { .. })), "{refused:?}");
}

#[test]
fn an_empty_project_id_is_refused_by_name_and_not_as_a_dataset_id() {
    assert_eq!(
        ProjectId::parse(""),
        Err(UnusableResourceName::Empty {
            what: NamedResource::Project,
        })
    );
}

#[cfg(feature = "fixtures")]
mod fixtures {
    use std::path::{Path, PathBuf};

    use sutura_domain::model::TableName;

    use crate::{FixtureNotLoaded, FixtureNotUsable};

    use super::super::{Broken, Recording, open, shared_posture};

    fn table() -> TableName {
        TableName::parse("fct").expect("a table name parses")
    }

    /// A fixture file written for one cell, under this crate's own target scratch.
    fn written(cell: &str, contents: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp").join(cell);
        std::fs::create_dir_all(&dir).expect("the scratch directory is creatable");
        let path = dir.join("fixture.csv");
        std::fs::write(&path, contents).expect("the fixture is writable");
        path
    }

    #[test]
    fn a_fixture_at_a_path_that_does_not_exist_is_refused_as_unreadable() {
        let missing = Path::new(env!("CARGO_MANIFEST_DIR")).join("no-such-dir/fixture.csv");
        let refused = open(Recording::empty(), shared_posture()).load_fixture(&table(), &missing);
        assert!(matches!(refused, Err(FixtureNotLoaded::Unreadable { .. })), "{refused:?}");
    }

    #[test]
    fn a_fixture_whose_header_is_not_a_column_is_refused_as_not_usable() {
        let path = written("bigquery-fixture-header", "orders.amount\n1\n");
        let refused = open(Recording::empty(), shared_posture()).load_fixture(&table(), &path);
        assert!(
            matches!(
                refused,
                Err(FixtureNotLoaded::NotUsable {
                    cause: FixtureNotUsable::Header { .. },
                    ..
                })
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn a_fixture_the_endpoint_refused_to_load_is_refused_as_an_endpoint_failure() {
        let path = written("bigquery-fixture-endpoint", "k,v\n1,2\n");
        let refused = open(Broken, shared_posture()).load_fixture(&table(), &path);
        assert!(matches!(refused, Err(FixtureNotLoaded::Endpoint { .. })), "{refused:?}");
    }
}
