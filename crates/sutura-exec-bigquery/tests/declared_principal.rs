#![forbid(unsafe_code)]
//! Leg 2 for `BigQuery`, as a hosted leg: does a declared subject's question really execute as that
//! subject's own principal at the identity pool, rather than as this deployment?
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
//! ADBC path, the deployment's own identity is neither of them, and - since the credential each
//! question runs under is built from that subject's own assertion - Google's token service accepted
//! that assertion against the declared pool. A run here is what turns leg 2 from wired into proven.
//!
//! Does NOT establish: which ROWS each principal may read, which needs the served surface and the
//! two accounts' grants.
//!
//! **What it now CAN establish and deliberately does not.** Since `telekom/sutura#929` F3 the
//! credential document names the account declared for the subject as its
//! `service_account_impersonation_url`, so `SESSION_USER()` should read as that exact address and
//! equality against it is available for the first time. This leg still asserts only *two subjects,
//! two distinct principals, neither of them the deployment's*, because a stronger expectation
//! written here would be a claim nobody has run: the address is supplied from the environment, so
//! an equality assertion that has never executed would read as a proven binding and is not one. The
//! two addresses ARE required from the environment now, which is what makes each subject's document
//! name a different account rather than both naming one.

#![cfg(feature = "adbc")]

// One `#[cfg(test)]` module holding the whole leg, which is this workspace's shape for an
// integration test target - `tests/conformance.rs` states it in the same words: the strict lints
// exempt test code, and a helper at file scope is not test code as far as clippy is concerned.
#[cfg(test)]
mod declared_principal {

    use sutura_domain::identity::{Presented, Secret};
    use sutura_domain::model::SourceName;
    use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
    use sutura_exec_bigquery::BigQueryWarehouse;
    use sutura_exec_bigquery::adbc::{DriverLocation, Impersonation, WorkloadPool};
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

    /// The driver this leg opens.
    ///
    /// A MOUNTED one, and that is the only route a cargo test binary has: the archive a release
    /// artefact links in is supplied by `nix/shipped.nix` for the four release triples, and this
    /// binary is none of them. So this venue measures the driver and not the packaging -
    /// `nix/bigquery-driver-check.sh` is where the packaging is measured.
    fn driver() -> DriverLocation {
        DriverLocation::parse(&required("SUTURA_BIGQUERY_ADBC_DRIVER")).expect("the declared driver path is absolute")
    }

    /// The adapter, opened `impersonation-at-source` over the real driver at the real project.
    fn opened() -> BigQueryWarehouse<sutura_exec_bigquery::adbc::AdbcBigQuery> {
        let project = ProjectId::parse(required("SUTURA_BQ_PROJECT")).expect("the declared project is a project id");
        let dataset = DatasetId::parse(required("SUTURA_BQ_DATASET")).expect("the declared dataset is a dataset id");
        let pool = WorkloadPool::parse(&required("SUTURA_BQ_AUDIENCE")).expect("the declared pool audience is usable");
        BigQueryWarehouse::over_adbc(
            source(),
            SourcePosture::ImpersonationAtSource,
            project,
            dataset,
            driver(),
            Impersonation::ThroughPool(pool),
        )
    }

    /// The identity a question runs as when this source is asked with `assertion`, for a caller
    /// the deployment declared should execute as `account`.
    ///
    /// **`SubjectToken` and not `SubjectPrincipal`**, which is not a spelling choice: this adapter
    /// refuses the principal shape (`BigQueryError::NoPrincipalSwitch`), so a cell that built one
    /// would fail at the seam and never reach a dataset. The assertion is the caller's own verified
    /// document, and the transport puts it behind a workload-identity credential document for the
    /// driver to federate and then impersonate `account` with.
    ///
    /// **Both values are per caller here, which is what the pre-F3 shape could not express.** A
    /// leg built with one account for both subjects would run both questions as one principal and
    /// the cell below would go red for a reason that is not the mechanism failing - so the two
    /// addresses come from two variables, and `each_subject_executes_as_its_own_principal_at_the_declared_pool`
    /// refuses to run without either.
    fn executed_as(assertion: &str, account: &str) -> String {
        let warehouse = opened();
        let presented = Presented::SubjectToken {
            material: Secret::new(assertion),
            impersonate: Some(
                sutura_domain::identity::PrincipalName::parse(account).expect("the declared account is a principal name"),
            ),
        };
        String::from(
            warehouse
                .session_user(&presented)
                .expect("the dataset answered the identity read")
                .as_str(),
        )
    }

    #[test]
    #[ignore = "needs a real BigQuery project, the driver .so, and two assertions the declared pool accepts"]
    fn each_subject_executes_as_its_own_principal_at_the_declared_pool() {
        // **THE leg-2 bar, and it is the same one the withdrawn HTTP venue met**: two distinct
        // subjects resolve to two distinct BigQuery principals, observed at the data system rather
        // than at a seam.
        //
        // **What the oracle now returns, and why the expectation changed with the mechanism.** The
        // credential document names no `service_account_impersonation_url`, so the credential IS the
        // principal the pool resolved the subject to and `SESSION_USER()` reads as that. An earlier
        // round of this cell compared the answer to the service-account address `impersonate`
        // declared, and its comment said a `principal://` string would mean the hop did not happen -
        // both of which were true of the DELETED principal switch and neither of which is true here.
        // So this asserts what the mechanism actually produces: two subjects, two distinct
        // principals, from the subjects' own assertions.
        let first_assertion = required("SUTURA_BQ_ASSERTION_A");
        let second_assertion = required("SUTURA_BQ_ASSERTION_B");
        assert_ne!(
            first_assertion, second_assertion,
            "one assertion presented twice is one subject, and proves nothing about two"
        );
        let first_account = required("SUTURA_BQ_ACCOUNT_A");
        let second_account = required("SUTURA_BQ_ACCOUNT_B");
        assert_ne!(
            first_account, second_account,
            "one declared account for both subjects runs both questions as one principal, whatever the pool resolved"
        );
        let ran_as_first = executed_as(&first_assertion, &first_account);
        let ran_as_second = executed_as(&second_assertion, &second_account);
        assert!(!ran_as_first.trim().is_empty(), "the dataset named no identity at all");
        assert_ne!(
            ran_as_first, ran_as_second,
            "both subjects resolved to one principal, which is the shared-identity outcome wearing leg 2's name"
        );
    }

    #[test]
    #[ignore = "needs a real BigQuery project, the driver .so, and the running identity's own account"]
    fn the_deployments_own_identity_is_neither_subjects_principal() {
        // **THE CONTROL, and without it the cell above cannot tell federation from the deployment
        // answering as itself.** A shared leg carries no subject, so this read goes out as whatever
        // the driver's application default credentials authenticate as. Compared against the two
        // subjects' OWN observed principals rather than against the declared accounts: the control
        // has to hold whatever the declaration says, and comparing against a declared address would
        // pass on a deployment where the second hop silently did not happen.
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
            driver(),
            Impersonation::Disabled,
        );
        let ours = String::from(
            shared
                .session_user(&Presented::SharedServiceUser { declared })
                .expect("the dataset answered the identity read")
                .as_str(),
        );
        for (assertion, account) in [
            ("SUTURA_BQ_ASSERTION_A", "SUTURA_BQ_ACCOUNT_A"),
            ("SUTURA_BQ_ASSERTION_B", "SUTURA_BQ_ACCOUNT_B"),
        ] {
            assert_ne!(
                ours,
                executed_as(&required(assertion), &required(account)),
                "a subject's question ran as this deployment's own identity, so the leg beside this one \
                 cannot tell federation from the deployment answering as itself"
            );
        }
    }
}
