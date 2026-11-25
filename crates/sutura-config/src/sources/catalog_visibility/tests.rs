//! `catalog_visibility`'s value, and the entries it is refused on, read through the whole settings load.

use sutura_domain::model::SourceName;

use super::{CatalogVisibility, CatalogVisibilityRefused};
use crate::environment::Environment;
use crate::settings::{Settings, SettingsError, Sources};

const DIRECT: &str = "security:\n  identity: \"multi-user\"\n  inbound:\n    mode: \"direct\"\n    \
                      resource: \"https://sutura.example.com\"\n    authorization_server: \"https://idp.example.com/realms/example\"\n    \
                      key_set_file: \"/etc/sutura/jwks.json\"\n    algorithms: [\"RS256\"]\n";

const LOGIN: &str = "    posture: \"impersonation-at-source\"\n    foundry_login:\n      \
                     token_endpoint: \"https://idp.example.com/realms/example/broker/foundry/token\"\n      \
                     client_id: \"sutura\"\n      client_secret_file: \"/run/secrets/idp-client-secret\"\n";

const SHARED: &str = "    posture: \"shared-service-user\"\n    acknowledged_because: \"one service user\"\n    \
                      token_file: \"/run/sutura/foundry-token\"\n";

/// One `foundry` source, `warehouse`, declaring `identity` and then `extra`.
fn foundry(identity: &str, extra: &str) -> Sources {
    Sources::defaults(Environment::Development).with_overlay(format!(
        "{DIRECT}sources:\n  warehouse:\n    kind: \"foundry\"\n    endpoint: \"https://foundry.example.com\"\n    \
         ontology: \"sales\"\n{identity}{extra}"
    ))
}

fn refusal(sources: &Sources) -> CatalogVisibilityRefused {
    let error = Settings::load(sources).expect_err("the narrowing is refused at load");
    match *error.reason() {
        SettingsError::CatalogVisibility { ref cause } => {
            assert_eq!(cause.alias().as_str(), "warehouse");
            cause.refused()
        }
        ref other => panic!("expected a catalog_visibility refusal, got {other:?}"),
    }
}

#[test]
fn a_foundry_login_source_carries_its_per_caller_catalog() {
    let settings = Settings::load(&foundry(LOGIN, "    catalog_visibility: \"per-caller\"\n")).expect("the deployment loads");
    let source = settings
        .sources()
        .get(&SourceName::parse("warehouse").expect("a legal identifier"))
        .expect("the warehouse entry is declared");
    assert_eq!(CatalogVisibility::of(source), Some(CatalogVisibility::PerCaller));
}

#[test]
fn per_caller_visibility_without_a_foundry_login_to_list_with_is_refused() {
    assert_eq!(
        refusal(&foundry(SHARED, "    catalog_visibility: \"per-caller\"\n")),
        CatalogVisibilityRefused::NoCallerLogin,
        "a shared source holds no caller's token, so nothing could list as the caller"
    );
}

#[test]
fn a_catalog_visibility_other_than_per_caller_is_refused_not_read_as_absent() {
    for written in ["\"deployment\"", "\"Per-Caller\"", "\"true\""] {
        assert_eq!(
            refusal(&foundry(LOGIN, &format!("    catalog_visibility: {written}\n"))),
            CatalogVisibilityRefused::Unknown,
            "{written}"
        );
    }
}
