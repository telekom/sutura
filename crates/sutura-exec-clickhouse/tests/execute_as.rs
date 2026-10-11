#![forbid(unsafe_code)]
//! `EXECUTE AS` against the tier's server: the half of per-caller execution only a server answers.
//! `nix/clickhouse-tier-provision.sh` declares the users, their grants and the row policy read here.

#[cfg(test)]
mod execute_as {
    use std::path::Path;

    use sutura_conformance::corpus;
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_domain::identity::{Presented, PrincipalName};
    use sutura_domain::plan::Executable;
    use sutura_domain::source::SourcePosture;
    use sutura_domain::warehouse::Warehouse as _;
    use sutura_exec_clickhouse::ClickHouseWarehouse;
    use sutura_exec_clickhouse::fixture::credential_from_env;
    use sutura_exec_clickhouse::transport::{Endpoint, Http};

    /// The one database the tier's row policy names, so it is shared rather than per test: only
    /// [`each_declared_user_reads_only_the_rows_its_own_policy_admits`] writes to it.
    const DATABASE: &str = "execute_as";

    fn open(case: &str) -> Option<ClickHouseWarehouse<Http>> {
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
            DATABASE,
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
    /// the same plan, the same service-user connection, two answers.
    #[test]
    fn each_declared_user_reads_only_the_rows_its_own_policy_admits() {
        let Some(warehouse) = open("rows") else { return };
        warehouse
            .load_conformance_csv(&corpus::table(), &corpus::on_disk())
            .unwrap_or_else(|e| panic!("clickhouse could not load the conformance corpus: {e}"));
        let case = corpus::cases()
            .into_iter()
            .find(|case| case.name() == "decimal-total-by-day")
            .expect("the corpus has the case");
        let rows_as = |name: &str| {
            let presented = Presented::SubjectPrincipal {
                name: PrincipalName::parse(name).expect("a tier user is a principal"),
            };
            warehouse
                .execute(Executable::Query(case.plan()), &presented, corpus::deadline())
                .unwrap_or_else(|e| panic!("{name}: {e}"))
                .rows()
        };
        assert_eq!(rows_as("sutura_analyst_a"), case.expected().rows().len());
        assert_eq!(rows_as("sutura_analyst_b"), 0);
    }
}
