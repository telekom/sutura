use std::collections::BTreeSet;

use sutura_domain::identity::{Presented, Secret};
use sutura_domain::model::QualifiedTable;
use sutura_domain::raw::RawStatement;
use sutura_domain::warehouse::Warehouse as _;

use super::fakes::{Recording, impersonating_posture, open, test_deadline};
use crate::BigQueryError;
use crate::transport::JobDeadline;

#[test]
fn planned_bigquery_jobs_keep_each_callers_token_project_and_deadline_without_enabling_passthrough() {
    let warehouse = open(Recording::empty(), impersonating_posture());
    let text = RawStatement::parse("SELECT COUNT(*) FROM `acme-data.sales.orders` WHERE amount > 0").expect("bounded SQL");
    let tables = BTreeSet::from([QualifiedTable::parse("acme-data.sales.orders").expect("a qualified table")]);
    let deadline = test_deadline();
    assert!(warehouse.accepts_planned_statements());
    assert!(!warehouse.accepts_raw_statements());
    for token in ["first-caller", "second-caller"] {
        let presented = Presented::SubjectToken {
            material: Secret::new(token.to_owned()),
        };
        assert!(warehouse.execute_raw(&text, &presented, deadline).is_none());
        warehouse
            .execute_planned_statement(&text, &tables, &presented, deadline)
            .expect("planned execution is implemented")
            .expect("the fake answers");
    }
    let seen = warehouse.transport.seen.borrow();
    assert_eq!(seen.len(), 2);
    for (asked, token) in seen.iter().zip(["first-caller", "second-caller"]) {
        assert_eq!(asked.statement, text.as_str());
        assert_eq!(asked.subject.as_deref(), Some(token));
        assert_eq!(asked.project, "acme-data");
        assert_eq!(asked.deadline, JobDeadline::Port(deadline));
        assert_eq!(asked.params, [] as [String; 0]);
    }
    drop(seen);
    let multiple = tables
        .into_iter()
        .chain([QualifiedTable::parse("other-data.sales.orders").expect("another table")])
        .collect();
    let error = warehouse
        .execute_planned_statement(
            &text,
            &multiple,
            &Presented::SubjectToken {
                material: Secret::new("caller".to_owned()),
            },
            deadline,
        )
        .expect("implemented")
        .expect_err("no unambiguous billing project");
    assert!(matches!(error, BigQueryError::NoPayer { .. }), "{error}");
    assert_eq!(warehouse.transport.seen.borrow().len(), 2, "a refused payer sends no job");
}
