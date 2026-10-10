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
//! **Why the served half is still absent, measured rather than deferred** (`telekom/sutura#929`'s
//! re-review asked for it). Over the served surface a caller's capabilities are decided by the
//! `scope` claim of the token leg 1 verified, and a verified caller whose token names no capability
//! scope may invoke nothing - `sutura_http::capability`'s own header states it and
//! `sutura_http::inbound::tests::router`'s no-scope cell holds it. The SAME token - the leg-1
//! caller token, out of the `Authorization` header - is what `DeclaredPrincipalBroker` federates,
//! so ONE credential has to satisfy BOTH the capability gate and the identity pool's provider.
//! Neither candidate reachable from here does:
//!
//! * a Google-issued ID token satisfies the POOL only. `nix/bigquery-mint-assertion.sh` asks for a
//!   `target_audience` and asks for no scope, and nothing in the settings tree supplies one on a
//!   caller's behalf (`security.inbound` has no scope key), so such a caller answers `403`
//!   `insufficient_scope` before any source is asked.
//! * the Keycloak tier's tokens satisfy the GATE only - they carry this surface's scopes, and the
//!   tier serves them over a loopback listener with a throwaway CA, which Google's token service
//!   cannot fetch a key set from.
//!
//! So the served leg needs an issuer whose tokens carry this surface's capability scopes AND whose
//! key set the pool's provider can reach, which is what
//! `docs/where-identity-is-proven.md`'s served-binary row now names in its cost cell and is not
//! something this repository can mint. **Adding a settings key that granted capabilities to a
//! scopeless verified caller would close this by weakening the fail-closed gate, and is not on the
//! table here.**
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
//! **What it asserts: two subjects, two principals, neither the deployment's.** The credential
//! document names no second hop, so `SESSION_USER()` is each subject's own federated principal.
//! Its exact spelling is Google's, so the cell compares the two answers with each other and with
//! the deployment's own, and names no expected string.

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

    /// The money bound both legs here run under: a gibibyte, which is what `docs/adr/0017` says
    /// protects this venue's own project from a question that scans a partitioned table end to end.
    fn ceiling() -> sutura_exec_bigquery::adbc::BytesBilledCeiling {
        sutura_exec_bigquery::adbc::BytesBilledCeiling::parse(1024 * 1024 * 1024).expect("a gibibyte is a usable ceiling")
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
            ceiling(),
        )
    }

    /// The identity a question runs as when this source is asked with `assertion`.
    ///
    /// **`SubjectToken` and not `SubjectPrincipal`**, which is not a spelling choice: this adapter
    /// refuses the principal shape (`BigQueryError::NoPrincipalSwitch`), so a cell that built one
    /// would fail at the seam and never reach a dataset. The assertion is the caller's own verified
    /// document, and the transport puts it behind a workload-identity credential document for the
    /// driver to federate.
    fn executed_as(assertion: &str) -> String {
        let warehouse = opened();
        let presented = Presented::SubjectToken {
            material: Secret::new(assertion),
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
        // **THE leg-2 bar**: two distinct subjects resolve to two distinct BigQuery principals,
        // observed at the data system rather than at a seam. The first `assert_ne!` refuses a
        // MISCONFIGURED venue (one assertion presented twice) with a message naming it.
        let first_assertion = required("SUTURA_BQ_ASSERTION_A");
        let second_assertion = required("SUTURA_BQ_ASSERTION_B");
        assert_ne!(
            first_assertion, second_assertion,
            "one assertion presented twice is one subject, and proves nothing about two"
        );
        assert_ne!(
            executed_as(&first_assertion),
            executed_as(&second_assertion),
            "both subjects resolved to one principal, which is the shared-identity outcome wearing leg 2's name"
        );
    }

    #[test]
    #[ignore = "needs a real BigQuery project, the driver .so, and the running identity's own account"]
    fn the_deployments_own_identity_is_neither_subjects_principal() {
        // **THE CONTROL, and without it the cell above cannot tell federation from the deployment
        // answering as itself.** A shared leg carries no subject, so this read goes out as whatever
        // the driver's application default credentials authenticate as. Compared against the two
        // subjects' OWN observed principals.
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
            ceiling(),
        );
        let ours = String::from(
            shared
                .session_user(&Presented::SharedServiceUser { declared })
                .expect("the dataset answered the identity read")
                .as_str(),
        );
        for assertion in ["SUTURA_BQ_ASSERTION_A", "SUTURA_BQ_ASSERTION_B"] {
            assert_ne!(
                ours,
                executed_as(&required(assertion)),
                "a subject's question ran as this deployment's own identity, so the leg beside this one \
                 cannot tell federation from the deployment answering as itself"
            );
        }
    }
}
