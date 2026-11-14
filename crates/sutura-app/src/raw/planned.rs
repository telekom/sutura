//! Planned-only adapters cannot be reached by the raw passthrough path.

use std::cell::RefCell;
use std::collections::BTreeSet;

use sutura_domain::identity::Presented;
use sutura_domain::model::{QualifiedTable, SourceName};
use sutura_domain::plan::{AnchorPlan, Executable, StatementLegs as _};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::{AnchorRows, RawExecution, ResultBatches, Warehouse};

use super::{RawLegs, RunSqlError, pass_through};
use crate::Warehouses;
use crate::tests::{asked_by_a_person, shared, source, test_deadline};
use crate::tests_support::{AdapterFailure, CountingBroker, RawCapableWarehouse};

struct PlannedOnly {
    inner: RawCapableWarehouse,
    seen: RefCell<BTreeSet<QualifiedTable>>,
}

impl Warehouse for PlannedOnly {
    type Error = AdapterFailure;
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &SourceName {
        self.inner.source()
    }
    fn posture(&self) -> &SourcePosture {
        self.inner.posture()
    }
    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: sutura_domain::warehouse::deadline::Deadline,
    ) -> Result<ResultBatches, Self::Error> {
        self.inner.execute(executable, presented, deadline)
    }
    fn verify_anchor(&self, _plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        Err(AdapterFailure::RefusedBySource)
    }
    fn accepts_planned_statements(&self) -> bool {
        true
    }
    fn execute_planned_statement(
        &self,
        statement: &sutura_domain::raw::RawStatement,
        tables: &BTreeSet<QualifiedTable>,
        presented: &Presented,
        deadline: sutura_domain::warehouse::deadline::Deadline,
    ) -> RawExecution<Self::Error> {
        self.seen.borrow_mut().extend(tables.iter().cloned());
        self.inner.execute_raw(statement, presented, deadline)
    }
}

#[test]
fn a_planned_only_adapter_receives_scan_tables_but_cannot_receive_passthrough_text() {
    let warehouses = Warehouses::of(PlannedOnly {
        inner: RawCapableWarehouse::answering_rows(source(), shared(), 1),
        seen: RefCell::default(),
    });
    let context = asked_by_a_person();
    let broker = CountingBroker::default();
    let deadline = test_deadline();
    let text = sutura_domain::raw::RawStatement::parse("SELECT n FROM orders").expect("bounded SQL");
    let mut legs = RawLegs::new(&context, &broker, &warehouses, None, deadline);
    assert!(matches!(
        pass_through(&mut legs, &text, &source()),
        Err(RunSqlError::NoAcceptingSource)
    ));
    assert_eq!(broker.asked(), 0, "passthrough cannot even mint a credential");
    legs.open(&BTreeSet::from([source()])).expect("the broker answers");
    let tables = BTreeSet::from([QualifiedTable::parse("orders").expect("a table")]);
    assert_eq!(
        legs.run_planned(&source(), &text, &tables)
            .expect("planned path answers")
            .rows()
            .len(),
        1
    );
    assert_eq!(*warehouses.get(&source()).expect("the source").seen.borrow(), tables);
}
