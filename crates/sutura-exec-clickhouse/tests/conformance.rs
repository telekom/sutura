#![forbid(unsafe_code)]

#[cfg(test)]
mod conformance {
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use sutura_conformance::{Fixture, Missing, corpus};
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_exec_clickhouse::adbc::AdbcClickHouseWarehouse;
    use sutura_exec_clickhouse::fixture::credential_from_env;

    const SERVICE: &str = "clickhouse";

    fn open() -> Fixture<AdbcClickHouseWarehouse> {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), SERVICE) {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(absent) => return Fixture::Absent(Missing::tier(SERVICE, &absent)),
        };
        let credential = credential_from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        let database = format!("conformance_{}_{}", std::process::id(), schema_counter());
        let warehouse = AdbcClickHouseWarehouse::connect_in_database(
            corpus::source(),
            corpus::posture(),
            endpoint.host(),
            endpoint.port(),
            &credential,
            &database,
            sutura_domain::warehouse::ResultBudget::of_bytes(
                core::num::NonZeroUsize::new(1024 * 1024 * 1024).expect("a gibibyte is positive"),
            ),
        )
        .unwrap_or_else(|e| panic!("clickhouse did not open at {endpoint}: {e}"));
        warehouse
            .load_conformance_csv(&corpus::table(), &corpus::on_disk())
            .unwrap_or_else(|e| panic!("clickhouse could not load the conformance corpus: {e}"));
        Fixture::standing(warehouse)
    }

    fn schema_counter() -> usize {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    sutura_conformance::execute_packs! {
        adapter: clickhouse,
        warehouse: sutura_exec_clickhouse::adbc::AdbcClickHouseWarehouse,
        open: crate::conformance::open,
        executes_legs,
    }
}
