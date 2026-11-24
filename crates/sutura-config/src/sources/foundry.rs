//! The `foundry` entry's own keys: the ontology SQL endpoint and, when shared, its deployment token.
//!
//! This is deliberately separate from the metadata catalog's `foundry` block. A catalog declares
//! object types; a source connection executes plans over the same ontology. Keeping the two
//! placements separate lets one source be used with a local or another catalog without borrowing a
//! catalog credential at query time.
//!
//! An `impersonation-at-source` entry declares no `token_file` and no `verification_identity`:
//! each question sends the caller's own Foundry token, which `foundry_login`
//! (`super::caller_identity`) retrieves per request, and nothing at boot runs as anybody. Either key
//! would name an identity this source never uses, so both are refused rather than kept for a
//! fallback that does not exist - and an anchor on such a source does not boot.

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
    let token_file = if entry.posture.trim() == "impersonation-at-source" {
        for (key, value) in [
            ("token_file", entry.token_file),
            ("verification_identity", entry.verification_identity),
        ] {
            if written(value) {
                return Err(InvalidSourceRegistry::FoundryIdentityBesideCallerLogin {
                    alias: alias.clone(),
                    key,
                });
            }
        }
        None
    } else {
        let token_file = PathBuf::from(super::required(alias, kind, "token_file", entry.token_file)?);
        if token_file.is_relative() {
            return Err(InvalidSourceRegistry::RelativePath {
                alias: alias.clone(),
                key: "token_file",
                path: token_file,
            });
        }
        Some(token_file)
    };
    Ok(SourcePlacement::Foundry {
        endpoint: String::from(endpoint),
        ontology: String::from(ontology),
        token_file,
    })
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
        for (key, line) in [
            ("token_file", "    token_file: /run/sutura/foundry-token\n"),
            ("verification_identity", "    verification_identity: sutura_anchor_reader\n"),
        ] {
            refused_as(
                &declared(DIRECT, "foundry", "impersonation-at-source", &format!("{line}{LOGIN}")),
                &InvalidSourceRegistry::FoundryIdentityBesideCallerLogin { alias: warehouse(), key },
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
