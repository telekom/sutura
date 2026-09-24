#![forbid(unsafe_code)]

#[cfg(test)]
mod conformance {
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use sutura_conformance::{Fixture, Missing, corpus};
    use sutura_dev::provisioned::{self, Provisioned};
    use sutura_exec_clickhouse::ClickHouseWarehouse;
    use sutura_exec_clickhouse::fixture::credential_from_env;
    use sutura_exec_clickhouse::transport::{Endpoint, Http};

    const SERVICE: &str = "clickhouse";

    fn open() -> Fixture<ClickHouseWarehouse<Http>> {
        let endpoint = match provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), SERVICE) {
            Provisioned::At(endpoint) => endpoint,
            Provisioned::Skipped(absent) => return Fixture::Absent(Missing::tier(SERVICE, &absent)),
        };
        let auth = credential_from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        let database = format!("conformance_{}_{}", std::process::id(), schema_counter());
        let warehouse = ClickHouseWarehouse::connect_in_database(
            corpus::source(),
            corpus::posture(),
            Endpoint::plaintext(endpoint.host(), endpoint.port()),
            auth,
            &database,
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
        warehouse: sutura_exec_clickhouse::ClickHouseWarehouse<sutura_exec_clickhouse::transport::Http>,
        open: crate::conformance::open,
        refuses_legs,
    }
}
