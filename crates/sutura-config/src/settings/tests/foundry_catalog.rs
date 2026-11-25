//! `catalogs[].kind: foundry` through `Settings::load`: a token file or a client-credentials grant
//! loads with no catalog directories, and every boot refusal of the `foundry:` block names its key.
//!
//! The cells read the settings only through what the base tree already exports - `Settings::load`,
//! the refusal's `Debug` and the catalogs' `Debug` - so they build there and fail on its behaviour.

use crate::settings::{Environment, Settings, SettingsLoadError, Sources};

const PRELUDE: &str = "security:\n  identity: single-user\n  single_user_because: a test\n";

/// One `foundry` catalog entry whose own keys are `entry`, indented under the list item.
fn overlay(entry: &str) -> String {
    format!(
        "{PRELUDE}catalogs:\n  - name: ontology\n    kind: foundry\n    version: test-1\n    \
         endpoint: https://foundry.example.com\n{entry}"
    )
}

fn load(overlay: &str) -> Result<Settings, SettingsLoadError> {
    Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
}

fn refused(overlay: &str) -> String {
    format!("{:?}", load(overlay).expect_err("the declaration is refused"))
}

const ONTOLOGY: &str = "    foundry:\n      ontology: ri.ontology.main.ontology.00000000-0000-0000-0000-000000000000\n";

const GRANT: &str = "      client_credentials:\n        client_id: catalog-reader\n        \
                     client_secret_file: /run/secrets/foundry-client-secret\n        scopes: [\"api:ontologies-read\"]\n";

#[test]
fn a_foundry_catalog_with_a_token_file_loads_without_directories() {
    let settings = load(&overlay(&format!("    token_file: /run/secrets/foundry-token\n{ONTOLOGY}")))
        .expect("a token file and an ontology load");
    let shown = format!("{:?}", settings.catalogs());
    assert!(shown.contains("kind: Foundry"), "{shown}");
    assert!(
        shown.contains("Token { token_file: \"/run/secrets/foundry-token\" }"),
        "{shown}"
    );
}

#[test]
fn a_client_credentials_grant_loads_on_the_default_token_path() {
    let settings = load(&overlay(&format!("{ONTOLOGY}{GRANT}"))).expect("a grant loads");
    let shown = format!("{:?}", settings.catalogs());
    assert!(
        shown.contains("token_path: TokenPath(\"/multipass/api/oauth2/token\")"),
        "{shown}"
    );
    assert!(shown.contains("client_id: \"catalog-reader\""), "{shown}");
}

/// A `foundry` catalog writing `catalog_exclusions` and the source of its name writing `source_exclusions`.
fn with_source(catalog_exclusions: &str, source_exclusions: &str) -> String {
    format!(
        "{}{catalog_exclusions}sources:\n  ontology:\n    kind: foundry\n    endpoint: https://foundry.example.com\n    \
         ontology: sales\n    token_file: /run/secrets/foundry-query-token\n    posture: shared-service-user\n{source_exclusions}",
        overlay(&format!("    token_file: /run/secrets/foundry-token\n{ONTOLOGY}"))
    )
}

fn source_exclusions(settings: &Settings) -> String {
    let alias = sutura_domain::model::SourceName::parse("ontology").expect("an alias");
    let source = settings.sources().get(&alias).expect("the source is declared");
    format!("{:?}", source.placement())
}

#[test]
fn a_source_writes_its_own_exclusions_and_wins_over_the_catalog_default() {
    let default = load(&with_source("", "")).expect("no exclusions written loads");
    assert!(
        source_exclusions(&default).contains("com.palantir.system."),
        "{}",
        source_exclusions(&default)
    );

    let opted_out = load(&with_source("", "    exclude_object_types: []\n")).expect("an empty list loads");
    let shown = source_exclusions(&opted_out);
    assert!(shown.contains("ObjectTypeExclusions({})"), "{shown}");
    let catalog = opted_out
        .catalogs()
        .each()
        .find_map(crate::catalog::CatalogSettings::foundry)
        .expect("the foundry catalog")
        .exclusions();
    assert_eq!(
        catalog,
        sutura_domain::model::ObjectTypeExclusions::none(),
        "the catalog took the list only the source wrote"
    );

    let copied = load(&with_source("      exclude_object_types: [\"Aircraft\"]\n", "")).expect("the catalog's list loads");
    assert!(
        source_exclusions(&copied).contains("Exact(\"Aircraft\")"),
        "{}",
        source_exclusions(&copied)
    );

    let agreeing = load(&with_source(
        "      exclude_object_types: [\"Aircraft\"]\n",
        "    exclude_object_types: [\"Aircraft\"]\n",
    ))
    .expect("the same list on both loads");
    assert!(source_exclusions(&agreeing).contains("Exact(\"Aircraft\")"));
    load(&with_source(
        "      exclude_object_types: [\"Aircraft\", \"com.example.*\"]\n",
        "    exclude_object_types: [\"com.example.*\", \"Aircraft\", \"Aircraft\"]\n",
    ))
    .expect("the same patterns in another order, or repeated, are the same list");

    let disagreeing = refused(&with_source(
        "      exclude_object_types: [\"Aircraft\"]\n",
        "    exclude_object_types: []\n",
    ));
    assert!(
        disagreeing.contains("ExclusionsDisagree") && disagreeing.contains("ontology"),
        "{disagreeing}"
    );
    let malformed = refused(&with_source("", "    exclude_object_types: [\"com.*.x\"]\n"));
    assert!(
        malformed.contains("ExcludeObjectTypes") && malformed.contains("com.*.x"),
        "{malformed}"
    );
}

#[test]
fn neither_or_both_auths_is_refused() {
    let neither = refused(&overlay(ONTOLOGY));
    assert!(neither.contains("NoAuth"), "{neither}");
    let both = refused(&overlay(&format!(
        "    token_file: /run/secrets/foundry-token\n{ONTOLOGY}{GRANT}"
    )));
    assert!(both.contains("TwoAuths"), "{both}");
}

#[test]
fn a_missing_ontology_or_client_is_refused_naming_the_key() {
    let no_block = refused(&overlay("    token_file: /run/secrets/foundry-token\n"));
    assert!(no_block.contains("Missing { key: \"foundry\" }"), "{no_block}");
    let no_ontology = refused(&overlay(&format!("    foundry:\n{GRANT}")));
    assert!(no_ontology.contains("Missing { key: \"foundry.ontology\" }"), "{no_ontology}");
    let no_client = refused(&overlay(&format!(
        "{ONTOLOGY}      client_credentials:\n        client_secret_file: /run/secrets/foundry-client-secret\n"
    )));
    assert!(
        no_client.contains("Missing { key: \"foundry.client_credentials.client_id\" }"),
        "{no_client}"
    );
}

#[test]
fn a_token_path_that_could_name_another_host_is_refused() {
    for path in [
        "https://identity.example.com/token",
        "//identity.example.com/token",
        "token",
        "/token?x=1",
        "/user@token",
    ] {
        let shown = refused(&overlay(&format!("{ONTOLOGY}{GRANT}        token_path: \"{path}\"\n")));
        assert!(shown.contains("TokenPath"), "{path}: {shown}");
    }
}

#[test]
fn an_empty_scope_is_refused() {
    let grant = GRANT.replace("[\"api:ontologies-read\"]", "[\"api:ontologies-read\", \" \"]");
    let shown = refused(&overlay(&format!("{ONTOLOGY}{grant}")));
    assert!(shown.contains("NotAScopeToken"), "{shown}");
}

#[test]
fn a_foundry_block_on_another_kind_is_refused_naming_the_catalog() {
    let overlay = format!(
        "{PRELUDE}catalogs:\n  - name: docs\n    kind: markdown\n    dir: catalog\n    data_dir: data\n    \
         version: test-1\n{ONTOLOGY}"
    );
    let shown = refused(&overlay);
    assert!(shown.contains("FoundryOnOtherKind"), "{shown}");
}
