#![forbid(unsafe_code)]
//! The linked PostgreSQL and `DuckDB` drivers: refused by name in a source build, and RUNNING
//! beside the `BigQuery` one where a build links all three archives.
//!
//! One cell per driver, two arms each, and the `cfg` decides which arm is CORRECT: a build that
//! linked the archive and got an `Err`, or linked none and got a driver, fails either way. Every
//! `cargo` gate here takes the unlinked arms. The linked arms' venue is `nix/shipped.nix`'s
//! `adbc-drivers-linked-x86_64-unknown-linux-musl-test`, a static musl build of this file that
//! `nix/bigquery-driver-check.sh` realises on x86_64-linux - and that derivation also requires each
//! linked arm's marker line in the log, so a build that stopped linking an archive (and so took the
//! unlinked arm, green) is red there.
//!
//! The mounted `DuckDB` cell is the one every `cargo` gate RUNS: nixpkgs' `libduckdb`, the library
//! `nix/duckdb-adbc.nix` merges into the linked archive, opened by path.

// `cfg(test)` for the reason `crates/sutura-sql/tests/adversarial_findings.rs` gives: clippy's
// `allow-*-in-tests` reach only code inside a `#[cfg(test)]` item.
#[cfg(test)]
mod tests {
    use adbc_core::error::Status;
    use adbc_core::options::{AdbcVersion, OptionDatabase, OptionValue};
    use adbc_core::{Connection as _, Database as _, Driver as _, Statement as _};
    use adbc_driver_manager::ManagedDriver;
    use arrow_array::RecordBatch;
    use arrow_array::cast::AsArray as _;
    use arrow_array::types::Int32Type;

    /// The PostgreSQL driver is there exactly where its archive is linked - and where it is not, it
    /// is refused by name rather than handed out as the `BigQuery` driver, which the shared
    /// `AdbcDriverInit` symbol once made possible.
    ///
    /// Linked, both drivers initialise in one static binary and the PostgreSQL one executes libpq: a
    /// database on a refused port fails with libpq's own connect error, which only the statically
    /// linked libpq can have produced. What that does not reach: a server. The TCP connect is
    /// refused before any TLS, so OpenSSL is linked here and never negotiates.
    #[test]
    fn the_postgres_driver_is_there_exactly_where_its_archive_is_linked() {
        match (cfg!(adbc_postgres_driver_linked), sutura_adbc::linked_postgres_driver()) {
            (false, Err(refused)) => {
                assert_eq!(refused.status, Status::NotFound, "{refused:?}");
                assert!(refused.message.contains("PostgreSQL"), "{refused:?}");
            }
            (true, Ok(mut postgres)) => {
                sutura_adbc::linked_driver().expect("the BigQuery driver initialises beside it");
                let uri = OptionValue::String(String::from("postgresql://127.0.0.1:1/none?connect_timeout=5"));
                let Err(refused) = postgres.new_database_with_opts([(OptionDatabase::Uri, uri)]) else {
                    panic!("nothing listens on port 1, so no database opens");
                };
                assert_eq!(refused.status, Status::IO, "{refused:?}");
                assert!(refused.message.contains("[libpq] Failed to connect"), "{refused:?}");
                // The prefix alone is the driver's for ANY refusal, a rejected option included; this
                // is libpq's own text for a socket it opened and was refused on.
                assert!(refused.message.contains("port 1 failed"), "{refused:?}");
                println!("linked-postgres-driver-ran-libpq");
            }
            (linked, other) => panic!("archive linked: {linked}, yet the driver is {other:?}"),
        }
    }

    /// The `DuckDB` driver is there exactly where its archive is linked, and linked it RUNS a query -
    /// its whole engine is in the static binary, not only the entrypoint.
    #[test]
    fn the_duckdb_driver_is_there_exactly_where_its_archive_is_linked() {
        match (cfg!(adbc_duckdb_driver_linked), sutura_adbc::linked_duckdb_driver()) {
            (false, Err(refused)) => {
                assert_eq!(refused.status, Status::NotFound, "{refused:?}");
                assert!(refused.message.contains("DuckDB"), "{refused:?}");
            }
            (true, Ok(mut duckdb)) => {
                answers_select_one(&mut duckdb);
                println!("linked-duckdb-driver-ran-select-1");
            }
            (linked, other) => panic!("archive linked: {linked}, yet the driver is {other:?}"),
        }
    }

    /// A mounted `DuckDB` opens by the entrypoint `mounted_duckdb_driver` passes, and by no name the
    /// driver manager would derive - so the passing is load-bearing, not ceremony.
    #[test]
    fn a_mounted_duckdb_opens_by_its_own_entrypoint_and_not_by_a_derived_one() {
        let dir = std::env::var_os("DUCKDB_LIB_DIR").expect("the dev shell and every nix check set DUCKDB_LIB_DIR");
        let library = std::path::Path::new(&dir).join(format!(
            "{}duckdb{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        ));
        let Err(derived) = ManagedDriver::load_dynamic_from_filename(&library, None, AdbcVersion::default()) else {
            panic!("{} defines a name the driver manager derives", library.display());
        };
        assert!(derived.message.contains("AdbcDriverInit"), "{derived:?}");
        let mut duckdb = sutura_adbc::mounted_duckdb_driver(&library).expect("the mounted DuckDB initialises");
        answers_select_one(&mut duckdb);
    }

    /// `SELECT 1` on an in-memory database answers one `INTEGER` row holding 1.
    fn answers_select_one(duckdb: &mut ManagedDriver) {
        let database = duckdb.new_database().expect("an in-memory database opens");
        let mut connection = database.new_connection().expect("a connection opens");
        let mut statement = connection.new_statement().expect("a statement opens");
        statement.set_sql_query("SELECT 1").expect("the query is accepted");
        let batches: Vec<RecordBatch> = statement
            .execute()
            .expect("SELECT 1 executes")
            .collect::<Result<_, _>>()
            .expect("every batch reads");
        let [batch] = batches.as_slice() else {
            panic!("one batch, got {}", batches.len());
        };
        let column = batch
            .column(0)
            .as_primitive_opt::<Int32Type>()
            .unwrap_or_else(|| panic!("an INTEGER column, got {}", batch.column(0).data_type()));
        assert_eq!(column.values(), &[1]);
    }
}
