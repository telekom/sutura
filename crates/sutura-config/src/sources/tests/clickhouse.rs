use super::*;

// --------------------------------------------------------------- per-kind: clickhouse ----

/// A `clickhouse` entry with every key that kind is opened with, on a loopback literal so the
/// plaintext declaration is one this kind may make. A test changes one field at a time.
fn clickhouse(written: &str) -> RawSourceEntry<'_> {
    RawSourceEntry {
        written,
        kind: "clickhouse",
        data_dir: None,
        billing_project: None,
        dataset: None,
        credential_file: None,
        max_bytes_billed: None,
        posture: "shared-service-user",
        acknowledged_because: Some("one service user reaching the database for everybody who asks"),
        verification_identity: None,
        workload_identity: None,
        host: Some("127.0.0.1"),
        unix_socket: None,
        port: Some(8123),
        database: None,
        user: Some("sutura"),
        password_file: Some("/etc/sutura/ch-password"),
        transport_mode: Some("plaintext"),
        transport_anchors: None,
        client_certificate: None,
        client_key: None,
    }
}

#[test]
fn a_clickhouse_source_declares_its_endpoint_and_its_plaintext_choice() {
    let registry =
        SourceRegistry::parse(&[clickhouse("warehouse")], Some(&single_user())).expect("a complete clickhouse entry parses");
    let configured = registry.get(&alias("warehouse")).expect("the entry is there");
    assert_eq!(configured.kind(), SourceKind::ClickHouse);
    match configured.placement() {
        placement::SourcePlacement::ClickHouse {
            host,
            port,
            user,
            password_file,
            transport,
        } => {
            assert_eq!(host.as_str(), "127.0.0.1");
            assert_eq!(*port, 8123);
            assert_eq!(user, "sutura");
            assert_eq!(password_file, std::path::Path::new("/etc/sutura/ch-password"));
            assert_eq!(*transport, transport::SourceTransport::Plaintext);
        }
        other => panic!("expected a clickhouse placement, got {other:?}"),
    }
}

#[test]
fn a_remote_clickhouse_host_without_tls_is_a_startup_refusal_naming_the_key() {
    // Issue 124's rule on the kind that sends its password as HTTP Basic: over plaintext to a host a
    // network can reach, the credential and every row cross it in clear text.
    let remote = RawSourceEntry {
        host: Some("ch.example.com"),
        ..clickhouse("warehouse")
    };
    let error = SourceRegistry::parse(&[remote], Some(&single_user())).expect_err("a remote plaintext source is refused");
    assert!(
        matches!(error, InvalidSourceRegistry::RemoteWithoutTls { ref host, .. } if host == "ch.example.com"),
        "{error}"
    );
    assert!(error.to_string().contains("transport_mode"), "{error}");

    let secured = RawSourceEntry {
        host: Some("ch.example.com"),
        transport_mode: Some("verified"),
        transport_anchors: Some("/etc/sutura/ca.pem"),
        ..clickhouse("warehouse")
    };
    SourceRegistry::parse(&[secured], Some(&single_user())).expect("a remote verified source parses");
}

#[test]
fn the_two_postgres_keys_clickhouse_reads_past_are_refused_rather_than_ignored() {
    // Each on its own: `refuse_foreign_keys` names the FIRST written key, so two at once would prove
    // only one of them is observed.
    for (key, entry) in [
        (
            "database",
            RawSourceEntry {
                database: Some("sutura"),
                ..clickhouse("warehouse")
            },
        ),
        (
            "unix_socket",
            RawSourceEntry {
                unix_socket: Some("/run/clickhouse"),
                ..clickhouse("warehouse")
            },
        ),
        (
            "data_dir",
            RawSourceEntry {
                data_dir: Some(DATA),
                ..clickhouse("warehouse")
            },
        ),
    ] {
        let error = SourceRegistry::parse(&[entry], Some(&single_user())).expect_err("a key this kind reads past is refused");
        let InvalidSourceRegistry::KeyNotForKind { kind, key: named, .. } = error else {
            panic!("expected a wrong-kind refusal for {key}, got {error}");
        };
        assert_eq!(kind, SourceKind::ClickHouse);
        assert_eq!(named, key);
    }
}

#[test]
fn a_clickhouse_source_missing_its_port_does_not_parse() {
    // No guessed `8123`: a deployment behind a proxy publishes neither of the conventional ports.
    let entry = RawSourceEntry {
        port: None,
        ..clickhouse("warehouse")
    };
    let error = SourceRegistry::parse(&[entry], Some(&single_user())).expect_err("a missing port is refused");
    assert!(
        matches!(
            error,
            InvalidSourceRegistry::MissingForKind {
                kind: SourceKind::ClickHouse,
                key: "port",
                ..
            }
        ),
        "{error}"
    );
}
