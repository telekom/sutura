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
    assert!(shown.contains("token_path: \"/multipass/api/oauth2/token\""), "{shown}");
    assert!(shown.contains("client_id: \"catalog-reader\""), "{shown}");
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
    assert!(shown.contains("EmptyScope"), "{shown}");
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
