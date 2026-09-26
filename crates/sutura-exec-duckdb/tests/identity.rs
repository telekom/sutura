#![forbid(unsafe_code)]
//! The identity refusal through the execution PORT, not only through the private `deliverable`
//! helper the in-crate cells call: deleting `self.deliverable(presented)?` from `dry_run` or
//! `execute` left every existing cell green, because none of them reached either port method with
//! a subject credential.

#[cfg(test)]
mod identity {
    use sutura_conformance::corpus;
    use sutura_domain::identity::{Presented, PrincipalName, Secret};
    use sutura_domain::plan::Executable;
    use sutura_domain::warehouse::Warehouse as _;
    use sutura_exec_duckdb::{DuckDbError, DuckDbWarehouse};

    fn subjects() -> [Presented; 2] {
        [
            Presented::SubjectToken {
                material: Secret::new("an-exchanged-token"),
                impersonate: None,
            },
            Presented::SubjectPrincipal {
                name: PrincipalName::parse("analyst_role").expect("a test name is a name"),
            },
        ]
    }

    /// An empty in-memory database: nothing to prepare against, so a missing guard answers a
    /// `Prepare` or `Execute` error rather than the refusal, and the assertion names the difference.
    fn warehouse() -> DuckDbWarehouse {
        DuckDbWarehouse::in_memory(corpus::source(), corpus::posture()).expect("an in-memory database opens")
    }

    #[test]
    fn a_subject_credential_is_refused_by_dry_run_before_anything_is_prepared() {
        let warehouse = warehouse();
        let case = corpus::cases().into_iter().next().expect("the corpus has a case");
        for presented in subjects() {
            let outcome = warehouse.dry_run(Executable::Query(case.plan()), &presented, corpus::deadline());
            assert!(
                matches!(outcome, Err(DuckDbError::NoPlaceForASubject { .. })),
                "a subject credential has nowhere to arrive on this adapter: {outcome:?}"
            );
        }
    }

    #[test]
    fn a_subject_credential_is_refused_by_execute_before_anything_is_run() {
        let warehouse = warehouse();
        let case = corpus::cases().into_iter().next().expect("the corpus has a case");
        for presented in subjects() {
            let outcome = warehouse.execute(Executable::Query(case.plan()), &presented, corpus::deadline());
            assert!(
                matches!(outcome, Err(DuckDbError::NoPlaceForASubject { .. })),
                "a subject credential has nowhere to arrive on this adapter: {outcome:?}"
            );
        }
    }
}
