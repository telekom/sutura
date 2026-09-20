//! Leg 2 for `BigQuery`, as a hosted leg: does a declared subject's question really execute as the
//! account this source declared for it?
//!
//! **This is the venue `docs/where-identity-is-proven.md` calls *a served binary under a verified
//! human caller*, minus the served binary** - and that split is the honest half of the claim. The
//! oracle for WHICH ACCOUNT is `SELECT SESSION_USER()`, which `BigQueryWarehouse::session_user`
//! reads and no HTTP route exposes (`ACCEPTS_RAW_STATEMENTS` is `false`), so it can only be asked
//! here. WHICH ROWS a caller sees needs the served surface and is not asked at all yet.
//!
//! # Why it is `#[ignore]`d and why it FAILS rather than skips
//!
//! It needs a real project, a real driver `.so` and two real service accounts the running identity
//! may impersonate. `just validate` has no network, so an unignored leg would red every run.
//! `#[ignore]` is how the repository has always carried a hosted venue, and the second half is what
//! stops that from becoming a venue that always passes: every environment value is REQUIRED, and an
//! absent one panics with the variable's name. A leg that skipped on a misconfigured environment
//! would report a green run over nothing, which is the failure this page exists to prevent.
//!
//! # What a green run here does and does not establish
//!
//! Establishes: two distinct subjects resolve to two distinct `BigQuery` principals through the
//! ADBC path, and the deployment's own identity is neither of them.
//!
//! Does NOT establish: that a CALLER's own credential was checked by anything but this deployment.
//! It is not in the chain - `crate`'s own header states why - so this leg shows the resolution is
//! per subject and nothing about what would stop a forged subject. Nor does it establish that the
//! served surface applies the two accounts' row grants; that is a second cell in a venue nobody has
//! built.

#![cfg(feature = "adbc")]

// One `#[cfg(test)]` module holding the whole leg, which is this workspace's shape for an
// integration test target - `tests/conformance.rs` states it in the same words: the strict lints
// exempt test code, and a helper at file scope is not test code as far as clippy is concerned.
#[cfg(test)]
mod declared_principal {

    use sutura_domain::identity::{Presented, PrincipalName};
    use sutura_domain::model::SourceName;
    use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
    use sutura_exec_bigquery::BigQueryWarehouse;
    use sutura_exec_bigquery::adbc::{Impersonation, ImpersonationScopes};
    use sutura_exec_bigquery::transport::{DatasetId, ProjectId};

    /// One required environment value, or a panic naming it.
    ///
    /// **A panic and not a skip**, for this file's own header reason: a hosted venue whose environment
    /// is half-configured must be RED. The panic names the variable and never its value - a project id
    /// and an account address are the two things a public workflow log should not learn from a failure
    /// message it did not have to print.
    fn required(name: &str) -> String {
        match std::env::var(name) {
            Ok(value) if !value.trim().is_empty() => value,
            _ => panic!("{name} is unset or empty, and this leg proves nothing without it"),
        }
    }

    /// The one source alias this leg declares.
    fn source() -> SourceName {
        SourceName::parse("warehouse").expect("a source name is a name")
    }

    /// The adapter, opened `impersonation-at-source` over the real driver at the real project.
    fn opened() -> BigQueryWarehouse<sutura_exec_bigquery::adbc::AdbcBigQuery> {
        let driver = required("SUTURA_BIGQUERY_ADBC_DRIVER");
        let project = ProjectId::parse(required("SUTURA_BQ_PROJECT")).expect("the declared project is a project id");
        let dataset = DatasetId::parse(required("SUTURA_BQ_DATASET")).expect("the declared dataset is a dataset id");
        let scopes = ImpersonationScopes::parse(&required("SUTURA_BQ_SCOPE")).expect("the declared scope is a scope");
        BigQueryWarehouse::over_adbc(
            source(),
            SourcePosture::ImpersonationAtSource,
            project,
            dataset,
            driver,
            Impersonation::AtScope(scopes),
        )
    }

    /// The identity a question runs as when this source is asked as `account`.
    fn executed_as(account: &str) -> String {
        let warehouse = opened();
        let presented = Presented::SubjectPrincipal {
            name: PrincipalName::parse(account).expect("a declared account is a principal name"),
        };
        String::from(
            warehouse
                .session_user(&presented)
                .expect("the dataset answered the identity read")
                .as_str(),
        )
    }

    #[test]
    #[ignore = "needs a real BigQuery project, the driver .so, and two accounts this identity may impersonate"]
    fn each_subject_executes_as_the_account_this_source_declared_for_it() {
        // **THE leg-2 bar, and it is the same one the withdrawn HTTP venue met**: two distinct subjects
        // resolve to two distinct BigQuery principals, observed at the data system rather than at a
        // seam. `SESSION_USER()` resolves to the impersonated account's own address - that is what
        // `telekom/sutura#376`'s `iamcredentials` hop bought and what the driver's
        // `bigquery.impersonate.target_principal` goes through as well - so a `principal://` string
        // here would mean the hop did not happen.
        let first = required("SUTURA_BQ_ACCOUNT_A");
        let second = required("SUTURA_BQ_ACCOUNT_B");
        assert_ne!(first, second, "two accounts that are one account prove nothing");
        let ran_as_first = executed_as(&first);
        let ran_as_second = executed_as(&second);
        assert_eq!(ran_as_first, first, "the first subject's question ran as somebody else");
        assert_eq!(ran_as_second, second, "the second subject's question ran as somebody else");
        assert_ne!(
            ran_as_first, ran_as_second,
            "both subjects resolved to one principal, which is the shared-identity outcome wearing leg 2's name"
        );
    }

    #[test]
    #[ignore = "needs a real BigQuery project, the driver .so, and the running identity's own account"]
    fn the_deployments_own_identity_is_neither_declared_account() {
        // **THE CONTROL, and without it the cell above passes on a deployment that answered both
        // questions as itself** - which is precisely what it would do if the impersonation option were
        // dropped. A shared leg carries no principal, so this read goes out as whatever the driver
        // authenticates as; if that is already one of the two accounts, the other cell's two answers
        // could both be the process and one of them would still match.
        let declared = SharedIdentityDeclared::of(
            AcknowledgementReason::parse("the control leg reads this venue's own identity, under no subject at all")
                .expect("a reason is a reason"),
        );
        let shared = BigQueryWarehouse::over_adbc(
            source(),
            SourcePosture::SharedServiceUser {
                declared: declared.clone(),
            },
            ProjectId::parse(required("SUTURA_BQ_PROJECT")).expect("the declared project is a project id"),
            DatasetId::parse(required("SUTURA_BQ_DATASET")).expect("the declared dataset is a dataset id"),
            required("SUTURA_BIGQUERY_ADBC_DRIVER"),
            Impersonation::Disabled,
        );
        let ours = String::from(
            shared
                .session_user(&Presented::SharedServiceUser { declared })
                .expect("the dataset answered the identity read")
                .as_str(),
        );
        for declared_account in ["SUTURA_BQ_ACCOUNT_A", "SUTURA_BQ_ACCOUNT_B"] {
            assert_ne!(
                ours,
                required(declared_account),
                "this venue's own identity IS one of the declared accounts, so the leg beside this one \
                 cannot tell impersonation from the deployment answering as itself"
            );
        }
    }
}
