//! The VERIFYING half of the source channel, against the tier's real server.
//!
//! **This is the cell issue 125 could not write before the fact: a chain is actually verified.**
//! `crates/sutura-exec-postgres/src/tls.rs` proves the `rustls::ClientConfig` construction refuses
//! what a closed type refuses; nothing there connects, so nothing there shows a handshake failing.
//! These cells do. The negatives are the ones that matter: a server whose chain is signed by an
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
    use sutura_exec_postgres::PostgresError;
    use sutura_exec_postgres::PostgresWarehouse;
    use sutura_exec_postgres::fixture::FixtureCredential;
    use sutura_exec_postgres::tls::{TlsAnchors, TlsIdentity, client_config};

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

    /// A connection config for the loopback TLS dial.
    ///
    /// This deliberately leaves tokio-postgres's default `SslMode::Prefer` unset: `connect_secured`
    /// strengthens it to `Require` whenever a verifier is supplied. That the tier always answers the
    /// TLS negotiation `S` means no cell in THIS file can observe a downgrade if that strengthening
    /// were ever removed - the tier has no server to decline TLS with. The hermetic negative that
    /// proves it is `crates/sutura-exec-postgres/src/tests.rs`, against a listener that answers `N`.
    fn config(port: u16) -> tokio_postgres::Config {
        let credential = FixtureCredential::from_env().unwrap_or_else(|unconfigured| panic!("{unconfigured}"));
        PostgresWarehouse::local_config(LOOPBACK, port, &credential)
    }

    /// The same dial as [`config`], selecting the role whose HBA line requires a certificate.
    fn mutual_config(port: u16) -> tokio_postgres::Config {
        let mut config = config(port);
        config.user(MUTUAL_ROLE);
        config
    }

    #[test]
    fn a_source_chain_from_the_declared_anchor_is_verified_and_answers() {
        let Some(tier) = tier() else {
            return;
        };
        let tls = client_config(&TlsAnchors::Bundle(tier.anchor), None).expect("the tier's certificate is a usable anchor");
        // A successful open IS the proof: the handshake verifies the chain against the declared
        // anchor, and `connect_secured` then runs `SET statement_timeout` over the session.
        PostgresWarehouse::connect_secured(corpus::source(), corpus::posture(), &config(tier.port), Some(tls))
            .expect("a source verifying the tier's own chain connects and runs a statement");
    }

    #[test]
    fn a_mutual_source_presents_the_identity_the_server_demands() {
        let Some(tier) = tier() else {
            return;
        };
        let identity = TlsIdentity::new(tier.client_certificate, tier.client_key);
        let tls = client_config(&TlsAnchors::Bundle(tier.anchor), Some(&identity))
            .expect("the tier's anchors and client identity build a mutual config");
        PostgresWarehouse::connect_secured(corpus::source(), corpus::posture(), &mutual_config(tier.port), Some(tls))
            .expect("the mutual-only role accepts the client identity the tier signed");
    }

    #[test]
    fn a_mutual_role_refuses_a_client_that_presents_no_certificate() {
        let Some(tier) = tier() else {
            return;
        };
        let tls = client_config(&TlsAnchors::Bundle(tier.anchor), None)
            .expect("the tier's anchor builds a verifier without a client identity");
        let refused =
            PostgresWarehouse::connect_secured(corpus::source(), corpus::posture(), &mutual_config(tier.port), Some(tls));
        let error = refused.expect_err("the mutual-only role must reject a client with no certificate");
        assert!(matches!(error, PostgresError::Connect { .. }), "{error}");
    }

    #[test]
    fn a_source_chain_from_an_untrusted_issuer_is_refused() {
        let Some(tier) = tier() else {
            return;
        };
        // An anchor that signs nothing the server presents. If verification were not happening, this
        // would connect exactly as the cell above does.
        let untrusted = rcgen::generate_simple_self_signed([String::from("not-the-tier")]).expect("a self-signed pair generates");
        let anchor = std::env::temp_dir().join(format!("sutura-pg-untrusted-{}.pem", std::process::id()));
        std::fs::write(&anchor, untrusted.cert.pem()).expect("the untrusted anchor writes");
        let tls = client_config(&TlsAnchors::Bundle(anchor.clone()), None)
            .expect("an unrelated certificate is still a parsable anchor");
        let refused = PostgresWarehouse::connect_secured(corpus::source(), corpus::posture(), &config(tier.port), Some(tls));
        let _ignored = std::fs::remove_file(&anchor);
        let error = refused.expect_err("a chain the declared anchors do not name is refused");
        assert!(matches!(error, PostgresError::Connect { .. }), "{error}");
    }
}
