//! The Oracle half of this composition root: one declared source becomes an open `Warehouse`.
//!
//! **`clickhouse`'s shape, for its reasons**: the dispatcher stays in the composition root, both
//! `open_oracle` definitions live here because the compiler picks between them at every call site,
//! and the per-source BUILD lives once in `crate::oracle`, shared with `crate::sources`' own root.

/// Opens one Oracle adapter per declared source, under the declared credential.
///
/// **Nothing is attached and nothing is registered** - the tables live in the database. After the
/// source is looked up, everything that can fail before a listener is bound happens in
/// [`crate::oracle::build`]: the posture cross-check, the password file, and the connection itself.
#[cfg(feature = "oracle")]
pub(crate) fn open_oracle(
    declared: &[&sutura_domain::model::SourceName],
    registry: &sutura_config::SourceRegistry,
    runtime: sutura_config::RuntimeSettings,
) -> Result<super::OpenedSources, String> {
    let mut engines: Option<sutura_app::Warehouses<super::OracleSource>> = None;
    for source in declared {
        let configured = super::configured_source(source, registry)?;
        let working_set = sutura_exec_datafusion::WorkingSet::of_bytes(runtime.working_set().bytes());
        let engine = crate::oracle::build(source, configured, working_set)?;
        engines = Some(match engines {
            None => sutura_app::Warehouses::of(engine),
            Some(open) => open.and(engine).map_err(super::flatten)?,
        });
    }
    // Unreachable: `declared` is non-empty and every iteration assigns. Written as a fallback for
    // the reason `open_files` gives - the workspace denies `unwrap` and `expect`.
    engines
        .map(super::OpenedSources::Oracle)
        .ok_or_else(|| String::from("this catalog declares no models, so there is nothing to open"))
}

/// The refusal for a build that did not link the Oracle adapter - `open_clickhouse`'s twin shape.
#[cfg(not(feature = "oracle"))]
pub(crate) fn open_oracle(
    declared: &[&sutura_domain::model::SourceName],
    _registry: &sutura_config::SourceRegistry,
    _runtime: sutura_config::RuntimeSettings,
) -> Result<super::OpenedSources, String> {
    let named = declared
        .iter()
        .map(|source| source.as_str())
        .collect::<Vec<&str>>()
        .join(", ");
    Err(format!(
        "[{named}] declares `kind: oracle`, and this binary was built without the `oracle` feature - \
         so it links no Oracle adapter. Build `sutura-cli` with `--features oracle`, or declare a \
         `files` source"
    ))
}

/// `sutura serve`'s own `oracle` cells: the refusal for a build that did not link the adapter, the
/// posture refusal, the furthest a fixture with no server reaches - the declared password file,
/// alone and beside a `files` source - and the listener that redirects, refused before
/// authentication.
#[cfg(test)]
mod tests {
    #[cfg(feature = "oracle")]
    use crate::serve::ENGINE_SOURCE;
    use crate::serve::open_engine;
    #[cfg(feature = "oracle")]
    use crate::serve::tests::entry;
    use crate::serve::tests::{bundle_over, default_timeout, one_worker, refusal, registry};

    /// One `oracle` entry whose password file is not there, so a refusal naming that key is proof
    /// the composition reached the credential step - and dialled nothing.
    fn oracle_entry(alias: &str, posture: &str, extra: &str) -> String {
        format!(
            "  {alias}:\n    kind: \"oracle\"\n    host: \"127.0.0.1\"\n    port: 1521\n    service_name: \"FREEPDB1\"\n    \
             user: \"sutura\"\n    password_file: \"/nonexistent/sutura-test-oracle-password\"\n    \
             transport_mode: \"plaintext\"\n    posture: \"{posture}\"\n{extra}"
        )
    }

    /// **Refused at startup and naming the source and the feature, not skipped**: a default build
    /// links no Oracle adapter. Compiled by `cargo xtask check-default-features` and RUN by
    /// `cargo xtask check-default-feature-tests`; `--all-features` makes this cfg false.
    #[test]
    #[cfg(not(feature = "oracle"))]
    fn an_oracle_source_is_refused_by_a_build_that_did_not_link_the_adapter() {
        let error = refusal(
            open_engine(
                &bundle_over(&[("customers", "warehouse", "dim_customer")]),
                &registry(&oracle_entry("warehouse", "shared-service-user", "")),
                one_worker(),
                default_timeout(),
                None,
            ),
            "a kind this binary linked no adapter for must not start",
        );
        assert!(error.contains("warehouse"), "the refusal must name the source: {error}");
        assert!(
            error.contains("--features oracle"),
            "the refusal must say what to build: {error}"
        );
    }

    #[test]
    #[cfg(feature = "oracle")]
    fn an_oracle_source_reaches_the_password_file_the_deployment_declared() {
        let error = refusal(
            open_engine(
                &bundle_over(&[("customers", "warehouse", "dim_customer")]),
                &registry(&oracle_entry("warehouse", "shared-service-user", "")),
                one_worker(),
                default_timeout(),
                None,
            ),
            "the declared password file is not there, so this deployment does not start",
        );
        assert!(error.contains("warehouse.password_file"), "{error}");
    }

    /// **An impersonating source passes the posture cross-check.** Declared with its `delegation`
    /// over `verified`, it stops at its own password file - before anything is dialled.
    #[test]
    #[cfg(feature = "oracle")]
    fn an_impersonating_oracle_source_reaches_the_password_file_the_deployment_declared() {
        let entry = "  warehouse:\n    kind: \"oracle\"\n    host: \"db.example.com\"\n    port: 2484\n    \
                     service_name: \"FREEPDB1\"\n    user: \"sutura\"\n    \
                     password_file: \"/nonexistent/sutura-test-oracle-password\"\n    \
                     transport_mode: \"verified\"\n    transport_anchors: \"/nonexistent/sutura-test-oracle-ca.pem\"\n    \
                     posture: \"impersonation-at-source\"\n    delegation:\n      - token_endpoint: \
                     \"https://idp.example.com/token\"\n        client_id: \"sutura\"\n        \
                     client_secret_file: \"/nonexistent/sutura-test-oracle-client-secret\"\n        \
                     audience: \"https://db.example.com\"\n";
        let error = refusal(
            open_engine(
                &bundle_over(&[("customers", "warehouse", "dim_customer")]),
                &registry(entry),
                one_worker(),
                default_timeout(),
                None,
            ),
            "the declared password file is not there, so this deployment does not start",
        );
        assert!(error.contains("warehouse.password_file"), "{error}");
    }

    /// The mixed registry's `oracle` group: the `files` half opens, then the Oracle half is refused
    /// by its own credential read rather than by the kind or the mix.
    #[test]
    #[cfg(feature = "oracle")]
    fn a_catalog_spanning_files_and_oracle_reaches_the_oracle_credential() {
        let both = format!(
            "{}{}",
            entry(ENGINE_SOURCE, "shared-service-user", ""),
            oracle_entry("warehouse", "shared-service-user", "")
        );
        let error = refusal(
            open_engine(
                &bundle_over(&[
                    ("customers", ENGINE_SOURCE, "dim_customer"),
                    ("products", "warehouse", "dim_product"),
                ]),
                &registry(&both),
                one_worker(),
                default_timeout(),
                None,
            ),
            "a mixed catalog opens each kind and reaches the oracle arm's own credential refusal",
        );
        assert!(error.contains("warehouse.password_file"), "{error}");
    }

    /// **Refused before authentication, and the redirect's address is never dialled.** The fake
    /// listener on the declared address answers the driver's CONNECT with a REDIRECT to a second
    /// loopback listener no source declares. It redirects to another loopback PORT rather than off
    /// the machine, so it runs in a sandbox with no other address; the refusal does not depend on
    /// where the redirect points.
    #[test]
    #[cfg(feature = "oracle")]
    fn a_listener_redirect_to_an_address_nobody_declared_is_refused() {
        let listener = sutura_dev::tns_listener::RedirectingListener::start().expect("the fake listener binds");
        let directory = std::env::temp_dir().join(format!("sutura-cli-oracle-redirect-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a scratch directory is creatable");
        let password = directory.join("password");
        std::fs::write(&password, "not-a-real-password").expect("the password file writes");
        let entry = format!(
            "  warehouse:\n    kind: \"oracle\"\n    host: \"127.0.0.1\"\n    port: {}\n    \
             service_name: \"FREEPDB1\"\n    user: \"sutura\"\n    password_file: \"{}\"\n    \
             transport_mode: \"plaintext\"\n    posture: \"shared-service-user\"\n",
            listener.port(),
            password.display()
        );
        let opened = open_engine(
            &bundle_over(&[("customers", "warehouse", "dim_customer")]),
            &registry(&entry),
            one_worker(),
            default_timeout(),
            None,
        );
        let _ignored = std::fs::remove_dir_all(&directory);
        let error = refusal(opened, "a listener that redirects is refused");
        assert!(error.contains("redirected the connection"), "{error}");
        assert!(
            !listener.target_was_dialled(std::time::Duration::from_secs(2)),
            "the address the redirect named was dialled"
        );
    }

    /// **A malformed authentication response refuses the source, and the process keeps running.**
    /// The fake accepts the CONNECT and answers the first authentication message with session
    /// data that has none of the verifier fields. The open runs inside `catch_unwind`, so an open
    /// that does not return fails this cell's first assertion rather than the test harness.
    #[test]
    #[cfg(feature = "oracle")]
    fn a_malformed_authentication_response_refuses_the_oracle_source() {
        let port = sutura_dev::tns_listener::authenticating(&[("AUTH_SESSKEY", "00")]).expect("the fake listener binds");
        let directory = std::env::temp_dir().join(format!("sutura-cli-oracle-auth-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a scratch directory is creatable");
        let password = directory.join("password");
        std::fs::write(&password, "not-a-real-password").expect("the password file writes");
        let entry = format!(
            "  warehouse:\n    kind: \"oracle\"\n    host: \"127.0.0.1\"\n    port: {port}\n    \
             service_name: \"FREEPDB1\"\n    user: \"sutura\"\n    password_file: \"{}\"\n    \
             transport_mode: \"plaintext\"\n    posture: \"shared-service-user\"\n",
            password.display()
        );
        let opened = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            open_engine(
                &bundle_over(&[("customers", "warehouse", "dim_customer")]),
                &registry(&entry),
                one_worker(),
                default_timeout(),
                None,
            )
        }));
        let _ignored = std::fs::remove_dir_all(&directory);
        let opened = opened.expect("the open returns for a malformed authentication response");
        let error = refusal(opened, "a malformed authentication response is refused");
        assert!(error.contains("AUTH_PBKDF2_VGEN_COUNT"), "{error}");
    }

    /// **A `verified` source opens TLS before the listener reads a TNS packet.** The fake answers
    /// a CONNECT with packet type 3, which the driver refuses by name. A `plaintext` source reaches
    /// that refusal; a `verified` one does not, because its first bytes are a TLS handshake that the
    /// fake cannot answer.
    #[test]
    #[cfg(feature = "oracle")]
    fn a_verified_oracle_source_opens_tls_before_any_tns_packet() {
        let directory = std::env::temp_dir().join(format!("sutura-cli-oracle-verified-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a scratch directory is creatable");
        let password = directory.join("password");
        std::fs::write(&password, "not-a-real-password").expect("the password file writes");
        let anchors = directory.join("ca.pem");
        let params = rcgen::CertificateParams::new([String::from("127.0.0.1")]).expect("an IP name parameterizes");
        let key = rcgen::KeyPair::generate().expect("a key pair generates");
        let certificate = params.self_signed(&key).expect("a self-signed certificate signs");
        std::fs::write(&anchors, certificate.pem()).expect("the anchors file writes");
        let refused_over = |transport: &str| {
            let port = sutura_dev::tns_listener::answering(3, Vec::new()).expect("the fake listener binds");
            let entry = format!(
                "  warehouse:\n    kind: \"oracle\"\n    host: \"127.0.0.1\"\n    port: {port}\n    \
                 service_name: \"FREEPDB1\"\n    user: \"sutura\"\n    password_file: \"{}\"\n    \
                 {transport}\n    posture: \"shared-service-user\"\n",
                password.display()
            );
            let opened = open_engine(
                &bundle_over(&[("customers", "warehouse", "dim_customer")]),
                &registry(&entry),
                one_worker(),
                default_timeout(),
                None,
            );
            refusal(opened, "the fake answers no driver")
        };
        let plaintext = refused_over("transport_mode: \"plaintext\"");
        let verified = refused_over(&format!(
            "transport_mode: \"verified\"\n    transport_anchors: \"{}\"",
            anchors.display()
        ));
        let _ignored = std::fs::remove_dir_all(&directory);
        assert!(plaintext.contains("unknown packet type 3"), "{plaintext}");
        assert!(!verified.contains("unknown packet type 3"), "{verified}");
    }
}
