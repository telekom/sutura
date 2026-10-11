//! The `cube` entry's own keys: which of them this kind is opened with, and which are refused for
//! meaning nothing to it.
//!
//! What lives here is this kind's half of `super::entry::parse_placement`; the shared helpers - the
//! foreign-key refusal, the required-key read and the absolute-path read - stay in `entry` and are
//! reached through `super::`, so there is one copy of each and this file cannot narrow one.

use std::path::PathBuf;

use super::entry::dialled_source_keys;
use super::{InvalidSourceRegistry, RawSourceEntry, SourceKind};
use crate::sources::placement::SourcePlacement;
use sutura_domain::model::SourceName;

/// Reads a `kind: cube` entry into its placement.
///
/// **The two declarations are the whole entry**: the API's `endpoint` and the `token_file` its one
/// token is read from. Every dialled key, every `bigquery` and `files` key and a workload-identity
/// block are refused, because the REST client reads none of them and a key written here would sit in
/// the file doing nothing.
///
/// # Errors
///
/// A key that belongs to another kind; a missing `endpoint` or `token_file`; an `endpoint` carrying
/// credentials; a relative `token_file`; and `posture: impersonation-at-source`, which a source with
/// one deployment token has no place to run.
pub(super) fn parse_placement(
    alias: &SourceName,
    kind: SourceKind,
    entry: &RawSourceEntry<'_>,
    written: impl Fn(Option<&str>) -> bool,
) -> Result<SourcePlacement, InvalidSourceRegistry> {
    super::refuse_foreign_keys(alias, kind, foreign_keys(entry, &written))?;
    super::refuse_foreign_keys(alias, kind, dialled_source_keys(entry, written))?;
    if entry.posture.trim() == "impersonation-at-source" {
        return Err(InvalidSourceRegistry::CubeImpersonation { alias: alias.clone() });
    }
    let endpoint = super::required(alias, kind, "endpoint", entry.endpoint)?;
    if endpoint.contains('@') {
        return Err(InvalidSourceRegistry::CubeEndpointCredentials { alias: alias.clone() });
    }
    let token_file: PathBuf = super::parse_absolute(
        alias,
        "token_file",
        super::required(alias, kind, "token_file", entry.token_file)?,
    )?;
    Ok(SourcePlacement::Cube {
        endpoint: endpoint.to_owned(),
        token_file,
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

/// The five keys a `cube` entry has no use for beside the dialled ones: the `files` key, the three
/// `bigquery` keys and a workload-identity block.
fn foreign_keys(entry: &RawSourceEntry<'_>, written: &impl Fn(Option<&str>) -> bool) -> [(&'static str, bool); 5] {
    [
        ("data_dir", written(entry.data_dir)),
        ("billing_project", written(entry.billing_project)),
        ("dataset", written(entry.dataset)),
        ("max_bytes_billed", entry.max_bytes_billed.is_some()),
        ("workload_identity", entry.workload_identity.is_some()),
    ]
}

#[cfg(test)]
mod tests {
    use sutura_domain::model::SourceName;

    use crate::security::DeploymentIdentity;
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
            subjects: None,
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
            placement::SourcePlacement::Cube { endpoint, token_file } => {
                assert_eq!(endpoint, "https://cube.example.com");
                assert_eq!(token_file, std::path::Path::new("/etc/sutura/cube-token"));
            }
            other => panic!("expected a cube placement, got {other:?}"),
        }
    }

    #[test]
    fn a_cube_source_declared_to_impersonate_is_refused() {
        let impersonating = RawSourceEntry {
            posture: "impersonation-at-source",
            acknowledged_because: None,
            ..cube("metrics")
        };
        let error = refusal(impersonating);
        assert!(
            matches!(error, InvalidSourceRegistry::CubeImpersonation { .. }),
            "got {error:?}"
        );
        assert!(
            error.to_string().contains("runs no query as a subject"),
            "the refusal names why: {error}"
        );
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
