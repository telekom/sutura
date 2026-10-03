#![forbid(unsafe_code)]
//! A declared OAuth sign-in, through `Conninfo::oauth`, `AdbcPostgres` and the LINKED static driver,
//! against `nix/oauth-tier.sh`'s PostgreSQL that admits only TLS with an authorised OAuth bearer.
//!
//! `#[ignore]`d, because only one venue has both halves: `nix/shipped.nix`'s
//! `adbc-postgres-sign-in-x86_64-unknown-linux-musl-test` starts the tier, sources its `env`, builds
//! this file for `x86_64` musl with the driver archive linked, and runs it with `--ignored`. That
//! derivation also requires the first cell's marker line in its log, so a build that skipped or lost
//! the cell is red there, not green. `nix/bigquery-driver-check.sh` realises it in CI.
//!
//! What this does not reach: a real issuer's JWT - `nix/oauth-validator.c` checks a fixed string,
//! never a JWT, and a deployment's validator is the operator's. Nor the mounted driver's libpq, nor
//! token expiry or refresh - the tier hands libpq one bearer and lets it sign in or refuse.

#[cfg(test)]
mod oauth {
    use std::path::PathBuf;

    use sutura_conformance::corpus;
    use sutura_domain::identity::Secret;
    use sutura_domain::raw::RawStatement;
    use sutura_domain::warehouse::{RawRows, Value, Warehouse as _};
    use sutura_exec_postgres::PostgresError;
    use sutura_exec_postgres::adbc::{AdbcPostgres, Channel, Conninfo, OAuth, PostgresDriver};
    use sutura_exec_postgres::connection::ConnectionTarget;
    use sutura_exec_postgres::tls::TlsAnchors;

    /// Signs in as the tier's role `sutura` with `bearer`, and reads back what the server recorded
    /// this backend's sign-in as.
    fn read_system_user(bearer: &str) -> Result<RawRows, PostgresError> {
        transport(bearer)
            .execute_raw(&statement(), &corpus::presented(), corpus::deadline())
            .expect("the ADBC transport runs raw statements")
    }

    fn transport(bearer: &str) -> AdbcPostgres {
        let port = std::env::var("SUTURA_OAUTH_TIER_PORT")
            .expect("the venue sources the tier's env")
            .parse()
            .expect("the tier's port is a port");
        let ca = std::env::var("SUTURA_OAUTH_TIER_CA").expect("the venue sources the tier's env");
        let driver = PostgresDriver::linked_in().expect("the venue links the driver archive");
        let oauth = OAuth::new("https://issuer.example", "sutura").expect("a test declaration is one");
        let anchors = TlsAnchors::Bundle(PathBuf::from(ca));
        let target = ConnectionTarget::Host("localhost");
        let conninfo = Conninfo::oauth(
            &corpus::source(),
            target,
            port,
            "postgres",
            "sutura",
            &oauth,
            &Secret::new(bearer),
            Channel::Verified(&anchors),
        )
        .expect("the tier's CA and a bearer build");
        AdbcPostgres::new(corpus::source(), corpus::posture(), driver, conninfo).expect("the default ceiling parses")
    }

    fn statement() -> RawStatement {
        RawStatement::parse("SELECT system_user").expect("a test statement is a statement")
    }

    #[test]
    #[ignore = "needs the OAuth tier and the linked driver: `adbc-postgres-sign-in-x86_64-unknown-linux-musl-test`"]
    fn the_linked_driver_signs_in_with_an_authorised_oauth_bearer_over_a_verified_channel() {
        let rows = read_system_user("sutura-tier-bearer-sutura").expect("the tier admits sutura's own bearer");
        assert_eq!(rows.rows(), [vec![Value::Text(String::from("oauth:sutura"))]]);
        println!("linked-postgres-driver-signed-in-with-oauth");
    }

    #[test]
    #[ignore = "needs the OAuth tier and the linked driver: `adbc-postgres-sign-in-x86_64-unknown-linux-musl-test`"]
    fn another_roles_bearer_is_refused_rather_than_signed_in() {
        // The negative control for the cell above: a bearer that would authorise role `other` is
        // handed to libpq while signing in as `sutura`, so the validator refuses it and nothing
        // falls back to another method.
        let refused = read_system_user("sutura-tier-bearer-other").expect_err("another role's bearer must not sign in");
        let said = format!("{refused:?}");
        assert!(said.contains("OAuth bearer authentication failed"), "{said}");
    }
}
