#![forbid(unsafe_code)]
//! A declared Kerberos sign-in, through `Conninfo::kerberos`, `AdbcPostgres` and the LINKED static
//! driver, against `nix/kerberos-tier.sh`'s KDC and its PostgreSQL that admits GSSAPI-encrypted
//! GSSAPI and nothing else.
//!
//! `#[ignore]`d, because only one venue has both halves: `nix/shipped.nix`'s
//! `adbc-postgres-kerberos-x86_64-unknown-linux-musl-test` starts the tier, sources its `env`, builds
//! this file for `x86_64` musl with the driver archive linked, and runs it with `--ignored`. That
//! derivation also requires the first cell's marker line in its log, so a build that skipped or lost
//! the cell is red there, not green. `nix/bigquery-driver-check.sh` realises it in CI.
//!
//! What this does not reach: TLS with Kerberos inside it, the mounted driver's libpq and any
//! principal but the one the named credential cache holds - `conninfo.rs`'s limits say why that is
//! one per process.

#[cfg(test)]
mod kerberos {
    use sutura_conformance::corpus;
    use sutura_domain::raw::RawStatement;
    use sutura_domain::warehouse::{RawRows, Value, Warehouse as _};
    use sutura_exec_postgres::PostgresError;
    use sutura_exec_postgres::adbc::{AdbcPostgres, Channel, Conninfo, GssEncryption, Kerberos, KerberosService, PostgresDriver};
    use sutura_exec_postgres::connection::ConnectionTarget;

    /// Signs in as the tier's keytab, asking for a ticket to `<service>/localhost`, and reads back
    /// what the server recorded about this backend's sign-in.
    fn signed_in(service: &str) -> Result<RawRows, PostgresError> {
        transport(service)
            .execute_raw(&statement(), &corpus::presented(), corpus::deadline())
            .expect("the ADBC transport runs raw statements")
    }

    fn transport(service: &str) -> AdbcPostgres {
        let port = std::env::var("SUTURA_KERBEROS_TIER_PORT")
            .expect("the venue sources the tier's env")
            .parse()
            .expect("the tier's port is a port");
        let driver = PostgresDriver::linked_in().expect("the venue links the driver archive");
        let service = KerberosService::parse(service).expect("a test service is a service");
        let kerberos = Kerberos::new(service, GssEncryption::Required);
        let target = ConnectionTarget::Host("localhost");
        let conninfo = Conninfo::kerberos(
            &corpus::source(),
            target,
            port,
            "postgres",
            "sutura",
            &kerberos,
            Channel::Plaintext,
        )
        .expect("the tier's env names a keytab");
        AdbcPostgres::new(corpus::source(), corpus::posture(), driver, conninfo).expect("the default ceiling parses")
    }

    fn statement() -> RawStatement {
        RawStatement::parse(
            "SELECT current_user || ' ' || gss_authenticated || ' ' || encrypted || ' ' || principal \
             FROM pg_stat_gssapi WHERE pid = pg_backend_pid()",
        )
        .expect("a test statement is a statement")
    }

    #[test]
    #[ignore = "needs the Kerberos tier and the linked driver: `adbc-postgres-kerberos-x86_64-unknown-linux-musl-test`"]
    fn the_linked_driver_signs_in_with_kerberos_over_a_gssapi_encrypted_channel() {
        let rows = signed_in("postgres").expect("the KDC issues a ticket for postgres/localhost");
        assert_eq!(
            rows.rows(),
            [vec![Value::Text(String::from("sutura true true sutura@SUTURA.TEST"))]]
        );
        println!("linked-postgres-driver-signed-in-with-kerberos");
    }

    #[test]
    #[ignore = "needs the Kerberos tier and the linked driver: `adbc-postgres-kerberos-x86_64-unknown-linux-musl-test`"]
    fn a_declared_service_the_kdc_does_not_know_is_refused_rather_than_signed_in() {
        // The negative control for the cell above: `krbsrvname` reaches libpq, so a service name the
        // KDC holds no key for fails the ticket request, and nothing falls back to another method.
        let refused = signed_in("postgresql").expect_err("no key exists for postgresql/localhost");
        let said = format!("{refused:?}");
        assert!(said.contains("not found in Kerberos database"), "{said}");
    }
}
