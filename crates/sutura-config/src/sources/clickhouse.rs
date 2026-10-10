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
use crate::sources::placement::{DeclaredUsers, HostName, InvalidImpersonate, SourcePlacement};
use sutura_domain::identity::SubjectKey;
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
        impersonate: parse_impersonate(alias, kind, entry, &user)?,
        user,
    })
}

/// The subject -> `ClickHouse` user map: required and non-empty on an `impersonation-at-source`
/// entry and refused on any other, and never beside `workload_identity`, which only `bigquery` reads.
/// No declared user may be `service`, the source's own `user`.
fn parse_impersonate(
    alias: &SourceName,
    kind: SourceKind,
    entry: &RawSourceEntry<'_>,
    service: &str,
) -> Result<DeclaredUsers, InvalidSourceRegistry> {
    super::refuse_foreign_keys(alias, kind, [("workload_identity", entry.workload_identity.is_some())])?;
    let invalid = |cause| InvalidSourceRegistry::Impersonate {
        alias: alias.clone(),
        cause,
    };
    let declared = match (entry.posture.trim() == "impersonation-at-source", entry.impersonate) {
        (false, None) => return Ok(DeclaredUsers::new()),
        (false, Some(_)) => return Err(invalid(InvalidImpersonate::NotImpersonating)),
        (true, None) => return Err(invalid(InvalidImpersonate::Missing)),
        (true, Some(declared)) if declared.is_empty() => return Err(invalid(InvalidImpersonate::Missing)),
        (true, Some(declared)) => declared,
    };
    let mut parsed = DeclaredUsers::new();
    for (subject, user) in declared {
        let subject = SubjectKey::parse(subject).map_err(|cause| invalid(InvalidImpersonate::Subject { cause }))?;
        let user = user.trim();
        if user.is_empty() {
            return Err(invalid(InvalidImpersonate::EmptyUser));
        }
        if user == service {
            return Err(invalid(InvalidImpersonate::ServiceUser));
        }
        if parsed.insert(subject, String::from(user)).is_some() {
            return Err(invalid(InvalidImpersonate::DuplicateSubject));
        }
    }
    Ok(parsed)
}

/// The seven keys a `clickhouse` entry has no use for, paired with whether this entry wrote each.
///
/// The `files` key, the three `bigquery` keys, and the three dialled keys this kind does not read -
/// see this module's own header for why `unix_socket` and `database` are an absence in the adapter
/// rather than a pending feature; `service_name` is `oracle`'s.
fn foreign_keys(entry: &RawSourceEntry<'_>, written: impl Fn(Option<&str>) -> bool) -> [(&'static str, bool); 7] {
    [
        ("data_dir", written(entry.data_dir)),
        ("billing_project", written(entry.billing_project)),
        ("dataset", written(entry.dataset)),
        ("max_bytes_billed", entry.max_bytes_billed.is_some()),
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
    use crate::sources::placement::InvalidImpersonate;
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
            database_file: None,
            user: Some("sutura"),
            password_file: Some("/etc/sutura/ch-password"),
            transport_mode: Some("plaintext"),
            transport_anchors: None,
            client_certificate: None,
            client_key: None,
            impersonate: None,
            subjects: None,
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
                impersonate,
            } => {
                assert!(impersonate.is_empty(), "a shared source declares no subject map");
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

    fn impersonating(map: &std::collections::BTreeMap<String, String>) -> RawSourceEntry<'_> {
        RawSourceEntry {
            posture: "impersonation-at-source",
            acknowledged_because: None,
            impersonate: Some(map),
            ..clickhouse("warehouse")
        }
    }

    #[test]
    fn an_impersonating_clickhouse_source_carries_its_declared_subject_map() {
        let map = std::collections::BTreeMap::from([(String::from("subject-a"), String::from(" analyst_a "))]);
        let registry = SourceRegistry::parse(&[impersonating(&map)], None).expect("an impersonating entry with a map parses");
        let configured = registry.get(&alias("warehouse")).expect("the entry is there");
        let placement::SourcePlacement::ClickHouse { impersonate, .. } = configured.placement() else {
            panic!("expected a clickhouse placement");
        };
        let subject = sutura_domain::identity::SubjectKey::parse("subject-a").expect("a test subject parses");
        assert_eq!(impersonate.get(&subject).map(String::as_str), Some("analyst_a"));
    }

    #[test]
    fn a_clickhouse_subject_map_is_required_exactly_when_the_source_impersonates() {
        let empty = std::collections::BTreeMap::new();
        let map = std::collections::BTreeMap::from([(String::from("subject-a"), String::from("analyst_a"))]);
        let blank = std::collections::BTreeMap::from([(String::from("subject-a"), String::from(" "))]);
        let missing = RawSourceEntry {
            impersonate: None,
            ..impersonating(&empty)
        };
        let on_shared = RawSourceEntry {
            impersonate: Some(&map),
            ..clickhouse("warehouse")
        };
        for (entry, expected) in [
            (missing, InvalidImpersonate::Missing),
            (impersonating(&empty), InvalidImpersonate::Missing),
            (impersonating(&blank), InvalidImpersonate::EmptyUser),
            (on_shared, InvalidImpersonate::NotImpersonating),
        ] {
            let error = SourceRegistry::parse(&[entry], Some(&single_user())).expect_err("a misdeclared map is refused");
            assert!(
                matches!(error, InvalidSourceRegistry::Impersonate { ref cause, .. } if *cause == expected),
                "{expected:?}: {error}"
            );
        }
    }

    /// A subject mapped to the source's own `user`, padded or not, is refused at load.
    #[test]
    fn an_impersonate_entry_naming_the_service_user_is_refused() {
        for written in ["sutura", " sutura "] {
            let map = std::collections::BTreeMap::from([
                (String::from("subject-a"), String::from("analyst_a")),
                (String::from("subject-b"), String::from(written)),
            ]);
            let error = SourceRegistry::parse(&[impersonating(&map)], Some(&single_user()))
                .expect_err("a declared user equal to the source's own user is refused");
            assert!(
                matches!(error, InvalidSourceRegistry::Impersonate { ref cause, .. }
                    if cause.to_string().contains("the source's own `user`")),
                "{written:?}: {error:?}"
            );
        }
    }

    /// Only a `bigquery` source reads `workload_identity`, and only a `clickhouse` source reads
    /// `impersonate` - each refused on the other kind rather than read past.
    #[test]
    fn the_two_subject_maps_are_refused_on_the_kind_that_does_not_read_them() {
        let map = std::collections::BTreeMap::from([(String::from("subject-a"), String::from("analyst_a"))]);
        let with_wif = RawSourceEntry {
            workload_identity: Some(crate::raw::RawWorkloadIdentity {
                audience: String::from("//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/p/providers/sso"),
                impersonate: std::collections::BTreeMap::new(),
                expected_issuer: None,
                expected_audience: None,
                delegation: None,
            }),
            ..impersonating(&map)
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
        let on_files = RawSourceEntry {
            kind: "files",
            data_dir: Some("/srv/sutura/data"),
            host: None,
            port: None,
            user: None,
            password_file: None,
            transport_mode: None,
            impersonate: Some(&map),
            ..clickhouse("warehouse")
        };
        let error = SourceRegistry::parse(&[on_files], Some(&single_user())).expect_err("impersonate is a clickhouse key");
        assert!(
            matches!(error, InvalidSourceRegistry::KeyNotForKind { key: "impersonate", .. }),
            "{error}"
        );
    }
}
