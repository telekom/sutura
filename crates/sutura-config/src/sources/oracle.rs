//! The `oracle` entry's own keys: which of them this kind is opened with, and which are refused
//! for meaning nothing to it.
//!
//! **Split out of the parent for the reason `clickhouse` is** - the 1000-line cap is unexemptable
//! under `crates/`. The shared helpers - the foreign-key refusal, the required-key read, the
//! absolute-path read and the remote-plaintext rule - stay in the parent and are reached through
//! `super::`, so there is one copy of each and this file cannot narrow one.

use std::path::PathBuf;

use super::{InvalidSourceRegistry, RawSourceEntry, SourceKind};
use crate::sources::placement::{HostName, NoOracleChannel, OracleChannel, OracleServiceName, SourcePlacement};
use sutura_domain::model::SourceName;

/// Reads a `kind: oracle` entry into its placement.
///
/// **Every value the driver's connection needs is declared and nothing else is accepted**: the
/// listener's host and port, the service name it resolves, the user and the file its password is
/// read from. `transport_mode` is required and must be `verified`, with `transport_anchors` naming a
/// PEM bundle - the only anchors the server is verified against. `plaintext` is refused for every
/// host: to a loopback one as [`InvalidSourceRegistry::OraclePlaintext`] in a build with the
/// `oracle` feature (without it, as a placement with no channel, which the composition root refuses
/// as not linked), to any other by the parent's remote-plaintext rule.
///
/// The connection stays on that host: a listener's redirect is refused before authentication, held
/// by `sutura-cli`'s `a_listener_redirect_to_an_address_nobody_declared_is_refused`.
///
/// An `impersonation-at-source` entry also declares the `delegation` each caller's verified token is
/// exchanged through for the token that opens their session.
///
/// # Why `mutual` and `transport_anchors: system` are refused
///
/// The driver builds its own TLS configuration from the PEM certificates it is handed: it reads no
/// host store and presents a client certificate only from a wallet, which no key here declares.
/// Accepting either would be the *reads as done and is not* defect `docs/adr/0010`'s
/// declared-trust-store rule exists to name.
///
/// # Errors
///
/// A key that belongs to another kind; a missing `host`, `port`, `service_name`, `user`,
/// `password_file` or `transport_mode`; a `host` that is not a usable host; a relative
/// `password_file`; a transport declaration that is not usable, `mutual`, or `verified` against the
/// host store; `plaintext` on any host; and a `delegation` that is missing on an impersonating entry,
/// written on any other, or unusable.
pub(super) fn parse_placement(
    alias: &SourceName,
    kind: SourceKind,
    entry: &RawSourceEntry<'_>,
    written: impl Fn(Option<&str>) -> bool,
) -> Result<SourcePlacement, InvalidSourceRegistry> {
    super::refuse_foreign_keys(alias, kind, foreign_keys(entry, written))?;
    let host =
        HostName::parse(super::required(alias, kind, "host", entry.host)?).map_err(|cause| InvalidSourceRegistry::Host {
            alias: alias.clone(),
            cause,
        })?;
    let port = entry.port.ok_or_else(|| InvalidSourceRegistry::MissingForKind {
        alias: alias.clone(),
        kind,
        key: "port",
    })?;
    let service_name =
        OracleServiceName::parse(super::required(alias, kind, "service_name", entry.service_name)?).map_err(|cause| {
            InvalidSourceRegistry::OracleServiceName {
                alias: alias.clone(),
                cause,
            }
        })?;
    let user = super::required(alias, kind, "user", entry.user)?.to_owned();
    let password_file: PathBuf = super::parse_absolute(
        alias,
        "password_file",
        super::required(alias, kind, "password_file", entry.password_file)?,
    )?;
    // The same flat-key read every dialled kind makes, so a mode that discards a key it would not
    // read is refused identically here - and only then is the mode itself asked about.
    let transport = crate::sources::transport::parse(
        alias,
        super::required(alias, kind, "transport_mode", entry.transport_mode)?,
        entry.transport_anchors,
        entry.client_certificate,
        entry.client_key,
    )
    .map_err(|cause| InvalidSourceRegistry::Transport {
        alias: alias.clone(),
        cause,
    })?;
    super::refuse_remote_plaintext(alias, &host, &transport)?;
    // A build without the adapter keeps a loopback `plaintext` entry, so its "not linked" refusal is
    // the one the operator reads.
    let unlinked = cfg!(not(feature = "oracle")) && matches!(transport, super::transport::SourceTransport::Plaintext);
    let channel = if unlinked {
        None
    } else {
        Some(declared(alias, kind, &host, transport)?)
    };
    let delegation = super::parse_delegation(alias, kind, entry, None)?;
    Ok(SourcePlacement::Oracle {
        host,
        port,
        service_name,
        user,
        password_file,
        channel,
        delegation,
    })
}

/// The `verified` channel `transport` declares, or the typed refusal naming `alias`.
fn declared(
    alias: &SourceName,
    kind: SourceKind,
    host: &HostName,
    transport: super::transport::SourceTransport,
) -> Result<OracleChannel, InvalidSourceRegistry> {
    OracleChannel::declared(host, transport).map_err(|refused| match refused {
        NoOracleChannel::Undeliverable(mode) => InvalidSourceRegistry::TlsNotDeliverable {
            alias: alias.clone(),
            kind,
            mode,
        },
        NoOracleChannel::ServerName(cause) => InvalidSourceRegistry::TlsServerName {
            alias: alias.clone(),
            cause,
        },
        NoOracleChannel::Plaintext => InvalidSourceRegistry::OraclePlaintext { alias: alias.clone() },
    })
}

/// The six keys an `oracle` entry has no use for, paired with whether this entry wrote each.
///
/// The `files` key, the three `bigquery` keys, and the two dialled keys this kind does not read: the
/// driver dials a TCP listener, and names the database by `service_name` rather than `database`.
fn foreign_keys(entry: &RawSourceEntry<'_>, written: impl Fn(Option<&str>) -> bool) -> [(&'static str, bool); 6] {
    [
        ("data_dir", written(entry.data_dir)),
        ("billing_project", written(entry.billing_project)),
        ("dataset", written(entry.dataset)),
        ("max_bytes_billed", entry.max_bytes_billed.is_some()),
        ("unix_socket", written(entry.unix_socket)),
        ("database", written(entry.database)),
    ]
}

/// The typed refusals this file adds, one cell per rule - beside the parse, as `clickhouse`'s are.
#[cfg(test)]
mod tests {
    use sutura_domain::model::SourceName;

    use crate::security::DeploymentIdentity;
    use crate::sources::workload_identity::DelegationGrant;
    use crate::sources::{InvalidSourceRegistry, RawSourceEntry, SourceKind, SourceRegistry, placement};

    fn alias(name: &str) -> SourceName {
        SourceName::parse(name).expect("a test alias is a name")
    }

    fn single_user() -> DeploymentIdentity {
        DeploymentIdentity::parse("single-user", Some("one operator, their own files, their own credentials"))
            .expect("a declared single-user mode parses")
    }

    /// An `oracle` entry with every key that kind is opened with, `verified` against a declared
    /// bundle. A test changes one field at a time.
    fn oracle(written: &str) -> RawSourceEntry<'_> {
        RawSourceEntry {
            written,
            kind: "oracle",
            data_dir: None,
            billing_project: None,
            dataset: None,
            max_bytes_billed: None,
            posture: "shared-service-user",
            acknowledged_because: Some("one service user reaching the database for everybody who asks"),
            verification_identity: None,
            workload_identity: None,
            host: Some("127.0.0.1"),
            unix_socket: None,
            port: Some(1521),
            database: None,
            service_name: Some("FREEPDB1"),
            endpoint: None,
            token_file: None,
            database_file: None,
            user: Some("sutura"),
            password_file: Some("/etc/sutura/oracle-password"),
            transport_mode: Some("verified"),
            transport_anchors: Some("/etc/sutura/ca.pem"),
            client_certificate: None,
            client_key: None,
            impersonate: false,
            subjects: false,
            delegation: None,
        }
    }

    #[test]
    fn an_oracle_source_declares_its_listener_its_service_and_its_password_file() {
        let registry =
            SourceRegistry::parse(&[oracle("warehouse")], Some(&single_user())).expect("a complete oracle entry parses");
        let configured = registry.get(&alias("warehouse")).expect("the entry is there");
        assert_eq!(configured.kind(), SourceKind::Oracle);
        match configured.placement() {
            placement::SourcePlacement::Oracle {
                host,
                port,
                service_name,
                user,
                password_file,
                channel: Some(channel),
                delegation,
            } => {
                assert_eq!(channel.anchors(), std::path::Path::new("/etc/sutura/ca.pem"));
                assert!(delegation.is_empty(), "a shared source declares no delegation");
                assert_eq!(host.as_str(), "127.0.0.1");
                assert_eq!(*port, 1521);
                assert_eq!(service_name.as_str(), "FREEPDB1");
                assert_eq!(user, "sutura");
                assert_eq!(password_file, std::path::Path::new("/etc/sutura/oracle-password"));
            }
            other => panic!("expected an oracle placement, got {other:?}"),
        }
    }

    /// A remote host is declarable over `verified`, and the bundle it names is the channel's whole
    /// trust: the placement carries that path and nothing that could widen it.
    #[test]
    fn a_verified_oracle_source_carries_the_bundle_it_declares() {
        let verified = RawSourceEntry {
            host: Some("db.example.com"),
            transport_mode: Some("verified"),
            transport_anchors: Some("/etc/sutura/ca.pem"),
            ..oracle("warehouse")
        };
        let registry = SourceRegistry::parse(&[verified], Some(&single_user())).expect("a verified oracle entry parses");
        let configured = registry.get(&alias("warehouse")).expect("the entry is there");
        let placement::SourcePlacement::Oracle {
            channel: Some(channel), ..
        } = configured.placement()
        else {
            panic!("expected an oracle placement, got {:?}", configured.placement());
        };
        assert_eq!(channel.anchors(), std::path::Path::new("/etc/sutura/ca.pem"));
        assert_eq!(
            *channel.server_name(),
            placement::TlsServerName::parse(&placement::HostName::parse("db.example.com").expect("a host name parses"))
                .expect("a DNS name is a TLS server name")
        );
    }

    /// A `verified` host that TLS cannot name is refused at parse, naming the key, so the driver is
    /// never handed it.
    #[test]
    fn a_verified_oracle_host_that_tls_cannot_name_is_refused_naming_the_key() {
        let unnamed = RawSourceEntry {
            host: Some("db..example.com"),
            transport_mode: Some("verified"),
            transport_anchors: Some("/etc/sutura/ca.pem"),
            ..oracle("warehouse")
        };
        let error = SourceRegistry::parse(&[unnamed], Some(&single_user())).expect_err("the host is refused");
        assert!(matches!(error, InvalidSourceRegistry::TlsServerName { .. }), "{error}");
        assert!(error.to_string().contains("sources.warehouse.host"), "{error}");
    }

    /// **The refusal this kind adds.** The two TLS declarations its driver cannot honour are refused
    /// naming the kind and the declaration: a client certificate, and the host's own store.
    #[test]
    fn mutual_or_the_host_store_on_an_oracle_source_is_refused_naming_the_declaration() {
        let system = RawSourceEntry {
            host: Some("db.example.com"),
            transport_mode: Some("verified"),
            transport_anchors: Some("system"),
            ..oracle("warehouse")
        };
        let mutual = RawSourceEntry {
            transport_anchors: Some("/etc/sutura/ca.pem"),
            client_certificate: Some("/etc/sutura/client.pem"),
            client_key: Some("/etc/sutura/client.key"),
            transport_mode: Some("mutual"),
            ..system.clone()
        };
        for (mode, entry) in [("verified with `transport_anchors: system`", system), ("mutual", mutual)] {
            let error = SourceRegistry::parse(&[entry], Some(&single_user())).expect_err("the declaration is refused");
            assert!(
                matches!(
                    error,
                    InvalidSourceRegistry::TlsNotDeliverable {
                        kind: SourceKind::Oracle,
                        mode: named,
                        ..
                    } if named == mode
                ),
                "{error}"
            );
            assert!(error.to_string().contains("transport_anchors"), "{error}");
        }
    }

    #[test]
    fn a_remote_oracle_host_without_tls_is_a_startup_refusal_naming_the_key() {
        let remote = RawSourceEntry {
            host: Some("db.example.com"),
            transport_mode: Some("plaintext"),
            transport_anchors: None,
            ..oracle("warehouse")
        };
        let error = SourceRegistry::parse(&[remote], Some(&single_user())).expect_err("a remote plaintext source is refused");
        assert!(
            matches!(error, InvalidSourceRegistry::RemoteWithoutTls { ref host, .. } if host == "db.example.com"),
            "{error}"
        );
        assert!(error.to_string().contains("transport_mode"), "{error}");
    }

    /// **No `oracle` source is read over `plaintext`**, a loopback one included: the refusal
    /// names the source and no placement is built.
    #[test]
    #[cfg(feature = "oracle")]
    fn a_loopback_oracle_source_declaring_plaintext_is_a_startup_refusal() {
        let loopback = RawSourceEntry {
            transport_mode: Some("plaintext"),
            transport_anchors: None,
            ..oracle("warehouse")
        };
        let error = SourceRegistry::parse(&[loopback], Some(&single_user())).expect_err("a plaintext source is refused");
        assert!(
            matches!(error, InvalidSourceRegistry::OraclePlaintext { ref alias } if alias.as_str() == "warehouse"),
            "{error}"
        );
        assert!(error.to_string().contains("sources.warehouse.transport_mode"), "{error}");
    }

    #[test]
    fn the_dialled_keys_oracle_reads_past_are_refused_rather_than_ignored() {
        // Each on its own: `refuse_foreign_keys` names the FIRST written key.
        for (key, entry) in [
            (
                "database",
                RawSourceEntry {
                    database: Some("FREEPDB1"),
                    ..oracle("warehouse")
                },
            ),
            (
                "unix_socket",
                RawSourceEntry {
                    unix_socket: Some("/run/oracle"),
                    ..oracle("warehouse")
                },
            ),
            (
                "data_dir",
                RawSourceEntry {
                    data_dir: Some("/srv/sutura/data"),
                    ..oracle("warehouse")
                },
            ),
        ] {
            let error = SourceRegistry::parse(&[entry], Some(&single_user())).expect_err("a key this kind reads past is refused");
            let InvalidSourceRegistry::KeyNotForKind { kind, key: named, .. } = error else {
                panic!("expected a wrong-kind refusal for {key}, got {error}");
            };
            assert_eq!(kind, SourceKind::Oracle);
            assert_eq!(named, key);
        }
    }

    /// No guessed service and no guessed port: both name a deployment's own listener.
    #[test]
    fn an_oracle_source_missing_its_service_name_or_port_does_not_parse() {
        for (key, entry) in [
            (
                "service_name",
                RawSourceEntry {
                    service_name: None,
                    endpoint: None,
                    token_file: None,
                    database_file: None,
                    ..oracle("warehouse")
                },
            ),
            (
                "port",
                RawSourceEntry {
                    port: None,
                    ..oracle("warehouse")
                },
            ),
        ] {
            let error = SourceRegistry::parse(&[entry], Some(&single_user())).expect_err("a missing key is refused");
            assert!(
                matches!(
                    error,
                    InvalidSourceRegistry::MissingForKind {
                        kind: SourceKind::Oracle,
                        key: missing,
                        ..
                    } if missing == key
                ),
                "{error}"
            );
        }
    }

    /// `service_name` is `oracle`'s alone: written on any other kind it is refused, not read. One
    /// entry per kind, because each kind reaches the refusal through its own list - `postgres`'
    /// inline one, `clickhouse`'s own, and `dialled_source_keys` for `files` and `bigquery`.
    #[test]
    fn a_service_name_on_any_other_kind_is_refused() {
        // `files` and `bigquery` refuse every dialled key, so their entry carries `service_name` alone.
        let undialled = RawSourceEntry {
            host: None,
            port: None,
            user: None,
            password_file: None,
            transport_mode: None,
            ..oracle("warehouse")
        };
        for (expected, entry) in [
            (
                SourceKind::Postgres,
                RawSourceEntry {
                    kind: "postgres",
                    database: Some("sutura"),
                    ..oracle("warehouse")
                },
            ),
            (
                SourceKind::ClickHouse,
                RawSourceEntry {
                    kind: "clickhouse",
                    ..oracle("warehouse")
                },
            ),
            (
                SourceKind::Files,
                RawSourceEntry {
                    kind: "files",
                    ..undialled.clone()
                },
            ),
            (
                SourceKind::BigQuery,
                RawSourceEntry {
                    kind: "bigquery",
                    ..undialled.clone()
                },
            ),
        ] {
            let error = SourceRegistry::parse(&[entry], Some(&single_user())).expect_err("service_name is refused here");
            assert!(
                matches!(
                    error,
                    InvalidSourceRegistry::KeyNotForKind {
                        kind,
                        key: "service_name",
                        ..
                    } if kind == expected
                ),
                "{error}"
            );
        }
    }

    /// A service name is one the driver reads as written, or it is refused: `:pooled` would switch
    /// the server type and `/x` name an instance, silently, if the value reached the connect string.
    #[test]
    fn a_service_name_the_driver_would_read_as_something_more_is_refused() {
        for (written, found) in [
            ("FREEPDB1:pooled", ':'),
            ("FREEPDB1/x", '/'),
            ("FREE-PDB1", '-'),
            // `FRÉE`: alphanumeric, and not ASCII - the driver would dial service `FR` and drop the rest.
            ("FR\u{c9}E", '\u{c9}'),
        ] {
            let entry = RawSourceEntry {
                service_name: Some(written),
                endpoint: None,
                token_file: None,
                database_file: None,
                ..oracle("warehouse")
            };
            let error = SourceRegistry::parse(&[entry], Some(&single_user())).expect_err("the service name is refused");
            assert!(
                matches!(
                    error,
                    InvalidSourceRegistry::OracleServiceName {
                        cause: placement::InvalidOracleServiceName::Character { found: named },
                        ..
                    } if named == found
                ),
                "{written}: {error}"
            );
        }
        SourceRegistry::parse(
            &[RawSourceEntry {
                service_name: Some("orcl.example_1"),
                endpoint: None,
                token_file: None,
                database_file: None,
                ..oracle("warehouse")
            }],
            Some(&single_user()),
        )
        .expect("letters, digits, `_` and `.` are a service name");
    }

    fn hop(audience: &str, grant: DelegationGrant) -> crate::raw::RawDelegation {
        crate::raw::RawDelegation {
            token_endpoint: String::from("https://idp.example.com/token"),
            client_id: String::from("sutura"),
            client_secret_file: String::from("/etc/sutura/client-secret"),
            audience: String::from(audience),
            grant,
        }
    }

    /// An impersonating entry over `verified`, declaring `delegation`. A test changes one field at a time.
    fn impersonating(delegation: &[crate::raw::RawDelegation]) -> RawSourceEntry<'_> {
        RawSourceEntry {
            host: Some("db.example.com"),
            transport_mode: Some("verified"),
            transport_anchors: Some("/etc/sutura/ca.pem"),
            posture: "impersonation-at-source",
            acknowledged_because: None,
            delegation: Some(delegation),
            ..oracle("warehouse")
        }
    }

    /// An impersonating source declares the hops each caller's token is exchanged through, and the
    /// placement carries them in the order written, each with its own grant.
    #[test]
    fn an_impersonating_oracle_source_declares_its_delegation_hops_in_order() {
        let declared = [
            hop("sutura-app", DelegationGrant::TokenExchange),
            hop("https://db.example.com", DelegationGrant::OnBehalfOf),
        ];
        let registry = SourceRegistry::parse(&[impersonating(&declared)], Some(&single_user()))
            .expect("an impersonating oracle entry parses");
        let configured = registry.get(&alias("warehouse")).expect("the entry is there");
        let placement::SourcePlacement::Oracle { delegation, .. } = configured.placement() else {
            panic!("expected an oracle placement, got {:?}", configured.placement());
        };
        let hops: Vec<_> = delegation.iter().map(|hop| (hop.audience(), hop.grant())).collect();
        assert_eq!(
            hops,
            vec![
                ("sutura-app", DelegationGrant::TokenExchange),
                ("https://db.example.com", DelegationGrant::OnBehalfOf)
            ]
        );
    }

    /// A refusal over one hop names its position in `delegation`, so with two hops the second is the
    /// one the message points at.
    #[test]
    fn an_oracle_delegation_refusal_names_the_position_of_the_hop() {
        let relative = [
            hop("sutura-app", DelegationGrant::TokenExchange),
            crate::raw::RawDelegation {
                client_secret_file: String::from("run/secrets/idp"),
                ..hop("https://db.example.com", DelegationGrant::OnBehalfOf)
            },
        ];
        let refused = SourceRegistry::parse(&[impersonating(&relative)], Some(&single_user()))
            .expect_err("a relative client secret file is refused");
        assert!(
            refused
                .to_string()
                .starts_with("`sources.warehouse.delegation[1].client_secret_file` is"),
            "{refused}"
        );
    }

    /// A chain ending in `broker-token` is refused at load; the same hop before another parses.
    #[test]
    fn an_oracle_delegation_ending_in_a_broker_token_hop_is_refused() {
        let ending = [
            hop("sutura-app", DelegationGrant::TokenExchange),
            hop("11111111-1111-4111-8111-111111111111", DelegationGrant::BrokerToken),
        ];
        let error = SourceRegistry::parse(&[impersonating(&ending)], Some(&single_user()))
            .expect_err("a chain ending in broker-token is refused");
        assert!(
            matches!(
                error,
                InvalidSourceRegistry::DelegationEndsInBrokerToken { key: "delegation", .. }
            ),
            "{error}"
        );
        let followed = [
            hop("11111111-1111-4111-8111-111111111111", DelegationGrant::BrokerToken),
            hop("https://db.example.com", DelegationGrant::OnBehalfOf),
        ];
        drop(
            SourceRegistry::parse(&[impersonating(&followed)], Some(&single_user()))
                .expect("a broker-token hop before another parses"),
        );
    }

    /// **Each way `delegation` is refused**: missing or empty on an impersonating entry, written on a
    /// shared one, and beside a `workload_identity` block.
    #[test]
    fn an_oracle_delegation_is_refused_unless_an_impersonating_verified_source_declares_it() {
        use placement::InvalidDelegation;

        let one = [hop("https://db.example.com", DelegationGrant::OnBehalfOf)];
        let cases = [
            (
                RawSourceEntry {
                    delegation: None,
                    ..impersonating(&one)
                },
                InvalidDelegation::Missing,
            ),
            (impersonating(&[]), InvalidDelegation::Missing),
            (
                RawSourceEntry {
                    delegation: Some(&one),
                    ..oracle("warehouse")
                },
                InvalidDelegation::NotImpersonating,
            ),
        ];
        for (entry, expected) in cases {
            let error = SourceRegistry::parse(&[entry], Some(&single_user())).expect_err("the delegation is refused");
            assert!(
                matches!(error, InvalidSourceRegistry::Delegation { ref cause, .. } if *cause == expected),
                "{expected:?}: {error}"
            );
        }
        let with_a_pool = RawSourceEntry {
            workload_identity: Some(crate::raw::RawWorkloadIdentity {
                audience: String::from("//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/p/providers/q"),
                expected_issuer: None,
                expected_audience: None,
                delegation: None,
            }),
            ..impersonating(&one)
        };
        let error = SourceRegistry::parse(&[with_a_pool], Some(&single_user())).expect_err("a pool is not an oracle key");
        assert!(
            matches!(
                error,
                InvalidSourceRegistry::KeyNotForKind {
                    key: "workload_identity",
                    ..
                }
            ),
            "{error}"
        );
    }

    /// `delegation` is `oracle`'s alone among these kinds, and the removed `subjects` is refused by
    /// name on every kind rather than read.
    #[test]
    fn delegation_on_another_kind_and_subjects_on_any_kind_are_refused() {
        let one = [hop("https://db.example.com", DelegationGrant::TokenExchange)];
        let postgres = |entry: RawSourceEntry<'static>| RawSourceEntry {
            kind: "postgres",
            database: Some("sutura"),
            service_name: None,
            ..entry
        };
        let error = SourceRegistry::parse(
            &[postgres(RawSourceEntry {
                delegation: Some(&[]),
                ..oracle("warehouse")
            })],
            Some(&single_user()),
        )
        .expect_err("a delegation is refused here");
        assert!(
            matches!(error, InvalidSourceRegistry::KeyNotForKind { key: "delegation", .. }),
            "{error}"
        );
        for entry in [
            postgres(RawSourceEntry {
                subjects: true,
                ..oracle("warehouse")
            }),
            RawSourceEntry {
                subjects: true,
                ..impersonating(&one)
            },
        ] {
            let error = SourceRegistry::parse(&[entry], Some(&single_user())).expect_err("`subjects` is refused");
            assert!(matches!(error, InvalidSourceRegistry::SubjectsRemoved { .. }), "{error}");
        }
    }
}
