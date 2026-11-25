//! The `foundry` entry's own keys: the ontology SQL endpoint and, when shared, its deployment token.
//!
//! This is deliberately separate from the metadata catalog's `foundry` block. A catalog declares
//! object types; a source connection executes plans over the same ontology. Keeping the two
//! placements separate lets one source be used with a local or another catalog without borrowing a
//! catalog credential at query time.
//!
//! # Whose token, per posture
//!
//! A `shared-service-user` entry declares exactly one deployment identity: a `token_file`, read
//! once at boot, or a `client_credentials` block (`crate::client_credentials`), minted on the
//! entry's own `endpoint` and minted again before it expires.
//!
//! An `impersonation-at-source` entry declares neither: each question sends the caller's own
//! Foundry token, which `foundry_login` (`super::caller_identity`) retrieves per request, and a
//! deployment token beside it would read as a fallback that does not exist. It MAY declare a
//! startup-only identity - `verification_identity` naming it and `verification_client_credentials`
//! holding its grant, both or neither - which runs the anchors at boot and never a caller's
//! question. Without one, an anchor on the source does not boot.

use std::path::PathBuf;

use super::entry::dialled_source_keys;
use super::placement::SourcePlacement;
use super::{InvalidSourceRegistry, RawSourceEntry, SourceKind};
use sutura_domain::model::SourceName;

/// Reads a `kind: foundry` entry.
pub(super) fn parse_placement(
    alias: &SourceName,
    kind: SourceKind,
    entry: &RawSourceEntry<'_>,
    written: impl Fn(Option<&str>) -> bool,
) -> Result<SourcePlacement, InvalidSourceRegistry> {
    super::refuse_foreign_keys(
        alias,
        kind,
        [
            ("data_dir", written(entry.data_dir)),
            ("max_bytes_billed", entry.max_bytes_billed.is_some()),
        ],
    )?;
    super::refuse_foreign_keys(alias, kind, dialled_source_keys(entry, &written))?;
    let endpoint = super::required(alias, kind, "endpoint", entry.endpoint)?;
    let ontology = super::required(alias, kind, "ontology", entry.ontology)?;
    let token_file = entry.token_file.filter(|_| written(entry.token_file)).map(PathBuf::from);
    if let Some(ref token_file) = token_file
        && token_file.is_relative()
    {
        return Err(InvalidSourceRegistry::RelativePath {
            alias: alias.clone(),
            key: "token_file",
            path: token_file.clone(),
        });
    }
    // Which identity keys agree with the posture is `declare`'s, once both blocks are read.
    Ok(SourcePlacement::Foundry {
        endpoint: String::from(endpoint),
        ontology: String::from(ontology),
        token_file,
        client_credentials: None,
        verification_client_credentials: None,
        exclude_object_types: sutura_domain::model::ObjectTypeExclusions::default(),
        exclusions_written: false,
    })
}

/// Why a `foundry` source's identity keys do not agree with its posture.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidFoundrySource {
    /// A deployment identity on an impersonating source, which holds none for a question.
    #[error(
        "`{key}` is declared on an `impersonation-at-source` Foundry source, which runs each question with \
         its caller's own token from `foundry_login` - remove `{key}`"
    )]
    IdentityBesideCallerLogin { key: &'static str },
    #[error("a `shared-service-user` Foundry source needs one of `token_file` or `client_credentials`, and neither is written")]
    NoDeploymentIdentity,
    #[error("a `shared-service-user` Foundry source takes one of `token_file` or `client_credentials`, and both are written")]
    TwoDeploymentIdentities,
    /// A startup identity on a shared source, whose anchors already run as its deployment identity.
    #[error(
        "`verification_client_credentials` is declared on a `shared-service-user` Foundry source, whose \
         anchors run as its deployment identity - remove it"
    )]
    VerificationOnShared,
    /// One half of an impersonating source's startup identity without the other.
    #[error("`{written}` is declared without `{missing}` - a Foundry startup identity takes both, or neither")]
    VerificationIncomplete { written: &'static str, missing: &'static str },
    #[error("a declared client-credentials block is not usable")]
    ClientCredentials(#[source] crate::client_credentials::InvalidClientCredentials),
    #[error("`exclude_object_types` holds {value:?}, which is neither an exact API name nor a prefix ending in `.*`")]
    ExcludeObjectTypes { value: String },
    /// The source and the catalog of the same alias each wrote `exclude_object_types`, differently.
    #[error(
        "`exclude_object_types` on the source and `foundry.exclude_object_types` on the catalog of the same name \
         differ - write it once, or write the same list in both"
    )]
    ExclusionsDisagree,
}

/// Reads one source's `client_credentials` and `verification_client_credentials` blocks into its
/// placement, and holds every identity key of a `foundry` source to its posture. Run for every
/// declared source, so a `foundry` source never leaves the registry unchecked; a block on any other
/// kind is refused as a key that kind does not read.
pub(crate) fn declare(
    registry: &mut super::SourceRegistry,
    written: &str,
    client_credentials: Option<&crate::client_credentials::RawClientCredentials>,
    verification_client_credentials: Option<&crate::client_credentials::RawClientCredentials>,
    exclude_object_types: Option<&[String]>,
) -> Result<(), InvalidSourceRegistry> {
    use crate::client_credentials::ClientCredentialsSettings;
    use sutura_domain::source::SourcePosture;

    let alias = SourceName::parse(written).map_err(|cause| InvalidSourceRegistry::Alias {
        written: String::from(written),
        cause,
    })?;
    let Some(source) = registry.by_alias.get_mut(&alias) else {
        return Ok(());
    };
    let kind = source.kind();
    if kind != SourceKind::Foundry {
        return super::refuse_foreign_keys(
            &alias,
            kind,
            [
                ("client_credentials", client_credentials.is_some()),
                ("verification_client_credentials", verification_client_credentials.is_some()),
                ("exclude_object_types", exclude_object_types.is_some()),
            ],
        );
    }
    let refused = |cause| InvalidSourceRegistry::Foundry {
        alias: alias.clone(),
        cause,
    };
    let parse = |raw: Option<&crate::client_credentials::RawClientCredentials>, key: &str| {
        raw.map(|raw| ClientCredentialsSettings::parse(raw, &format!("sources.{alias}.{key}")))
            .transpose()
            .map_err(|cause| refused(InvalidFoundrySource::ClientCredentials(cause)))
    };
    let grant = parse(client_credentials, "client_credentials")?;
    let exclusions = exclude_object_types
        .map(|patterns| sutura_domain::model::ObjectTypeExclusions::parse(patterns.iter().map(String::as_str)))
        .transpose()
        .map_err(|cause| {
            refused(InvalidFoundrySource::ExcludeObjectTypes {
                value: String::from(cause.value()),
            })
        })?;
    let verification = parse(verification_client_credentials, "verification_client_credentials")?;
    let impersonating = matches!(source.posture(), Some(SourcePosture::ImpersonationAtSource));
    let named = source
        .identity()
        .is_some_and(|identity| !matches!(identity.anchors_run_as(), sutura_domain::source::AnchorIdentity::NoneDeclared));
    let SourcePlacement::Foundry {
        ref token_file,
        client_credentials: ref mut held,
        verification_client_credentials: ref mut held_verification,
        exclude_object_types: ref mut held_exclusions,
        ref mut exclusions_written,
        ..
    } = source.placement
    else {
        return Ok(());
    };
    if impersonating {
        if token_file.is_some() {
            return Err(refused(InvalidFoundrySource::IdentityBesideCallerLogin { key: "token_file" }));
        }
        if grant.is_some() {
            return Err(refused(InvalidFoundrySource::IdentityBesideCallerLogin {
                key: "client_credentials",
            }));
        }
        match (named, verification.is_some()) {
            (true, false) => {
                return Err(refused(InvalidFoundrySource::VerificationIncomplete {
                    written: "verification_identity",
                    missing: "verification_client_credentials",
                }));
            }
            (false, true) => {
                return Err(refused(InvalidFoundrySource::VerificationIncomplete {
                    written: "verification_client_credentials",
                    missing: "verification_identity",
                }));
            }
            (true, true) | (false, false) => {}
        }
    } else {
        if verification.is_some() {
            return Err(refused(InvalidFoundrySource::VerificationOnShared));
        }
        match (token_file.is_some(), grant.is_some()) {
            (false, false) => return Err(refused(InvalidFoundrySource::NoDeploymentIdentity)),
            (true, true) => return Err(refused(InvalidFoundrySource::TwoDeploymentIdentities)),
            (true, false) | (false, true) => {}
        }
    }
    *held = grant;
    *held_verification = verification;
    if let Some(exclusions) = exclusions {
        *held_exclusions = exclusions;
        *exclusions_written = true;
    }
    Ok(())
}

/// The exclusions the `foundry` source `alias` wrote itself, if it wrote any.
pub(crate) fn written_exclusions<'a>(
    registry: &'a super::SourceRegistry,
    alias: &SourceName,
) -> Option<&'a sutura_domain::model::ObjectTypeExclusions> {
    match registry.by_alias.get(alias)?.placement {
        SourcePlacement::Foundry {
            ref exclude_object_types,
            exclusions_written: true,
            ..
        } => Some(exclude_object_types),
        _ => None,
    }
}

/// Gives the `foundry` source `alias` the exclusions its catalog of the same name wrote, unless
/// the source wrote its own: the source's key wins.
///
/// # Errors
///
/// [`InvalidFoundrySource::ExclusionsDisagree`] when both wrote it, differently.
pub(crate) fn exclude_object_types(
    registry: &mut super::SourceRegistry,
    alias: &SourceName,
    catalog: &sutura_domain::model::ObjectTypeExclusions,
) -> Result<(), InvalidSourceRegistry> {
    if let Some(source) = registry.by_alias.get_mut(alias)
        && let SourcePlacement::Foundry {
            ref mut exclude_object_types,
            exclusions_written,
            ..
        } = source.placement
    {
        if !exclusions_written {
            exclude_object_types.clone_from(catalog);
        } else if exclude_object_types != catalog {
            return Err(InvalidSourceRegistry::Foundry {
                alias: alias.clone(),
                cause: InvalidFoundrySource::ExclusionsDisagree,
            });
        }
    }
    Ok(())
}

/// Allows `endpoint` and `token_file` only on the kinds that read them.
pub(super) fn refuse_elsewhere(
    alias: &SourceName,
    kind: SourceKind,
    entry: &RawSourceEntry<'_>,
    written: impl Fn(Option<&str>) -> bool,
) -> Result<(), InvalidSourceRegistry> {
    super::refuse_foreign_keys(
        alias,
        kind,
        [
            ("ontology", kind != SourceKind::Foundry && written(entry.ontology)),
            (
                "token_file",
                !matches!(kind, SourceKind::Cube | SourceKind::Foundry) && written(entry.token_file),
            ),
            (
                "endpoint",
                !matches!(kind, SourceKind::Cube | SourceKind::Node | SourceKind::Foundry) && written(entry.endpoint),
            ),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::InvalidFoundrySource;
    use crate::environment::Environment;
    use crate::settings::{Settings, SettingsError, Sources};
    use crate::sources::{InvalidSourceRegistry, SourcePlacement};
    use sutura_domain::model::SourceName;

    fn overlay(kind: &str) -> String {
        let token_file = if kind == "node" {
            ""
        } else {
            "    token_file: /run/sutura/foundry-token\n"
        };
        format!(
            "security:\n  identity: single-user\n  single_user_because: one operator\nsources:\n  warehouse:\n    kind: {kind}\n    endpoint: https://foundry.example.com\n    ontology: sales\n{token_file}    posture: shared-service-user\n"
        )
    }

    #[test]
    fn foundry_ontology_is_read_only_by_the_foundry_source_kind() {
        let settings = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay("foundry")))
            .expect("the Foundry source is declared");
        let source = settings
            .sources()
            .get(&SourceName::parse("warehouse").expect("the alias parses"))
            .expect("the source exists");
        assert!(matches!(source.placement(), SourcePlacement::Foundry { ontology, .. } if ontology == "sales"));
        for kind in ["cube", "node"] {
            let error = Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay(kind)))
                .expect_err("other endpoint sources must refuse ontology");
            assert!(
                matches!(
                    error.reason(),
                    SettingsError::Sources {
                        cause: InvalidSourceRegistry::KeyNotForKind { key: "ontology", .. }
                    }
                ),
                "{kind}: {error:?}"
            );
        }
    }

    const DIRECT: &str = "  inbound:\n    mode: direct\n    resource: https://sutura.example.com\n    \
                          authorization_server: https://idp.example.com/realms/example\n    \
                          key_set_file: /etc/sutura/jwks.json\n    algorithms: [RS256]\n";

    const LOGIN: &str = "    foundry_login:\n      token_endpoint: https://idp.example.com/realms/example/broker/foundry/token\n      \
                         client_id: sutura\n      client_secret_file: /run/secrets/idp-client-secret\n";

    /// One `foundry` source, `warehouse`, under `posture`, with `extra` beneath it, on a deployment
    /// declaring `inbound`.
    fn declared(inbound: &str, kind: &str, posture: &str, extra: &str) -> Sources {
        Sources::defaults(Environment::Development).with_overlay(format!(
            "security:\n  identity: single-user\n  single_user_because: one operator\n{inbound}sources:\n  warehouse:\n    \
             kind: {kind}\n    endpoint: https://foundry.example.com\n    ontology: sales\n    posture: {posture}\n{extra}"
        ))
    }

    fn warehouse() -> SourceName {
        SourceName::parse("warehouse").expect("the alias parses")
    }

    /// Asserts `sources` is refused at load as `expected`, and answers the refusal as rendered.
    fn refused_as(sources: &Sources, expected: &InvalidSourceRegistry) -> String {
        let error = Settings::load(sources).expect_err("the entry is refused");
        let SettingsError::Sources { ref cause } = *error.reason() else {
            panic!("expected a sources refusal, got {error:?}");
        };
        assert_eq!(cause, expected);
        cause.to_string()
    }

    #[test]
    fn a_foundry_login_source_boots_on_a_direct_deployment_with_no_deployment_token() {
        let settings = Settings::load(&declared(DIRECT, "foundry", "impersonation-at-source", LOGIN))
            .expect("a foundry_login source is served");
        assert!(settings.refusals().is_empty(), "{:?}", settings.refusals());
        let source = settings.sources().get(&warehouse()).expect("the source exists");
        assert!(matches!(
            source.placement(),
            SourcePlacement::Foundry { token_file: None, .. }
        ));
        let Some(crate::sources::caller_identity::CallerIdentity::FoundryLogin(login)) = source.caller_identity() else {
            panic!("the foundry_login block is carried: {:?}", source.caller_identity());
        };
        assert_eq!(
            login.token_endpoint(),
            "https://idp.example.com/realms/example/broker/foundry/token"
        );
    }

    #[test]
    fn a_foundry_login_source_is_refused_without_a_direct_caller() {
        let error = Settings::load(&declared("", "foundry", "impersonation-at-source", LOGIN))
            .expect_err("the broker request sends the caller's own inbound token, which only `direct` verifies");
        let SettingsError::NotFitToServe { ref refusals } = *error.reason() else {
            panic!("expected a boot refusal, got {error:?}");
        };
        assert_eq!(
            *refusals,
            vec![crate::settings::NotFitToServe::DelegationWithoutDirectInbound {
                alias: warehouse(),
                key: "foundry_login",
            }]
        );
    }

    #[test]
    fn a_deployment_identity_beside_foundry_login_is_refused_at_load() {
        for (key, block) in [
            ("token_file", String::from("    token_file: /run/sutura/foundry-token\n")),
            ("client_credentials", grant("client_credentials")),
        ] {
            refused_as(
                &declared(DIRECT, "foundry", "impersonation-at-source", &format!("{block}{LOGIN}")),
                &InvalidSourceRegistry::Foundry {
                    alias: warehouse(),
                    cause: InvalidFoundrySource::IdentityBesideCallerLogin { key },
                },
            );
        }
    }

    /// A client-credentials block under `key`, four-space indented as an entry's key.
    fn grant(key: &str) -> String {
        format!(
            "    {key}:\n      client_id: sutura-source\n      client_secret_file: /run/secrets/foundry-client-secret\n      \
             scopes: [api:sql-queries-execute, api:ontologies-read]\n"
        )
    }

    const TOKEN_FILE: &str = "    token_file: /run/sutura/foundry-token\n";

    #[test]
    fn a_shared_foundry_source_declares_a_token_file_or_client_credentials_and_not_both() {
        let settings = Settings::load(&declared("", "foundry", "shared-service-user", &grant("client_credentials")))
            .expect("a shared source with a client-credentials grant loads");
        let source = settings.sources().get(&warehouse()).expect("the source exists");
        let SourcePlacement::Foundry {
            token_file: None,
            client_credentials: Some(ref declared_grant),
            verification_client_credentials: None,
            ..
        } = *source.placement()
        else {
            panic!("the grant is carried in place of a token file: {:?}", source.placement());
        };
        assert_eq!(declared_grant.client_id(), "sutura-source");
        assert_eq!(
            declared_grant.token_path().as_str(),
            crate::client_credentials::DEFAULT_TOKEN_PATH
        );
        assert_eq!(
            declared_grant.scopes().as_str(),
            "api:sql-queries-execute api:ontologies-read"
        );
        for (extra, cause) in [
            (String::new(), InvalidFoundrySource::NoDeploymentIdentity),
            (
                format!("{TOKEN_FILE}{}", grant("client_credentials")),
                InvalidFoundrySource::TwoDeploymentIdentities,
            ),
            (
                format!("{TOKEN_FILE}{}", grant("verification_client_credentials")),
                InvalidFoundrySource::VerificationOnShared,
            ),
        ] {
            refused_as(
                &declared("", "foundry", "shared-service-user", &extra),
                &InvalidSourceRegistry::Foundry {
                    alias: warehouse(),
                    cause,
                },
            );
        }
    }

    #[test]
    fn a_source_client_credentials_block_is_read_by_the_one_parser_and_named_by_its_key() {
        let relative = "    client_credentials:\n      client_id: sutura-source\n      client_secret_file: secrets/client\n";
        refused_as(
            &declared("", "foundry", "shared-service-user", relative),
            &InvalidSourceRegistry::Foundry {
                alias: warehouse(),
                cause: InvalidFoundrySource::ClientCredentials(
                    crate::client_credentials::InvalidClientCredentials::RelativeSecretFile {
                        key: String::from("sources.warehouse.client_credentials"),
                    },
                ),
            },
        );
        let off_host = format!(
            "{}      token_path: https://elsewhere.example.com/token\n",
            grant("client_credentials")
        );
        refused_as(
            &declared("", "foundry", "shared-service-user", &off_host),
            &InvalidSourceRegistry::Foundry {
                alias: warehouse(),
                cause: InvalidFoundrySource::ClientCredentials(crate::client_credentials::InvalidClientCredentials::TokenPath {
                    key: String::from("sources.warehouse.client_credentials"),
                }),
            },
        );
        let on_cube = "security:\n  identity: single-user\n  single_user_because: one operator\nsources:\n  warehouse:\n    \
                       kind: cube\n    endpoint: https://cube.example.com\n    token_file: /run/cube-token\n    \
                       posture: shared-service-user\n";
        let error = Settings::load(
            &Sources::defaults(Environment::Development).with_overlay(format!("{on_cube}{}", grant("client_credentials"))),
        )
        .expect_err("only a foundry source reads the block");
        assert!(
            matches!(
                error.reason(),
                SettingsError::Sources {
                    cause: InvalidSourceRegistry::KeyNotForKind {
                        key: "client_credentials",
                        ..
                    }
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn an_impersonating_foundry_source_takes_a_startup_identity_as_a_pair() {
        let named = "    verification_identity: sutura_anchor_reader\n";
        let settings = Settings::load(&declared(
            DIRECT,
            "foundry",
            "impersonation-at-source",
            &format!("{named}{}{LOGIN}", grant("verification_client_credentials")),
        ))
        .expect("a named startup identity with its grant loads");
        let source = settings.sources().get(&warehouse()).expect("the source exists");
        assert!(matches!(
            source.placement(),
            SourcePlacement::Foundry {
                token_file: None,
                client_credentials: None,
                verification_client_credentials: Some(_),
                ..
            }
        ));
        for (extra, written, missing) in [
            (
                String::from(named),
                "verification_identity",
                "verification_client_credentials",
            ),
            (
                grant("verification_client_credentials"),
                "verification_client_credentials",
                "verification_identity",
            ),
        ] {
            refused_as(
                &declared(DIRECT, "foundry", "impersonation-at-source", &format!("{extra}{LOGIN}")),
                &InvalidSourceRegistry::Foundry {
                    alias: warehouse(),
                    cause: InvalidFoundrySource::VerificationIncomplete { written, missing },
                },
            );
        }
    }

    #[test]
    fn foundry_login_runs_only_an_impersonating_foundry_source() {
        refused_as(
            &declared(
                DIRECT,
                "foundry",
                "shared-service-user",
                &format!("    token_file: /run/sutura/foundry-token\n{LOGIN}"),
            ),
            &InvalidSourceRegistry::IdentityModeNotImpersonating {
                alias: warehouse(),
                block: "foundry_login",
            },
        );
        let bigquery = Sources::defaults(Environment::Development).with_overlay(format!(
            "security:\n  identity: single-user\n  single_user_because: one operator\n{DIRECT}sources:\n  warehouse:\n    \
             kind: bigquery\n    max_bytes_billed: 1073741824\n    posture: impersonation-at-source\n{LOGIN}"
        ));
        let refusal = refused_as(
            &bigquery,
            &InvalidSourceRegistry::IdentityModeNotForKind {
                alias: warehouse(),
                block: "foundry_login",
            },
        );
        assert!(refusal.contains("`foundry` source"), "{refusal}");
        let pool = "    workload_identity:\n      audience: \"//iam.googleapis.com/projects/1/locations/global/workloadIdentityPools/p/providers/v\"\n";
        refused_as(
            &declared(DIRECT, "foundry", "impersonation-at-source", pool),
            &InvalidSourceRegistry::IdentityModeNotForKind {
                alias: warehouse(),
                block: "workload_identity",
            },
        );
    }

    #[test]
    fn an_impersonating_foundry_source_without_foundry_login_is_refused_naming_it() {
        let refusal = refused_as(
            &declared(DIRECT, "foundry", "impersonation-at-source", ""),
            &InvalidSourceRegistry::NoCallerIdentityMode { alias: warehouse() },
        );
        assert!(refusal.contains("foundry_login"), "{refusal}");
    }
}
