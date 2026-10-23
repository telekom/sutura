//! `build`'s choice between the two ways an impersonating source runs its caller, against a fake
//! listener that completes a password login.

use sutura_domain::model::SourceName;

/// The settings of a `direct` deployment over one impersonating `oracle` source at `port`, with
/// `extra` under its posture, over TLS that `anchors` verifies.
fn declared(port: u16, password: &std::path::Path, anchors: &std::path::Path, extra: &str) -> sutura_config::Settings {
    let overlay = format!(
        "security:\n  identity: \"single-user\"\n  single_user_because: \"a test\"\n  inbound:\n    mode: \"direct\"\n    \
         resource: \"https://sutura.example.com\"\n    authorization_server: \"https://idp.example.com\"\n    \
         key_set_file: \"/nonexistent/keys.json\"\n    algorithms: [\"RS256\"]\nsources:\n  warehouse:\n    \
         kind: \"oracle\"\n    host: \"127.0.0.1\"\n    port: {port}\n    service_name: \"FREEPDB1\"\n    user: \"sutura\"\n    \
         password_file: \"{}\"\n    transport_mode: \"verified\"\n    transport_anchors: \"{}\"\n    \
         posture: \"impersonation-at-source\"\n{extra}    delegation:\n      - token_endpoint: \
         \"https://idp.example.com/token\"\n        client_id: \"sutura\"\n        \
         client_secret_file: \"/nonexistent/sutura-test-idp-secret\"\n        audience: \"database-client-id\"\n",
        password.display(),
        anchors.display()
    );
    sutura_config::Settings::load(
        &sutura_config::Sources::defaults(sutura_config::Environment::Development).with_overlay(overlay),
    )
    .expect("the test settings load")
}

/// **`caller_session: end-user-context` opens the source through the attaching path**, which asks
/// the database at startup whether it offers end-user security contexts: one that offers none
/// refuses the source, while the same source without the key opens with a token login.
#[test]
fn an_end_user_context_source_is_refused_at_startup_by_a_database_that_offers_none() {
    let directory = std::env::temp_dir().join(format!("sutura-cli-oracle-end-user-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("a scratch directory is creatable");
    let password = directory.join("password");
    std::fs::write(&password, "not-a-real-password").expect("the password file writes");
    let anchors = directory.join("ca.pem");
    let issued = sutura_http_client::tls_test_support::issue();
    std::fs::write(&anchors, issued.certificate().pem()).expect("the anchors file writes");
    let config = sutura_http_client::tls_test_support::server_config(&issued);
    let open = move |mut stream: std::net::TcpStream| {
        let mut server = rustls::ServerConnection::new(std::sync::Arc::clone(&config)).ok()?;
        while server.is_handshaking() {
            let _io = server.complete_io(&mut stream).ok()?;
        }
        Some(rustls::StreamOwned::new(server, stream))
    };
    let (port, _served) = sutura_dev::tns_listener::logging_in(open, false).expect("the fake listener binds");
    let source = SourceName::parse("warehouse").expect("a test source is a source");
    let working_set =
        sutura_exec_datafusion::WorkingSet::of_bytes(core::num::NonZeroUsize::new(1 << 20).expect("a mebibyte is positive"));
    let attaching_settings = declared(port, &password, &anchors, "    caller_session: \"end-user-context\"\n");
    let own_login_settings = declared(port, &password, &anchors, "");
    let attaching = super::build(
        &source,
        attaching_settings.sources().get(&source).expect("the source is declared"),
        working_set,
    )
    .map(drop);
    let own_login = super::build(
        &source,
        own_login_settings.sources().get(&source).expect("the source is declared"),
        working_set,
    )
    .map(drop);
    let _ignored = std::fs::remove_dir_all(&directory);
    let error = attaching.expect_err("a database that offers no end-user contexts is refused");
    assert!(error.contains("does not offer end-user security contexts"), "{error}");
    assert_eq!(own_login, Ok(()), "the same source without the key opens");
}
