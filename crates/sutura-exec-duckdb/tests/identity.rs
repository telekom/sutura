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
    use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared};
    use sutura_domain::warehouse::{ResultBudget, Warehouse as _};
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
        let budget = ResultBudget::of_bytes(core::num::NonZeroUsize::new(1024 * 1024).expect("a mebibyte is positive"));
        DuckDbWarehouse::in_memory(corpus::source(), corpus::posture(), budget).expect("an in-memory database opens")
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

    /// A shared leg - the shape this adapter carries - whose acknowledgement is not this source's.
    #[test]
    fn a_shared_leg_with_another_acknowledgement_is_refused_by_execute() {
        let warehouse = warehouse();
        let case = corpus::cases().into_iter().next().expect("the corpus has a case");
        let fabricated = Presented::SharedServiceUser {
            declared: SharedIdentityDeclared::of(
                AcknowledgementReason::parse("a witness no operator wrote for this source").expect("a test reason is a reason"),
            ),
        };
        let outcome = warehouse.execute(Executable::Query(case.plan()), &fabricated, corpus::deadline());
        assert!(
            matches!(outcome, Err(DuckDbError::PresentedDisagreesWithPosture { .. })),
            "a shared leg carrying another source's witness is refused: {outcome:?}"
        );
    }
}
