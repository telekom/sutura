use super::*;

// ----------------------------------------------------------------- per-kind: postgres ----

/// A `postgres` entry with everything that kind needs, over a unix socket, so a test changes one
/// field at a time. The socket path means this deploys like the nix tier - no network, no TLS.
fn postgres(written: &str) -> RawSourceEntry<'_> {
    RawSourceEntry {
        written,
        kind: "postgres",
        data_dir: None,
        billing_project: None,
        dataset: None,
        credential_file: None,
        max_bytes_billed: None,
        posture: "shared-service-user",
        acknowledged_because: Some("one service role reaching the database for everybody who asks"),
        verification_identity: None,
        workload_identity: None,
        host: None,
        unix_socket: Some("/tmp/sutura-pg"),
        port: Some(5432),
        database: Some("sutura"),
        user: Some("sutura"),
        password_file: Some("/etc/sutura/pg-password"),
        transport_mode: Some("plaintext"),
        transport_anchors: None,
        client_certificate: None,
        client_key: None,
    }
}

#[test]
fn a_postgres_source_declares_its_connection_and_its_plaintext_choice() {
    let entries = [postgres("warehouse")];
    let registry = SourceRegistry::parse(&entries, Some(&single_user())).expect("a complete postgres entry parses");
    let configured = registry.get(&alias("warehouse")).expect("the entry is there");
    assert_eq!(configured.kind(), super::SourceKind::Postgres);
    match configured.placement() {
        super::placement::SourcePlacement::Postgres {
            dial,
            database,
            user,
            password_file,
            transport,
        } => {
            assert_eq!(
                *dial,
                super::placement::PostgresDial::UnixSocket {
                    directory: std::path::PathBuf::from("/tmp/sutura-pg"),
                    port: 5432,
                }
            );
            assert_eq!(database, "sutura");
            assert_eq!(user, "sutura");
            assert_eq!(password_file, std::path::Path::new("/etc/sutura/pg-password"));
            assert_eq!(*transport, super::transport::SourceTransport::Plaintext);
        }
        other => panic!("expected a postgres placement, got {other:?}"),
    }
}

#[test]
fn a_declared_host_that_names_a_url_scheme_is_refused_at_the_registry() {
    // The registry-level half of `HostName`'s own refusal, with the newtype's reason attached.
    let entries = [RawSourceEntry {
        host: Some("postgres://db"),
        unix_socket: None,
        ..postgres("warehouse")
    }];
    let error = SourceRegistry::parse(&entries, Some(&single_user())).expect_err("a URL is not a usable host");
    match error {
        InvalidSourceRegistry::Host { cause, .. } => assert!(matches!(cause, InvalidHostName::Scheme { .. }), "{cause:?}"),
        other => panic!("expected a host refusal, got {other:?}"),
    }
}

#[test]
fn a_postgres_source_missing_a_required_key_does_not_parse() {
    // Fail-closed, and it names the key. A database, a role, a password file and a transport are
    // each facts an adapter needs that nothing here can guess.
    for (key, entry) in [
        (
            "database",
            RawSourceEntry {
                database: None,
                ..postgres("warehouse")
            },
        ),
        (
            "user",
            RawSourceEntry {
                user: None,
                ..postgres("warehouse")
            },
        ),
        (
            "password_file",
            RawSourceEntry {
                password_file: None,
                ..postgres("warehouse")
            },
        ),
        (
            "port",
            RawSourceEntry {
                port: None,
                ..postgres("warehouse")
            },
        ),
    ] {
        let error =
            SourceRegistry::parse(&[entry], Some(&single_user())).expect_err("a postgres source has to say where it connects");
        let InvalidSourceRegistry::MissingForKind { key: refused, .. } = error else {
            panic!("expected a missing-key refusal, got {error:?}");
        };
        assert_eq!(refused, key, "{error}");
    }
}

#[test]
fn a_postgres_source_must_declare_exactly_one_of_host_or_unix_socket() {
    // Both directions are refused. Neither says nothing about where an adapter comes up to; both
    // says a TCP dial and a socket dial cannot both be what the connection is.
    let neither = RawSourceEntry {
        host: None,
        unix_socket: None,
        ..postgres("warehouse")
    };
    let error = SourceRegistry::parse(&[neither], Some(&single_user())).expect_err("a source has to say where it is");
    assert!(
        matches!(error, InvalidSourceRegistry::MissingForKind { key, .. } if key == "host_or_unix_socket"),
        "{error}"
    );

    let both = RawSourceEntry {
        host: Some("127.0.0.1"),
        unix_socket: Some("/tmp/pg"),
        ..postgres("warehouse")
    };
    let error = SourceRegistry::parse(&[both], Some(&single_user())).expect_err("a source is one dial or the other");
    assert!(
        matches!(error, InvalidSourceRegistry::KeyNotForKind { key, .. } if key == "host_and_unix_socket"),
        "{error}"
    );
}

#[test]
fn a_relative_password_file_or_unix_socket_is_refused_naming_the_key() {
    // The same fact `a_relative_credential_file_is_refused_and_the_refusal_names_the_key` proves
    // for `bigquery`, over the two `postgres` paths that were untested: a working directory is
    // whatever this process's supervisor chose, and a password file or a socket resolved against it
    // is a different file on every host.
    let entries = [RawSourceEntry {
        password_file: Some("pg-password"),
        ..postgres("warehouse")
    }];
    let error = SourceRegistry::parse(&entries, Some(&single_user())).expect_err("a relative password file is refused");
    match error {
        InvalidSourceRegistry::RelativePath { key, ref path, .. } => {
            assert_eq!(key, "password_file");
            assert_eq!(path, std::path::Path::new("pg-password"));
        }
        other => panic!("expected a relative-path refusal, got {other:?}"),
    }

    let entries = [RawSourceEntry {
        unix_socket: Some("relative/pg"),
        ..postgres("warehouse")
    }];
    let error = SourceRegistry::parse(&entries, Some(&single_user())).expect_err("a relative unix socket is refused");
    match error {
        InvalidSourceRegistry::RelativePath { key, ref path, .. } => {
            assert_eq!(key, "unix_socket");
            assert_eq!(path, std::path::Path::new("relative/pg"));
        }
        other => panic!("expected a relative-path refusal, got {other:?}"),
    }
}

#[test]
fn a_postgres_source_refuses_the_keys_of_other_kinds() {
    // A postgres source that also declares a data_dir or a billing_project is a configuration
    // nobody can see - an operator believed a key was in effect and nothing would read it.
    let files_key = RawSourceEntry {
        data_dir: Some("/srv/data"),
        ..postgres("warehouse")
    };
    let error = SourceRegistry::parse(&[files_key], Some(&single_user())).expect_err("a directory on a dataset is refused");
    assert!(matches!(error, InvalidSourceRegistry::KeyNotForKind { key: "data_dir", .. }));
}

#[test]
fn a_postgres_source_asking_for_tls_with_no_anchors_is_refused_naming_the_source() {
    // Rule 2 of docs/adr/0010: the trust store is stated, not inherited. `TrustAnchors` has no
    // default, so a `verified` or `mutual` transport with no `transport_anchors` cannot load.
    let no_anchors = RawSourceEntry {
        transport_mode: Some("verified"),
        transport_anchors: None,
        ..postgres("warehouse")
    };
    let error = SourceRegistry::parse(&[no_anchors], Some(&single_user())).expect_err("TLS with no anchors is refused");
    assert!(matches!(error, InvalidSourceRegistry::Transport { .. }), "{error}");
}

#[test]
fn a_remote_host_without_tls_is_a_startup_refusal_naming_the_key() {
    // Issue 124's fail-closed rule, as a test: a source a network can reach must not be declared
    // `plaintext`, because a password and a whole result set would cross it in clear text. The
    // refusal names `transport_mode`, which is where an operator writes the remedy.
    let remote = RawSourceEntry {
        host: Some("db.example.com"),
        unix_socket: None,
        ..postgres("warehouse")
    };
    let error = SourceRegistry::parse(&[remote], Some(&single_user())).expect_err("a remote plaintext source is refused");
    assert!(
        matches!(error, InvalidSourceRegistry::RemoteWithoutTls { ref host, .. } if host == "db.example.com"),
        "{error}"
    );
    assert!(error.to_string().contains("transport_mode"), "{error}");

    // A loopback LITERAL is the same declaration and is allowed - the rule is about reachability,
    // not about whether TLS happens to be spelled out.
    let local = RawSourceEntry {
        host: Some("127.0.0.1"),
        unix_socket: None,
        ..postgres("warehouse")
    };
    SourceRegistry::parse(&[local], Some(&single_user())).expect("a loopback plaintext source parses");

    // And the same remote host WITH a declared TLS mode and anchors is what a deployment writes.
    let secured = RawSourceEntry {
        host: Some("db.example.com"),
        unix_socket: None,
        transport_mode: Some("verified"),
        transport_anchors: Some("/etc/sutura/ca.pem"),
        ..postgres("warehouse")
    };
    SourceRegistry::parse(&[secured], Some(&single_user())).expect("a remote verified source parses");
}

#[test]
fn a_tls_transport_over_a_unix_socket_is_refused_naming_both_keys() {
    // The other direction issue 124/125 hold: TLS over a unix socket has no handshake to perform, so
    // a `verified`/`mutual` declaration on that dial is refused at PARSE, before it can reach
    // `PostgresWarehouse::connect_secured` and fail at connect time with an error naming neither key.
    let verified_socket = RawSourceEntry {
        transport_mode: Some("verified"),
        transport_anchors: Some("system"),
        ..postgres("warehouse")
    };
    let error =
        SourceRegistry::parse(&[verified_socket], Some(&single_user())).expect_err("verified TLS over a socket is refused");
    assert!(
        matches!(error, InvalidSourceRegistry::TlsOverUnixSocket { mode: "verified", .. }),
        "{error}"
    );
    assert!(error.to_string().contains("unix_socket"), "{error}");
    assert!(error.to_string().contains("transport_mode"), "{error}");

    let mutual_socket = RawSourceEntry {
        transport_mode: Some("mutual"),
        transport_anchors: Some("system"),
        client_certificate: Some("/tls/c.pem"),
        client_key: Some("/tls/k.pem"),
        ..postgres("warehouse")
    };
    let error = SourceRegistry::parse(&[mutual_socket], Some(&single_user())).expect_err("mutual TLS over a socket is refused");
    assert!(
        matches!(error, InvalidSourceRegistry::TlsOverUnixSocket { mode: "mutual", .. }),
        "{error}"
    );

    // And the unchanged case this must not touch: `plaintext` over a unix socket is the declared,
    // fixture-tier shape and still parses.
    SourceRegistry::parse(&[postgres("warehouse")], Some(&single_user())).expect("plaintext over a socket still parses");
}

#[test]
fn a_partial_client_certificate_is_refused_at_load() {
    // A certificate with no key, or a key with no certificate, is the same class as
    // `server.tls_certificate` without `server.tls_key`. Silent refusal would start mTLS disabled.
    let cert_only = RawSourceEntry {
        transport_mode: Some("mutual"),
        transport_anchors: Some("system"),
        client_certificate: Some("/tls/c.pem"),
        client_key: None,
        ..postgres("warehouse")
    };
    let error = SourceRegistry::parse(&[cert_only], Some(&single_user())).expect_err("a certificate alone is refused");
    assert!(matches!(error, InvalidSourceRegistry::Transport { .. }), "{error}");

    let key_only = RawSourceEntry {
        transport_mode: Some("mutual"),
        transport_anchors: Some("system"),
        client_certificate: None,
        client_key: Some("/tls/k.pem"),
        ..postgres("warehouse")
    };
    let error = SourceRegistry::parse(&[key_only], Some(&single_user())).expect_err("a key alone is refused");
    assert!(matches!(error, InvalidSourceRegistry::Transport { .. }), "{error}");
}

#[test]
fn a_mutual_transport_declares_its_anchors_and_identity() {
    // A TCP host, not the fixture default's unix socket: `TlsOverUnixSocket` now refuses a
    // `verified`/`mutual` transport declared over a socket dial, so this cell's own claim - that a
    // COMPLETE mutual declaration parses - needs a dial TLS can actually run over.
    let entry = RawSourceEntry {
        host: Some("db.example.com"),
        unix_socket: None,
        transport_mode: Some("mutual"),
        transport_anchors: Some("system"),
        client_certificate: Some("/tls/c.pem"),
        client_key: Some("/tls/k.pem"),
        ..postgres("warehouse")
    };
    let registry = SourceRegistry::parse(&[entry], Some(&single_user())).expect("a complete mutual entry parses");
    let configured = registry.get(&alias("warehouse")).expect("the entry is there");
    match configured.placement() {
        super::placement::SourcePlacement::Postgres { transport, .. } => match transport {
            super::transport::SourceTransport::Mutual { anchors, identity } => {
                assert_eq!(*anchors, super::transport::TrustAnchors::System);
                assert_eq!(identity.certificate(), std::path::Path::new("/tls/c.pem"));
                assert_eq!(identity.key(), std::path::Path::new("/tls/k.pem"));
            }
            other => panic!("expected mutual, got {other:?}"),
        },
        other => panic!("expected a postgres placement, got {other:?}"),
    }
}

#[test]
fn postgres_is_a_kind_the_vocabulary_names() {
    assert!(super::SourceKind::NAMES.contains(&"postgres"));
    assert_eq!(
        super::SourceKind::parse("postgres").expect("postgres is a kind"),
        super::SourceKind::Postgres
    );
    let rendered = super::SourceKind::parse("snowflake")
        .expect_err("snowflake is not a kind")
        .to_string();
    assert!(rendered.contains("postgres"), "{rendered}");
}
