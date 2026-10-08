#![forbid(unsafe_code)]
//! The VERIFYING half of the source channel, against the tier's real server.
//!
//! **This is the cell issue 125 could not write before the fact: a chain is actually verified.**
//! `sutura-adbc-postgres`'s `src/conninfo.rs` proves the string libpq is told; nothing there connects, so nothing there
//! shows a handshake failing. These cells do, through the real driver's libpq. The negatives are the ones that matter: a server whose chain is signed by an
//! issuer the declared anchors do not name is REFUSED, and the tier's mutual-only role refuses a
//! client that presents no certificate.
//!
//! # Where this runs, and what it means when it does not
//!
//! `nix/postgres-tier.nix` publishes one loopback TCP listener beside its unix socket, with a
//! generated certificate, and exports the anchor under `SUTURA_POSTGRES_TIER_CA` from the same
//! `credentials` subcommand that publishes the password. Every venue that runs the suite provisions
//! that tier, so these cells RUN there: `checks.nextest` evals `credentials` in its own phase, and
//! `just test` reaches the same export through `nix/with-tier.sh`. Where nothing required a tier,
//! `provisioned::here` writes its `SKIPPED` notice to stderr and these cells return without
//! asserting - the same direction every other tier-backed cell in this workspace takes, and the
//! reason the notice is not duplicated here.
//!
//! Together with `tests/conformance.rs`'s unix-socket connection, these are the three declared
//! states: plaintext, verified TLS, and mutual TLS. The limit is identity: the client certificate
//! authenticates this DEPLOYMENT as one static database role. It is not a calling subject and is no
//! evidence for per-subject impersonation.

// `#[cfg(test)]` for the reason `tests/conformance.rs` gives: clippy honours `allow-expect-in-tests`
// only under a literal `#[cfg(test)]` ancestor, and without it every `expect` below is a lint error
// under `-D warnings`.
#[cfg(test)]
mod tls {
    use std::path::{Path, PathBuf};

    use sutura_conformance::corpus;
    use sutura_dev::provisioned;
    use sutura_domain::raw::RawStatement;
    use sutura_domain::warehouse::Warehouse as _;
    use sutura_exec_postgres::PostgresError;
    use sutura_exec_postgres::adbc::{AdbcError, AdbcPostgres, Channel};
    use sutura_exec_postgres::fixture::FixtureCredential;
    use sutura_tls::{Anchors, Identity};

    /// The service the provisioner is asked for - the same name `nix/postgres-tier.nix` publishes.
    const SERVICE: &str = "postgres";

    /// The loopback literal the tier's generated certificate carries an IP SAN for.
    const LOOPBACK: &str = "127.0.0.1";

    /// The anchor the tier exports, beside the three credential variables.
    const ANCHOR: &str = "SUTURA_POSTGRES_TIER_CA";

    /// The client pair the tier signs and demands for its mutual-only role.
    const CLIENT_CERTIFICATE: &str = "SUTURA_POSTGRES_TIER_CLIENT_CERT";
    const CLIENT_KEY: &str = "SUTURA_POSTGRES_TIER_CLIENT_KEY";

    /// The database role whose `pg_hba.conf` line uses certificate authentication.
    const MUTUAL_ROLE: &str = "sutura_mtls";

    struct Tier {
        port: u16,
        anchor: PathBuf,
        client_certificate: PathBuf,
        client_key: PathBuf,
    }

    /// One required path from the tier's `credentials` output.
    fn required_path(variable: &str) -> PathBuf {
        std::env::var(variable).map_or_else(
            |_| panic!("the postgres tier is present but {variable} is not published"),
            PathBuf::from,
        )
    }

    /// The tier's loopback port and TLS material, or `None` where nothing is provisioned.
    fn tier() -> Option<Tier> {
        let found = provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), SERVICE);
        let port = found.endpoint()?.port();
        // These come from the same `credentials` the password does, so a server that is there and
        // any missing value is a half-run provisioner. It must fail, never turn the TLS suite into
        // an absent-tier skip.
        Some(Tier {
            port,
            anchor: required_path(ANCHOR),
            client_certificate: required_path(CLIENT_CERTIFICATE),
            client_key: required_path(CLIENT_KEY),
        })
    }

    /// One statement over `channel` to the tier's loopback listener, as `credential` - `None` where
    /// it answered, and the refusal where it did not.
    fn refused(credential: &FixtureCredential, port: u16, channel: Channel<'_>) -> Option<PostgresError> {
        let conninfo = credential
            .conninfo(&corpus::source(), LOOPBACK, port, channel)
            .expect("the declared channel builds a connection string");
        let warehouse = AdbcPostgres::new(
            corpus::source(),
            corpus::posture(),
            sutura_exec_postgres::adbc::PostgresDriver::from_host().expect("the tier is up, so a driver is named"),
            conninfo,
        )
        .expect("the default ceiling parses");
        let statement = RawStatement::parse("select 1").expect("a test statement is a statement");
        warehouse
            .execute_raw(&statement, &corpus::presented(), corpus::deadline())
            .and_then(Result::err)
    }

    fn credential() -> FixtureCredential {
        FixtureCredential::from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"))
    }

    /// A connect libpq refused - the handshake, not anything a statement did.
    fn refused_at_connect(error: &PostgresError) -> bool {
        matches!(
            *error,
            PostgresError::Adbc {
                cause: AdbcError::Adbc(_)
            }
        )
    }

    #[test]
    fn a_source_chain_from_the_declared_anchor_is_verified_and_answers() {
        let Some(tier) = tier() else {
            return;
        };
        // An answer IS the proof: libpq ran `verify-full` against the declared anchor alone.
        let refused = refused(&credential(), tier.port, Channel::Verified(&Anchors::Bundle(tier.anchor)));
        assert!(
            refused.is_none(),
            "a source verifying the tier's own chain answers: {refused:?}"
        );
    }

    #[test]
    fn a_mutual_source_presents_the_identity_the_server_demands() {
        let Some(tier) = tier() else {
            return;
        };
        let identity = Identity::new(tier.client_certificate, tier.client_key);
        let refused = refused(
            &credential().as_role(MUTUAL_ROLE),
            tier.port,
            Channel::Mutual(&Anchors::Bundle(tier.anchor), &identity),
        );
        assert!(
            refused.is_none(),
            "the mutual-only role accepts the identity the tier signed: {refused:?}"
        );
    }

    #[test]
    fn a_mutual_role_refuses_a_client_that_presents_no_certificate() {
        let Some(tier) = tier() else {
            return;
        };
        let error = refused(
            &credential().as_role(MUTUAL_ROLE),
            tier.port,
            Channel::Verified(&Anchors::Bundle(tier.anchor)),
        )
        .expect("the mutual-only role must reject a client with no certificate");
        assert!(refused_at_connect(&error), "{error:?}");
    }

    #[test]
    fn a_source_chain_from_an_untrusted_issuer_is_refused() {
        let Some(tier) = tier() else {
            return;
        };
        // An anchor that signs nothing the server presents. If verification were not happening, this
        // would connect exactly as the first cell does.
        let untrusted = rcgen::generate_simple_self_signed([String::from("not-the-tier")]).expect("a self-signed pair generates");
        let anchor = std::env::temp_dir().join(format!("sutura-pg-untrusted-{}.pem", std::process::id()));
        std::fs::write(&anchor, untrusted.cert.pem()).expect("the untrusted anchor writes");
        let outcome = refused(&credential(), tier.port, Channel::Verified(&Anchors::Bundle(anchor.clone())));
        let _ignored = std::fs::remove_file(&anchor);
        let error = outcome.expect("a chain the declared anchors do not name is refused");
        assert!(refused_at_connect(&error), "{error:?}");
    }
}
