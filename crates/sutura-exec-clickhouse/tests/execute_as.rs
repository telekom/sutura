#![forbid(unsafe_code)]
//! Per-caller execution against the tier's server - `EXECUTE AS`, and a caller signed in with their
//! own token - the half only a server answers. `nix/clickhouse-tier-provision.sh` declares the
//! users, their grants, the row policies and the test-only token checker read here.

#[cfg(test)]
mod execute_as {
    use std::path::Path;

    use sutura_conformance::corpus;
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_domain::identity::{Presented, PrincipalName, Secret};
    use sutura_domain::plan::Executable;
    use sutura_domain::source::SourcePosture;
    use sutura_domain::warehouse::Warehouse as _;
    use sutura_exec_clickhouse::ClickHouseWarehouse;
    use sutura_exec_clickhouse::fixture::credential_from_env;
    use sutura_exec_clickhouse::transport::{Endpoint, Http};

    fn open(case: &str, database: &str) -> Option<ClickHouseWarehouse<Http>> {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "clickhouse") {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(absent) => {
                eprintln!("execute_as::{case}: NOT RUN - {absent}");
                return None;
            }
        };
        let auth = credential_from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        let warehouse = ClickHouseWarehouse::connect_in_database(
            corpus::source(),
            SourcePosture::ImpersonationAtSource,
            Endpoint::plaintext(endpoint.host(), endpoint.port()),
            auth,
            database,
            budget(),
        )
        .unwrap_or_else(|e| panic!("clickhouse did not open at {endpoint}: {e}"));
        Some(warehouse)
    }

    fn budget() -> sutura_domain::warehouse::ResultBudget {
        sutura_domain::warehouse::ResultBudget::of_bytes(
            core::num::NonZeroUsize::new(64 * 1024 * 1024).expect("a test budget is positive"),
        )
    }

    /// Rows differ per caller because the SERVER evaluates each declared user's own row policy:
    /// the same plan, the same service-user connection, two answers. A user the service user may not
    /// become is refused by the server - never answered as the service user. In this one cell because
    /// it is the one that loads this database's table.
    #[test]
    fn each_declared_user_reads_only_the_rows_its_own_policy_admits() {
        // The one database this row policy names: only this cell writes to it.
        let Some(warehouse) = open("rows", "execute_as") else { return };
        warehouse
            .load_conformance_csv(&corpus::table(), &corpus::on_disk())
            .unwrap_or_else(|e| panic!("clickhouse could not load the conformance corpus: {e}"));
        let case = corpus::cases()
            .into_iter()
            .find(|case| case.name() == "decimal-total-by-day")
            .expect("the corpus has the case");
        let run_as = |name: &str| {
            let presented = Presented::SubjectPrincipal {
                name: PrincipalName::parse(name).expect("a tier user is a principal"),
            };
            warehouse.execute(Executable::Query(case.plan()), &presented, corpus::deadline())
        };
        let rows_as = |name: &str| run_as(name).unwrap_or_else(|e| panic!("{name}: {e}")).rows();
        assert_eq!(rows_as("sutura_analyst_a"), case.expected().rows().len());
        assert_eq!(rows_as("sutura_analyst_b"), 0);
        let refused = run_as("sutura_ungranted").expect_err("a user with no IMPERSONATE grant is refused");
        assert!(warehouse.source_refused(&refused), "{refused}");
    }

    /// The tier's checker binds the token's `sub` to the Basic user: `sutura_caller_a`'s token signs
    /// in as `sutura_caller_a` and reads that user's rows, and the same token beside
    /// `sutura_caller_b` is refused by the server.
    #[test]
    fn a_callers_token_signs_in_only_as_the_user_it_names() {
        // The one database this row policy names: only this cell writes to it.
        let Some(warehouse) = open("signed_in", "signed_in") else {
            return;
        };
        warehouse
            .load_conformance_csv(&corpus::table(), &corpus::on_disk())
            .unwrap_or_else(|e| panic!("clickhouse could not load the conformance corpus: {e}"));
        let token = std::env::var("SUTURA_CLICKHOUSE_TIER_TOKEN")
            .unwrap_or_else(|_absent| panic!("the tier publishes SUTURA_CLICKHOUSE_TIER_TOKEN"));
        let case = corpus::cases()
            .into_iter()
            .find(|case| case.name() == "decimal-total-by-day")
            .expect("the corpus has the case");
        let signed_in_as = |name: &str| {
            let presented = Presented::SubjectSignIn {
                name: PrincipalName::parse(name).expect("a tier user is a principal"),
                material: Secret::new(token.clone()),
            };
            warehouse.execute(Executable::Query(case.plan()), &presented, corpus::deadline())
        };
        let rows = signed_in_as("sutura_caller_a").unwrap_or_else(|e| panic!("sutura_caller_a: {e}"));
        assert_eq!(rows.rows(), case.expected().rows().len());
        let refused = signed_in_as("sutura_caller_b").expect_err("a token signs in only as its own `sub`");
        assert!(warehouse.source_refused(&refused), "{refused}");
    }
}
