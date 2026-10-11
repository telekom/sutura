//! The `cube` entry's own keys: which of them this kind is opened with, and which are refused for
//! meaning nothing to it.
//!
//! What lives here is this kind's half of `super::entry::parse_placement`; the shared helpers - the
//! foreign-key refusal, the required-key read and the absolute-path read - stay in `entry` and are
//! reached through `super::`, so there is one copy of each and this file cannot narrow one.

use std::path::PathBuf;

use super::entry::dialled_source_keys;
use super::{InvalidSourceRegistry, RawSourceEntry, SourceKind};
use crate::sources::placement::{InvalidDelegation, SourcePlacement};
use sutura_domain::model::SourceName;

/// Reads a `kind: cube` entry into its placement.
///
/// **The API's `endpoint` and the `token_file` the deployment's token is read from**, and on an
/// `impersonation-at-source` entry the `delegation` each caller's token is exchanged through for
/// the request's bearer. Every dialled key, every `bigquery` and `files` key and a workload-identity
/// block are refused, because the REST client reads none of them and a key written here would sit in
/// the file doing nothing.
///
/// # Errors
///
/// A key that belongs to another kind; a missing `endpoint` or `token_file`; an `endpoint` carrying
/// credentials; a relative `token_file`; and a `delegation` that is missing on an impersonating
/// entry, declared on any other, or sent to an endpoint that is not `https`.
pub(super) fn parse_placement(
    alias: &SourceName,
    kind: SourceKind,
    entry: &RawSourceEntry<'_>,
    written: impl Fn(Option<&str>) -> bool,
) -> Result<SourcePlacement, InvalidSourceRegistry> {
    super::refuse_foreign_keys(alias, kind, foreign_keys(entry, &written))?;
    super::refuse_foreign_keys(alias, kind, dialled_source_keys(entry, written))?;
    let endpoint = super::required(alias, kind, "endpoint", entry.endpoint)?;
    if endpoint.contains('@') {
        return Err(InvalidSourceRegistry::CubeEndpointCredentials { alias: alias.clone() });
    }
    let token_file: PathBuf = super::parse_absolute(
        alias,
        "token_file",
        super::required(alias, kind, "token_file", entry.token_file)?,
    )?;
    let delegation = super::parse_delegation(
        alias,
        kind,
        entry,
        (!endpoint.starts_with("https://")).then_some(InvalidDelegation::PlaintextEndpoint),
    )?;
    Ok(SourcePlacement::Cube {
        endpoint: endpoint.to_owned(),
        token_file,
        delegation,
    })
}

/// `cube`'s two keys, refused on every other kind for the reason `duckdb`'s one is.
pub(super) fn refuse_elsewhere(
    alias: &SourceName,
    kind: SourceKind,
    entry: &RawSourceEntry<'_>,
    written: impl Fn(Option<&str>) -> bool,
) -> Result<(), InvalidSourceRegistry> {
    if kind == SourceKind::Cube {
        return Ok(());
    }
    super::refuse_foreign_keys(
        alias,
        kind,
        [
            ("endpoint", written(entry.endpoint)),
            ("token_file", written(entry.token_file)),
        ],
    )
}

/// The four keys a `cube` entry has no use for beside the dialled ones: the `files` key and the
/// three `bigquery` keys. A workload-identity block is `super::parse_delegation`'s refusal.
fn foreign_keys(entry: &RawSourceEntry<'_>, written: &impl Fn(Option<&str>) -> bool) -> [(&'static str, bool); 4] {
    [
        ("data_dir", written(entry.data_dir)),
        ("billing_project", written(entry.billing_project)),
        ("dataset", written(entry.dataset)),
        ("max_bytes_billed", entry.max_bytes_billed.is_some()),
    ]
}

#[cfg(test)]
mod tests {
    use sutura_domain::model::SourceName;

    use crate::security::DeploymentIdentity;
    use crate::sources::placement::InvalidDelegation;
    use crate::sources::workload_identity::{DelegationDeclared, DelegationGrant};
    use crate::sources::{InvalidSourceRegistry, RawSourceEntry, SourceKind, SourceRegistry, placement};

    fn alias(name: &str) -> SourceName {
        SourceName::parse(name).expect("a test alias is a name")
    }

    fn single_user() -> DeploymentIdentity {
        DeploymentIdentity::parse("single-user", Some("one operator, their own files, their own credentials"))
            .expect("a declared single-user mode parses")
    }

    /// A `cube` entry with the two keys that kind is opened with. A test changes one field at a time.
    fn cube(written: &str) -> RawSourceEntry<'_> {
        RawSourceEntry {
            written,
            kind: "cube",
            data_dir: None,
            billing_project: None,
            dataset: None,
            max_bytes_billed: None,
            posture: "shared-service-user",
            acknowledged_because: Some("one deployment token reaching the metrics layer for everybody who asks"),
            verification_identity: None,
            workload_identity: None,
            host: None,
            unix_socket: None,
            port: None,
            database: None,
            service_name: None,
            endpoint: Some("https://cube.example.com"),
            token_file: Some("/etc/sutura/cube-token"),
            database_file: None,
            user: None,
            password_file: None,
            transport_mode: None,
            transport_anchors: None,
            client_certificate: None,
            client_key: None,
            impersonate: None,
            delegation: None,
            subjects: false,
        }
    }

    fn refusal(entry: RawSourceEntry<'_>) -> InvalidSourceRegistry {
        SourceRegistry::parse(&[entry], Some(&single_user())).expect_err("the entry is refused")
    }

    #[test]
    fn a_cube_source_declares_its_endpoint_and_its_token_file() {
        let registry = SourceRegistry::parse(&[cube("metrics")], Some(&single_user())).expect("a complete cube entry parses");
        let configured = registry.get(&alias("metrics")).expect("the entry is there");
        assert_eq!(configured.kind(), SourceKind::Cube);
        match configured.placement() {
            placement::SourcePlacement::Cube {
                endpoint, token_file, ..
            } => {
                assert_eq!(endpoint, "https://cube.example.com");
                assert_eq!(token_file, std::path::Path::new("/etc/sutura/cube-token"));
            }
            other => panic!("expected a cube placement, got {other:?}"),
        }
    }

    fn hop(audience: &str) -> crate::raw::RawDelegation {
        crate::raw::RawDelegation {
            token_endpoint: String::from("https://idp.example.com/token"),
            client_id: String::from("sutura"),
            client_secret_file: String::from("/etc/sutura/client-secret"),
            audience: String::from(audience),
            grant: DelegationGrant::TokenExchange,
        }
    }

    /// An impersonating entry declaring `delegation`. A test changes one field at a time.
    fn impersonating(delegation: &[crate::raw::RawDelegation]) -> RawSourceEntry<'_> {
        RawSourceEntry {
            posture: "impersonation-at-source",
            acknowledged_because: None,
            delegation: Some(delegation),
            ..cube("metrics")
        }
    }

    /// An impersonating source carries the hops each caller's token is exchanged through, in the
    /// order written.
    #[test]
    fn an_impersonating_cube_source_declares_its_delegation_hops_in_order() {
        let declared = [hop("sutura-app"), hop("https://cube.example.com")];
        let registry =
            SourceRegistry::parse(&[impersonating(&declared)], Some(&single_user())).expect("an impersonating cube entry parses");
        let configured = registry.get(&alias("metrics")).expect("the entry is there");
        let placement::SourcePlacement::Cube { delegation, .. } = configured.placement() else {
            panic!("expected a cube placement, got {:?}", configured.placement());
        };
        let audiences: Vec<_> = delegation.iter().map(DelegationDeclared::audience).collect();
        assert_eq!(audiences, ["sutura-app", "https://cube.example.com"]);
    }

    /// A delegation is refused where it would not be what each request sends: missing on an
    /// impersonating entry, declared on a shared one, or bound for an endpoint that is not `https`.
    #[test]
    fn a_cube_delegation_is_refused_unless_an_impersonating_https_source_declares_it() {
        let one = [hop("https://cube.example.com")];
        for (entry, expected) in [
            (
                RawSourceEntry {
                    delegation: None,
                    ..impersonating(&one)
                },
                InvalidDelegation::Missing,
            ),
            (
                RawSourceEntry {
                    delegation: Some(&one),
                    ..cube("metrics")
                },
                InvalidDelegation::NotImpersonating,
            ),
            (
                RawSourceEntry {
                    endpoint: Some("http://cube.example.com"),
                    ..impersonating(&one)
                },
                InvalidDelegation::PlaintextEndpoint,
            ),
        ] {
            let error = refusal(entry);
            assert!(
                matches!(error, InvalidSourceRegistry::Delegation { ref cause, .. } if *cause == expected),
                "{error:?}"
            );
        }
    }

    #[test]
    fn a_cube_source_missing_a_key_or_carrying_a_bad_one_is_refused() {
        for (entry, key) in [
            (
                RawSourceEntry {
                    endpoint: None,
                    ..cube("m")
                },
                "endpoint",
            ),
            (
                RawSourceEntry {
                    token_file: Some(" "),
                    ..cube("m")
                },
                "token_file",
            ),
        ] {
            match refusal(entry) {
                InvalidSourceRegistry::MissingForKind { key: missing, .. } => assert_eq!(missing, key),
                other => panic!("expected `{key}` to be required, got {other:?}"),
            }
        }
        let relative = RawSourceEntry {
            token_file: Some("token"),
            ..cube("m")
        };
        assert!(matches!(
            refusal(relative),
            InvalidSourceRegistry::RelativePath { key: "token_file", .. }
        ));
        let credentials = RawSourceEntry {
            endpoint: Some("https://token@cube.example.com"),
            ..cube("m")
        };
        assert!(matches!(
            refusal(credentials),
            InvalidSourceRegistry::CubeEndpointCredentials { .. }
        ));
    }

    /// Each on its own: `refuse_foreign_keys` names the FIRST written key.
    #[test]
    fn a_key_that_means_nothing_to_a_cube_source_is_refused_naming_it() {
        for (entry, key) in [
            (
                RawSourceEntry {
                    host: Some("127.0.0.1"),
                    ..cube("m")
                },
                "host",
            ),
            (
                RawSourceEntry {
                    password_file: Some("/p"),
                    ..cube("m")
                },
                "password_file",
            ),
            (
                RawSourceEntry {
                    dataset: Some("d"),
                    ..cube("m")
                },
                "dataset",
            ),
            (
                RawSourceEntry {
                    data_dir: Some("/d"),
                    ..cube("m")
                },
                "data_dir",
            ),
        ] {
            match refusal(entry) {
                InvalidSourceRegistry::KeyNotForKind { key: refused, kind, .. } => {
                    assert_eq!((refused, kind), (key, SourceKind::Cube));
                }
                other => panic!("expected `{key}` to be refused, got {other:?}"),
            }
        }
    }

    #[test]
    fn the_cube_keys_are_refused_on_every_other_kind() {
        let postgres = RawSourceEntry {
            kind: "postgres",
            host: Some("127.0.0.1"),
            port: Some(5432),
            database: Some("d"),
            user: Some("u"),
            password_file: Some("/p"),
            transport_mode: Some("plaintext"),
            endpoint: Some("https://cube.example.com"),
            token_file: None,
            ..cube("m")
        };
        match refusal(postgres) {
            InvalidSourceRegistry::KeyNotForKind { key, .. } => assert_eq!(key, "endpoint"),
            other => panic!("expected `endpoint` to be refused, got {other:?}"),
        }
    }
}
