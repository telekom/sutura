#![forbid(unsafe_code)]
//! The linked PostgreSQL driver: refused by name in a source build, and RUNNING beside the
//! `BigQuery` one where a build links both archives.
//!
//! One cell, two arms, and the `cfg` decides which arm is CORRECT: a build that linked the archive
//! and got an `Err`, or linked none and got a driver, fails either way. Every `cargo` gate here takes
//! the unlinked arm. The linked arm's venue is `nix/shipped.nix`'s
//! `adbc-drivers-linked-x86_64-unknown-linux-musl-test`, a static musl build of this file that
//! `nix/bigquery-driver-check.sh` realises on x86_64-linux - and that derivation also requires the
//! linked arm's marker line in the log, so a build that stopped linking the archive (and so took the
//! unlinked arm, green) is red there.

// `cfg(test)` for the reason `crates/sutura-sql/tests/adversarial_findings.rs` gives: clippy's
// `allow-*-in-tests` reach only code inside a `#[cfg(test)]` item.
#[cfg(test)]
mod tests {
    use adbc_core::Driver as _;
    use adbc_core::error::Status;
    use adbc_core::options::{OptionDatabase, OptionValue};

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
                println!("linked-postgres-driver-ran-libpq");
            }
            (linked, other) => panic!("archive linked: {linked}, yet the driver is {other:?}"),
        }
    }
}
