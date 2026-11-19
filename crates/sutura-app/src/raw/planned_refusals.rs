//! Refusals through the real planner and caller scope, with a recording source port.

use std::cell::Cell;

use sutura_domain::capabilities::MetadataCapabilities;
use sutura_domain::catalog::{Audience, Column, ColumnType, Definitions, Description, Model};
use sutura_domain::identity::Presented;
use sutura_domain::knowledge::Knowledge;
use sutura_domain::model::{ColumnName, ModelName, SourceName, TableName};
use sutura_domain::pinned::{Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions};
use sutura_domain::plan::{AnchorPlan, Executable};
use sutura_domain::raw::{RawOutcome, RawRefusalReason, RawStatement, SqlConstruct};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::schema::TypeDialect;
use sutura_domain::warehouse::{AnchorRows, PreFlight, RawExecution, ResultBatches, Warehouse};
use sutura_exec_datafusion::DataFusionCombiner;

use super::{RunSqlScope, answer_sql};
use crate::tests::{asked_by_a_person, shared, source, test_deadline};
use crate::tests_support::{AdapterFailure, CountingBroker, DriverFailure, RawCapableWarehouse, run_sql_sources};
use crate::{Warehouses, verify_and_validate};

struct Physical {
    raw: RawCapableWarehouse,
    calls: Cell<usize>,
    fails: bool,
}

impl Warehouse for Physical {
    type Error = AdapterFailure;
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;
    const ACCEPTS_RAW_STATEMENTS: bool = true;

    fn source(&self) -> &SourceName {
        self.raw.source()
    }
    fn posture(&self) -> &SourcePosture {
        self.raw.posture()
    }
    fn type_dialect(&self) -> Option<TypeDialect> {
        Some(TypeDialect::DuckDb)
    }
    fn dry_run(&self, plan: Executable<'_>, presented: &Presented, deadline: Deadline) -> Result<PreFlight, Self::Error> {
        self.raw.dry_run(plan, presented, deadline)
    }
    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        self.raw.execute(executable, presented, deadline)
    }
    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(AdapterFailure::Statement { cause: DriverFailure })
    }
    fn execute_raw(&self, statement: &RawStatement, presented: &Presented, deadline: Deadline) -> RawExecution<Self::Error> {
        self.calls.set(self.calls.get() + 1);
        if self.fails {
            return Some(Err(AdapterFailure::Statement { cause: DriverFailure }));
        }
        self.raw.execute_raw(statement, presented, deadline)
    }
}

fn bundle() -> PinnedDefinitions {
    let model = |name: &str| {
        Model::new(
            ModelName::parse(name).expect("a fixture model"),
            source(),
            TableName::parse(name).expect("a fixture table"),
            [Column::new(
                ColumnName::parse("id").expect("a fixture column"),
                Some(ColumnType::parse("BIGINT").expect("a fixture type")),
                Description::default(),
                Some(false),
            )],
            Description::default(),
        )
    };
    PinnedDefinitions::pin(
        DefinitionVersion::parse("test-1").expect("a fixture version"),
        Definitions::assemble(
            vec![model("visible_table").with_audience(Audience::Open), model("hidden_table")],
            vec![],
            vec![],
        )
        .expect("the fixture is consistent"),
        Knowledge::none(),
        ContributionManifest::single(source(), Contribution::of(MetadataCapabilities::nothing())),
    )
    .expect("the fixture pins")
}

fn refusal(sql: &str, fails: bool) -> (RawRefusalReason, usize, usize) {
    let warehouses = Warehouses::of(Physical {
        raw: RawCapableWarehouse::answering_rows(source(), shared(), 1),
        calls: Cell::new(0),
        fails,
    });
    let pinned = verify_and_validate(bundle(), &warehouses).expect("the declared schemas validate");
    let broker = CountingBroker::default();
    let statement = RawStatement::parse(sql).expect("a bounded query");
    let scope = RunSqlScope::new(run_sql_sources(vec![source()]), true);
    let answer = answer_sql(
        &pinned,
        &asked_by_a_person(),
        &statement,
        &broker,
        &warehouses,
        &DataFusionCombiner::new().expect("the planner builds"),
        &scope,
        1 << 30,
        test_deadline(),
    )
    .expect("a refusal is an answer");
    let RawOutcome::Refusal { reason } = answer.into_outcome() else {
        panic!("the query must be refused");
    };
    (
        reason,
        broker.asked(),
        warehouses.get(&source()).expect("the fixture source").calls.get(),
    )
}

#[test]
fn hidden_and_absent_tables_have_byte_identical_details_and_open_nothing() {
    let (hidden, minted, ran) = refusal("select id from hidden_table", false);
    let (absent, absent_minted, absent_ran) = refusal("select id from absent_table", false);
    assert_eq!(hidden, RawRefusalReason::UnknownTable);
    assert_eq!(hidden.code(), absent.code());
    assert_eq!(hidden.to_string().as_bytes(), absent.to_string().as_bytes());
    assert_eq!((minted, ran, absent_minted, absent_ran), (0, 0, 0, 0));
}

#[test]
fn excluded_and_absent_columns_have_byte_identical_details_and_open_nothing() {
    let (excluded, minted, ran) = refusal("select excluded_column from visible_table", false);
    let (absent, absent_minted, absent_ran) = refusal("select absent_column from visible_table", false);
    assert_eq!(excluded, RawRefusalReason::UnknownColumn);
    assert_eq!(excluded.code(), absent.code());
    assert_eq!(excluded.to_string().as_bytes(), absent.to_string().as_bytes());
    assert_eq!((minted, ran, absent_minted, absent_ran), (0, 0, 0, 0));
}

#[test]
fn planned_source_failure_and_unsupported_syntax_have_safe_discovery_details() {
    let (source, minted, ran) = refusal("select id from visible_table", true);
    assert_eq!(source, RawRefusalReason::SourceFailed);
    assert_eq!((minted, ran), (1, 1));
    let (syntax, minted, ran) = refusal("select id from visible_table fetch first 1 rows only", false);
    assert_eq!(
        syntax,
        RawRefusalReason::UnsupportedSyntax {
            construct: SqlConstruct::FetchFirst
        }
    );
    assert_eq!((minted, ran), (0, 0));
    for reason in [source, syntax] {
        let detail = reason.to_string();
        assert!(detail.contains("describe_catalog"), "{detail}");
        assert!(detail.contains("prompt.list_physical_schema"), "{detail}");
        assert!(!detail.contains("visible_table"), "{detail}");
        assert!(!detail.contains("orders.csv"), "{detail}");
    }
}
