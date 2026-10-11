//! The `clickhouse` entry's own keys: which of them this kind is opened with, and which are
//! refused for meaning nothing to it.
//!
//! **Split out of the parent for the reason the parent's module declaration states** - the 1000-line
//! cap is unexemptable under `crates/`, and a fourth kind's key reading does not fit beside the
//! other three. What lives here is exactly this kind's half of
//! `super::parse_placement`; the shared helpers - the foreign-key refusal,
//! the required-key read, the absolute-path read and the remote-plaintext rule - stay in the parent
//! and are reached through `super::`, so there is one copy of each and this file cannot narrow one.

use std::path::PathBuf;

use super::{InvalidSourceRegistry, RawSourceEntry, SourceKind};
use crate::sources::placement::{HostName, InvalidDelegation, SourcePlacement};
use crate::sources::workload_identity::DelegationDeclared;
use sutura_domain::model::SourceName;

/// Reads a `kind: clickhouse` entry into its placement.
///
/// **Every value the transport needs is declared and nothing else is accepted.** The host and port
/// are the HTTP interface's address, the user and password file are the HTTP Basic credential, and
/// the transport is the channel - the same five declarations a `postgres` entry makes minus the
/// two that mean nothing here.
///
/// # The two keys refused for being absent from the adapter rather than from the file
///
/// `unix_socket` and `database` are in the refused set beside the `files` and `bigquery` keys.
/// Neither is a gap waiting to be filled: `ClickHouse`'s HTTP interface is dialled over TCP, and
/// `sutura_exec_clickhouse::transport::Http` sends no `database` field on the one request it makes,
/// so a `database:` written here would be a key an operator believes is in effect while the
/// deployment reads past it. That is the state this module's own foreign-key rule exists to refuse,
/// and refusing it on this kind too is the rule applied rather than an exception to it.
///
/// # Errors
///
/// A key that belongs to another kind; a missing `host`, `port`, `user`, `password_file` or
/// `transport_mode`; a `host` that is not a usable host; a relative `password_file`; a transport
/// declaration this build cannot use; and a non-loopback host declared `plaintext`.
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
    let user = super::required(alias, kind, "user", entry.user)?.to_owned();
    let password_file: PathBuf = super::parse_absolute(
        alias,
        "password_file",
        super::required(alias, kind, "password_file", entry.password_file)?,
    )?;
    // The channel is a declared decision, read from its FLAT keys - the same call the `postgres`
    // arm makes, so a mode that discards a key it would not read is refused identically on both.
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
    // Issue 124's fail-closed rule, through the parent's one copy of it: a password over HTTP Basic
    // and every row would otherwise cross the network in clear text.
    super::refuse_remote_plaintext(alias, &host, &transport)?;
    Ok(SourcePlacement::ClickHouse {
        host,
        port,
        password_file,
        transport,
        user,
        delegation: parse_delegation(alias, entry)?,
    })
}

/// The hops each caller's verified token is exchanged through, for the token that caller signs in
/// with. Absent on an impersonating entry, each caller is switched to with `EXECUTE AS` instead;
/// refused on any other entry, where no caller signs in.
fn parse_delegation(alias: &SourceName, entry: &RawSourceEntry<'_>) -> Result<Vec<DelegationDeclared>, InvalidSourceRegistry> {
    let invalid = |cause| InvalidSourceRegistry::Delegation {
        alias: alias.clone(),
        cause,
    };
    match (entry.posture.trim() == "impersonation-at-source", entry.delegation) {
        (_, None) => Ok(Vec::new()),
        (false, Some(_)) => Err(invalid(InvalidDelegation::NotImpersonating)),
        (true, Some([])) => Err(invalid(InvalidDelegation::Missing)),
        (true, Some(declared)) => super::workload_identity::source_hops(alias, declared),
    }
}

/// The eight keys a `clickhouse` entry has no use for, paired with whether this entry wrote each.
///
/// The `files` key, the four `bigquery` keys, and the three dialled keys this kind does not read -
/// see this module's own header for why `unix_socket` and `database` are an absence in the adapter
/// rather than a pending feature; `service_name` is `oracle`'s.
fn foreign_keys(entry: &RawSourceEntry<'_>, written: impl Fn(Option<&str>) -> bool) -> [(&'static str, bool); 8] {
    [
        ("data_dir", written(entry.data_dir)),
        ("billing_project", written(entry.billing_project)),
        ("dataset", written(entry.dataset)),
        ("max_bytes_billed", entry.max_bytes_billed.is_some()),
        ("workload_identity", entry.workload_identity.is_some()),
        ("unix_socket", written(entry.unix_socket)),
        ("database", written(entry.database)),
        ("service_name", written(entry.service_name)),
    ]
}

/// The typed refusals this file adds, one cell per rule. Beside the parse rather than in
/// `super::tests`, because a characterization cell belongs in the file whose behaviour it pins.
#[cfg(test)]
mod tests {
    use sutura_domain::model::SourceName;

    use crate::security::DeploymentIdentity;
    use crate::sources::{InvalidSourceRegistry, RawSourceEntry, SourceKind, SourceRegistry, placement, transport};

    fn alias(name: &str) -> SourceName {
        SourceName::parse(name).expect("a test alias is a name")
    }

    fn single_user() -> DeploymentIdentity {
        DeploymentIdentity::parse("single-user", Some("one operator, their own files, their own credentials"))
            .expect("a declared single-user mode parses")
    }

    /// A `clickhouse` entry with every key that kind is opened with, on a loopback literal so the
    /// plaintext declaration is one this kind may make. A test changes one field at a time.
    fn clickhouse(written: &str) -> RawSourceEntry<'_> {
        RawSourceEntry {
            written,
            kind: "clickhouse",
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
            port: Some(8123),
            database: None,
            service_name: None,
            endpoint: None,
            token_file: None,
            database_file: None,
            user: Some("sutura"),
            password_file: Some("/etc/sutura/ch-password"),
            transport_mode: Some("plaintext"),
            transport_anchors: None,
            client_certificate: None,
            client_key: None,
            impersonate: false,
            subjects: false,
            delegation: None,
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
                delegation,
            } => {
                assert!(delegation.is_empty(), "a shared source declares no delegation");
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
                    data_dir: Some("/srv/sutura/data"),
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

    fn impersonating() -> RawSourceEntry<'static> {
        RawSourceEntry {
            posture: "impersonation-at-source",
            acknowledged_because: None,
            ..clickhouse("warehouse")
        }
    }

    /// Each caller runs as the `ClickHouse` user named by their verified subject, so an impersonating
    /// entry declares no map at all - and the removed map is refused on every kind, naming the key.
    #[test]
    fn an_impersonating_clickhouse_source_needs_no_map_and_the_removed_one_is_refused() {
        let registry = SourceRegistry::parse(&[impersonating()], None).expect("an impersonating entry with no map parses");
        let configured = registry.get(&alias("warehouse")).expect("the entry is there");
        assert_eq!(configured.kind(), SourceKind::ClickHouse);
        for entry in [
            RawSourceEntry {
                impersonate: true,
                ..impersonating()
            },
            RawSourceEntry {
                impersonate: true,
                ..clickhouse("warehouse")
            },
            RawSourceEntry {
                kind: "files",
                data_dir: Some("/srv/sutura/data"),
                host: None,
                port: None,
                user: None,
                password_file: None,
                transport_mode: None,
                impersonate: true,
                ..clickhouse("warehouse")
            },
        ] {
            let error = SourceRegistry::parse(&[entry], Some(&single_user())).expect_err("the removed map is refused");
            assert!(matches!(error, InvalidSourceRegistry::ImpersonateRemoved { .. }), "{error}");
            assert!(
                error.to_string().contains("`sources.warehouse.impersonate` is removed"),
                "{error}"
            );
        }
    }

    /// Each caller signs in with their own exchanged token when the entry declares `delegation`: over
    /// plaintext only to a loopback host, and only on an impersonating entry.
    #[test]
    fn a_delegated_clickhouse_source_signs_callers_in_over_tls_off_loopback_only() {
        let hops = [crate::raw::RawDelegation {
            token_endpoint: String::from("https://idp.example.com/token"),
            client_id: String::from("sutura"),
            client_secret_file: String::from("/etc/sutura/client-secret"),
            audience: String::from("clickhouse"),
            grant: crate::sources::workload_identity::DelegationGrant::TokenExchange,
        }];
        let delegated = RawSourceEntry {
            delegation: Some(&hops),
            ..impersonating()
        };
        let registry = SourceRegistry::parse(std::slice::from_ref(&delegated), None).expect("a loopback delegated entry parses");
        let configured = registry.get(&alias("warehouse")).expect("the entry is there");
        assert!(
            matches!(*configured.placement(), placement::SourcePlacement::ClickHouse { ref delegation, .. } if delegation.len() == 1),
            "{:?}",
            configured.placement()
        );

        let remote = RawSourceEntry {
            host: Some("ch.example.com"),
            ..delegated.clone()
        };
        let error = SourceRegistry::parse(&[remote], None).expect_err("a remote plaintext delegated source is refused");
        assert!(matches!(error, InvalidSourceRegistry::RemoteWithoutTls { .. }), "{error}");

        let shared = RawSourceEntry {
            delegation: Some(&hops),
            ..clickhouse("warehouse")
        };
        let error = SourceRegistry::parse(&[shared], Some(&single_user())).expect_err("a shared source exchanges nothing");
        assert!(
            matches!(
                error,
                InvalidSourceRegistry::Delegation {
                    cause: placement::InvalidDelegation::NotImpersonating,
                    ..
                }
            ),
            "{error}"
        );
    }

    /// Only a `bigquery` source reads `workload_identity`, refused on this kind rather than read past.
    #[test]
    fn workload_identity_is_refused_on_a_clickhouse_source() {
        let with_wif = RawSourceEntry {
            workload_identity: Some(crate::raw::RawWorkloadIdentity {
                audience: String::from("//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/p/providers/sso"),
                expected_issuer: None,
                expected_audience: None,
                delegation: None,
            }),
            ..impersonating()
        };
        let error = SourceRegistry::parse(&[with_wif], None).expect_err("workload_identity is not a clickhouse key");
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
}
