//! `catalog.kind: datahub`, with the `datahub` feature ON: the refusals this build now REACHES
//! rather than the "not linked" one above. No real endpoint here - each cell is asserting which
//! typed refusal a boot-time misconfiguration earns, not a read.

use std::path::PathBuf;

use sutura_config::{CatalogKind, CatalogSettings, Catalogs};
use sutura_domain::model::SourceName;
use sutura_domain::pinned::DefinitionVersion;

fn version() -> DefinitionVersion {
    DefinitionVersion::parse("test-1").expect("a test version is a version")
}

fn name() -> SourceName {
    SourceName::parse("metrics").expect("a test name is a name")
}

#[test]
fn a_datahub_catalog_with_no_token_file_is_refused_naming_it() {
    // `CatalogSettings::parse` alone (with no `with_datahub_reader` call, which is what a
    // settings file that declared `catalog.kind: datahub` and nothing else produces) leaves
    // `endpoint`/`token_file`/`metric_property` all `None` - so this reaches PAST the
    // "not linked" refusal above and into the composition step's own required-field checks.
    let datahub = CatalogSettings::parse(
        name(),
        CatalogKind::Datahub,
        PathBuf::from("/nowhere/catalog"),
        PathBuf::from("/nowhere/data"),
        version(),
    )
    .expect("a directory and a version are a settings");
    let catalogs = Catalogs::parse(vec![datahub]).expect("one declared catalog is a registry");
    let err = super::super::catalog::open_catalog(&catalogs).expect_err("no endpoint was declared");
    assert!(err.contains("endpoint"), "{err}");
    assert!(
        !err.contains("--features datahub"),
        "this build DOES link the feature - the refusal must not send an operator chasing one: {err}"
    );
}

#[test]
fn a_datahub_catalog_missing_only_the_token_file_is_refused_naming_that_field() {
    let datahub = CatalogSettings::parse(
        name(),
        CatalogKind::Datahub,
        PathBuf::from("/nowhere/catalog"),
        PathBuf::from("/nowhere/data"),
        version(),
    )
    .expect("a directory and a version are a settings")
    .with_datahub_reader(String::from("https://datahub.example"), PathBuf::new(), String::new())
    .expect_err("an empty token_file and an empty metric_property are both refused");
    assert!(matches!(
        datahub,
        sutura_config::InvalidCatalogSettings::MissingForDatahub { field: "token_file" }
    ));
}

#[test]
fn a_datahub_catalog_with_a_plaintext_endpoint_beyond_loopback_is_refused() {
    // The composition step's own parse (`sutura_catalog_datahub::http::Endpoint::parse`), not
    // `sutura-config`'s - that crate does not depend on the adapter and cannot name the rule
    // itself. This is the settings-layer surface of PR1's `Endpoint` fix: a declared
    // `catalog.endpoint` a deployment could otherwise have written is refused at boot rather
    // than dialled.
    let datahub = CatalogSettings::parse(
        name(),
        CatalogKind::Datahub,
        PathBuf::from("/nowhere/catalog"),
        PathBuf::from("/nowhere/data"),
        version(),
    )
    .expect("a directory and a version are a settings")
    .with_datahub_reader(
        String::from("http://datahub.example.internal"),
        PathBuf::from("/nowhere/token"),
        String::from("deployment_metric_document"),
    )
    .expect("a non-empty endpoint, token_file and metric_property are all this type checks");
    let catalogs = Catalogs::parse(vec![datahub]).expect("one declared catalog is a registry");
    let err = super::super::catalog::open_catalog(&catalogs).expect_err("a plaintext non-loopback endpoint is refused");
    assert!(err.contains("datahub.example.internal"), "{err}");
    assert!(err.contains("endpoint"), "{err}");
}
