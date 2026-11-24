use std::path::{Path, PathBuf};

use sutura_domain::model::SourceName;
use sutura_domain::pinned::DefinitionVersion;

use super::{CatalogKind, CatalogSettings, Catalogs, InvalidCatalogSettings};
use crate::catalog::rdbms::{InvalidDescriptionSource, InvalidRdbmsCatalog};
use crate::settings::SettingsError;
use crate::{Environment, Settings, Sources};

/// Unwraps a `load` failure down to the typed `InvalidDescriptionSource`, so a cell can assert the
/// specific refusal rather than only that loading failed - the same `expect_err` would also pass on
/// an unrelated error, including one base already produces today.
pub(super) fn assert_description_error(
    result: Result<Settings, crate::settings::SettingsLoadError>,
    expected: &InvalidDescriptionSource,
) {
    let error = result.expect_err("a refused description source");
    let SettingsError::Catalog {
        cause: InvalidCatalogSettings::Rdbms { cause, .. },
    } = error.reason()
    else {
        panic!("expected SettingsError::Catalog(InvalidCatalogSettings::Rdbms), got {error:?}");
    };
    let InvalidRdbmsCatalog::Descriptions { cause } = cause else {
        panic!("expected InvalidRdbmsCatalog::Descriptions, got {cause:?}");
    };
    assert_eq!(cause, expected, "{error:?}");
}

fn version() -> DefinitionVersion {
    DefinitionVersion::parse("test-1").expect("a test version is a version")
}

fn kind() -> CatalogKind {
    CatalogKind::Markdown
}

fn name(raw: &str) -> SourceName {
    SourceName::parse(raw).expect("a test catalog name is a name")
}

fn settings(name_raw: &str) -> CatalogSettings {
    CatalogSettings::parse(
        name(name_raw),
        kind(),
        PathBuf::from("/nowhere/catalog"),
        PathBuf::from("/nowhere/data"),
        version(),
    )
    .expect("a declared catalog is a catalog")
}

#[test]
fn an_unknown_catalog_kind_is_refused_against_the_available_ones() {
    // `SourceKind`'s precedent, on the metadata side: the vocabulary is closed, and an unknown
    // word is a parse refusal listing what it could have been rather than a silent default.
    assert_eq!(
        CatalogKind::parse("markdown").expect("markdown is a kind"),
        CatalogKind::Markdown
    );
    assert_eq!(
        CatalogKind::parse("datahub").expect("datahub is a kind"),
        CatalogKind::Datahub
    );
    let error = CatalogKind::parse("atlas").expect_err("atlas is not a kind this build has");
    assert!(error.to_string().contains("atlas"), "{}", error);
    assert!(error.to_string().contains("markdown"), "{}", error);
    assert!(CatalogKind::NAMES.contains(&"markdown"));
    assert_eq!(
        CatalogKind::parse(CatalogKind::Markdown.as_str()).expect("a spelling is a kind"),
        CatalogKind::Markdown
    );
}

#[test]
fn the_three_declaring_kinds_added_by_issue_970_parse_and_spell() {
    // `#970` added `okf`/`openmetadata`/`rdbms` to the closed vocabulary: adapters with a
    // crate and a recorded fixture and, at first, no composition root. Each parses and spells
    // back its own word, and all appear in `NAMES` so a refusal lists the whole set.
    for (kind, word) in [
        (CatalogKind::Okf, "okf"),
        (CatalogKind::Openmetadata, "openmetadata"),
        (CatalogKind::Rdbms, "rdbms"),
    ] {
        assert_eq!(CatalogKind::parse(word).expect("a new kind is a kind"), kind);
        assert_eq!(kind.as_str(), word);
        assert!(CatalogKind::NAMES.contains(&word), "{word} is not in NAMES");
    }
}

#[test]
fn the_datacontract_kind_parses_and_spells() {
    // `#973` added `datacontract` to the closed vocabulary, the same shape `#970`'s three
    // kinds took above - kept as its own cell rather than folded into that one, so a future
    // diff to either proves only what it touched.
    assert_eq!(
        CatalogKind::parse("datacontract").expect("datacontract is a kind"),
        CatalogKind::DataContract
    );
    assert_eq!(CatalogKind::DataContract.as_str(), "datacontract");
    assert!(CatalogKind::NAMES.contains(&"datacontract"));
}

#[test]
fn an_empty_directory_is_refused_rather_than_resolving_to_the_working_directory() {
    let error = CatalogSettings::parse(name("catalog"), kind(), PathBuf::new(), PathBuf::from("data"), version())
        .expect_err("an empty catalog directory is not a directory");
    assert_eq!(error, InvalidCatalogSettings::EmptyPath { name: "catalogs[].dir" });

    let error = CatalogSettings::parse(name("catalog"), kind(), PathBuf::from("catalog"), PathBuf::new(), version())
        .expect_err("an empty data directory is not a directory");
    assert_eq!(
        error,
        InvalidCatalogSettings::EmptyPath {
            name: "catalogs[].data_dir"
        }
    );
}

#[test]
fn a_directory_that_does_not_exist_yet_is_accepted() {
    // Deliberate. Checking existence here would be a claim that is already stale by the time
    // the catalog is loaded, and it would make configuration validation depend on the
    // filesystem - which is what makes a settings test need a temporary directory.
    let settings = settings("catalog");
    assert_eq!(settings.name(), &name("catalog"));
    assert_eq!(settings.kind(), CatalogKind::Markdown);
    assert_eq!(settings.dir(), Some(PathBuf::from("/nowhere/catalog").as_path()));
    assert_eq!(settings.data_dir(), Some(PathBuf::from("/nowhere/data").as_path()));
    assert_eq!(settings.version().as_str(), "test-1");
}

#[test]
fn an_empty_catalog_list_is_refused_rather_than_serving_nothing() {
    // A deployment serves at least one metadata source; an empty `catalogs:` is a typo, not a
    // choice, and it is refused at the registry rather than discovering that no bundle loads.
    assert_eq!(
        Catalogs::parse(Vec::new()).expect_err("no catalog is not a deployment"),
        InvalidCatalogSettings::EmptyCatalog
    );
}

#[test]
fn two_catalogs_may_not_share_one_declared_name() {
    // The contribution manifest keys on the declared name, so a duplicate name is two
    // contributors a record cannot tell apart - refused by name, like a duplicated source alias.
    let error = Catalogs::parse(vec![settings("model"), settings("model")])
        .expect_err("the same name twice is two contributors nobody can tell apart");
    assert_eq!(error, InvalidCatalogSettings::DuplicateName { name: name("model") });
    let catalogs = Catalogs::parse(vec![settings("structure"), settings("metrics")])
        .expect("two distinct names are two distinct contributors");
    assert_eq!(catalogs.count(), 2);
}

#[test]
fn a_datahub_entrys_three_required_fields_and_two_optional_bounds_round_trip() {
    let base = settings("metrics");
    assert_eq!(base.endpoint(), None);
    assert_eq!(base.token_file(), None);
    assert_eq!(base.metric_property(), None);
    assert_eq!(base.deadline_seconds(), None);
    assert_eq!(base.max_response_bytes(), None);

    let complete = base
        .clone()
        .with_datahub_reader(
            String::from("https://datahub.example"),
            PathBuf::from("/nowhere/token"),
            String::from("deployment_metric_document"),
        )
        .expect("all three required fields are non-empty")
        .with_datahub_bounds(Some(45), Some(1 << 20));
    assert_eq!(complete.endpoint(), Some("https://datahub.example"));
    assert_eq!(complete.token_file(), Some(Path::new("/nowhere/token")));
    assert_eq!(complete.metric_property(), Some("deployment_metric_document"));
    assert_eq!(complete.deadline_seconds(), Some(45));
    assert_eq!(complete.max_response_bytes(), Some(1 << 20));

    // Declaring neither bound is not a refusal - `None` is what selects the reader's own
    // recommended default, resolved by the composition root rather than by this type.
    let defaulted = base
        .with_datahub_reader(
            String::from("https://datahub.example"),
            PathBuf::from("/nowhere/token"),
            String::from("deployment_metric_document"),
        )
        .expect("all three required fields are non-empty");
    assert_eq!(defaulted.deadline_seconds(), None);
    assert_eq!(defaulted.max_response_bytes(), None);
}

#[test]
fn a_datahub_entry_missing_any_of_the_three_required_fields_is_refused_naming_it() {
    let base = settings("metrics");
    let missing_endpoint = base
        .clone()
        .with_datahub_reader(String::new(), PathBuf::from("/nowhere/token"), String::from("p"))
        .expect_err("an empty endpoint is refused");
    assert_eq!(
        missing_endpoint,
        InvalidCatalogSettings::MissingForDatahub { field: "endpoint" }
    );

    let missing_token_file = base
        .clone()
        .with_datahub_reader(String::from("https://datahub.example"), PathBuf::new(), String::from("p"))
        .expect_err("an empty token_file is refused");
    assert_eq!(
        missing_token_file,
        InvalidCatalogSettings::MissingForDatahub { field: "token_file" }
    );

    let missing_property = base
        .with_datahub_reader(
            String::from("https://datahub.example"),
            PathBuf::from("/nowhere/token"),
            String::new(),
        )
        .expect_err("an empty metric_property is refused");
    assert_eq!(
        missing_property,
        InvalidCatalogSettings::MissingForDatahub {
            field: "metric_property"
        }
    );
}

#[test]
fn an_openmetadata_entrys_two_required_fields_and_two_optional_bounds_round_trip() {
    let base = settings("metrics");
    assert_eq!(base.endpoint(), None);
    assert_eq!(base.token_file(), None);
    assert_eq!(base.deadline_seconds(), None);
    assert_eq!(base.max_response_bytes(), None);

    let complete = base
        .clone()
        .with_http_reader(String::from("https://openmetadata.example"), PathBuf::from("/nowhere/token"))
        .expect("all required fields are non-empty")
        .with_read_bounds(Some(45), Some(1 << 20));
    assert_eq!(complete.endpoint(), Some("https://openmetadata.example"));
    assert_eq!(complete.token_file(), Some(Path::new("/nowhere/token")));
    assert_eq!(complete.deadline_seconds(), Some(45));
    assert_eq!(complete.max_response_bytes(), Some(1 << 20));

    // Declaring neither bound is not a refusal - `None` is what selects the reader's own
    // recommended default, resolved by the composition root rather than by this type.
    let defaulted = base
        .with_http_reader(String::from("https://openmetadata.example"), PathBuf::from("/nowhere/token"))
        .expect("all required fields are non-empty");
    assert_eq!(defaulted.deadline_seconds(), None);
    assert_eq!(defaulted.max_response_bytes(), None);
}

#[test]
fn an_openmetadata_entry_missing_either_required_field_is_refused_naming_it() {
    let base = CatalogSettings::parse(
        name("metrics"),
        CatalogKind::Openmetadata,
        PathBuf::from("/nowhere/catalog"),
        PathBuf::from("/nowhere/data"),
        version(),
    )
    .expect("a declared catalog is a catalog");
    let missing_endpoint = base
        .clone()
        .with_http_reader(String::new(), PathBuf::from("/nowhere/token"))
        .expect_err("an empty endpoint is refused");
    assert_eq!(
        missing_endpoint,
        InvalidCatalogSettings::MissingForReader {
            kind: "openmetadata",
            field: "endpoint"
        }
    );

    let missing_token_file = base
        .with_http_reader(String::from("https://openmetadata.example"), PathBuf::new())
        .expect_err("an empty token_file is refused");
    assert_eq!(
        missing_token_file,
        InvalidCatalogSettings::MissingForReader {
            kind: "openmetadata",
            field: "token_file"
        }
    );
}

/// `#975`: `CatalogSettings::parse` leaves the interval at never (a settings file fills in
/// the default), a positive value round-trips, and every kind takes it - not only `datahub`,
/// unlike the three fields above.
#[test]
fn a_refresh_interval_defaults_to_never_and_round_trips_on_every_kind() {
    for kind in [CatalogKind::Markdown, CatalogKind::Datahub, CatalogKind::Okf] {
        let base = CatalogSettings::parse(
            name("catalog"),
            kind,
            PathBuf::from("/nowhere/catalog"),
            PathBuf::from("/nowhere/data"),
            version(),
        )
        .expect("a directory and a version are a settings");
        assert_eq!(base.refresh_seconds(), None, "{kind:?}");
        let declared = base.with_refresh_seconds(Some(300)).expect("a positive interval is usable");
        assert_eq!(declared.refresh_seconds(), Some(300), "{kind:?}");
    }
}

/// A declared `0` is refused rather than read as "never" or "as fast as possible" - the same
/// reasoning `ReadBounds::parse` (in `sutura-catalog-datahub`) applies to a zero deadline.
#[test]
fn a_zero_refresh_interval_is_refused_naming_the_catalog() {
    let error = settings("catalog")
        .with_refresh_seconds(Some(0))
        .expect_err("a zero interval is not an interval");
    assert_eq!(error, InvalidCatalogSettings::ZeroRefresh { name: name("catalog") });
}

/// `#2071`: absent resolves to the 1 MiB default, and a declared value round-trips on every kind -
/// the same shape `a_refresh_interval_defaults_to_never_and_round_trips_on_every_kind` holds.
#[test]
fn max_definitions_bytes_defaults_to_one_mebibyte_and_round_trips_on_every_kind() {
    for kind in [CatalogKind::Markdown, CatalogKind::Datahub, CatalogKind::Okf] {
        let base = CatalogSettings::parse(
            name("catalog"),
            kind,
            PathBuf::from("/nowhere/catalog"),
            PathBuf::from("/nowhere/data"),
            version(),
        )
        .expect("a directory and a version are a settings");
        assert_eq!(base.max_definitions_bytes().get(), 1024 * 1024, "{kind:?}");
        let declared = base
            .with_max_definitions_bytes(Some(600_000))
            .expect("a bound inside the ceiling is usable");
        assert_eq!(declared.max_definitions_bytes().get(), 600_000, "{kind:?}");
    }
}

/// A declared `0` is refused - a bound of zero can never load a catalog with anything in it.
#[test]
fn a_zero_definitions_bound_is_refused_naming_the_catalog() {
    let error = settings("catalog")
        .with_max_definitions_bytes(Some(0))
        .expect_err("a zero bound is not a bound");
    assert_eq!(
        error,
        InvalidCatalogSettings::InvalidDefinitionsBound {
            name: name("catalog"),
            cause: sutura_domain::catalog::InvalidDefinitionsByteBound::Zero,
        }
    );
}

/// A declared value above `DEFINITIONS_BYTES_CEILING` (16 MiB) is refused, rather than letting a
/// deployment declare away the only thing still bounding the document an agent's prompt and
/// `describe_catalog` carry (`github.com/telekom/sutura#2071`).
#[test]
fn a_definitions_bound_above_the_ceiling_is_refused_naming_the_catalog() {
    let error = settings("catalog")
        .with_max_definitions_bytes(Some(17 * 1024 * 1024))
        .expect_err("a bound above the ceiling is not a bound");
    assert_eq!(
        error,
        InvalidCatalogSettings::InvalidDefinitionsBound {
            name: name("catalog"),
            cause: sutura_domain::catalog::InvalidDefinitionsByteBound::AboveCeiling {
                bytes: 17 * 1024 * 1024,
                ceiling: 16 * 1024 * 1024,
            },
        }
    );
}

pub(super) const ENTRY: &[&str] = &[
    "environment: prod",
    "source_alias: warehouse",
    "live_row_predicate: {column: deleted_at, operator: is_null}",
    "max_dictionary_rows: 1000",
    "max_dictionary_bytes: 1048576",
];

pub(super) const CONNECTION: &[&str] = &[
    "host: 127.0.0.1",
    "port: 5432",
    "database: dictionary",
    "user: reader",
    "password_file: /run/secrets/dictionary",
    "transport_mode: plaintext",
];

pub(super) fn overlay(kind: &str, entry: &[&str], connection: Option<&[&str]>) -> String {
    let indent = |lines: &[&str], by: &str| lines.iter().flat_map(|line| [by, line, "\n"]).collect::<String>();
    let connection = connection.map_or_default(|lines| format!("    connection:\n{}", indent(lines, "      ")));
    format!(
        "security:\n  identity: single-user\n  single_user_because: a test\nsources:\n  warehouse:\n    kind: files\n    data_dir: /srv/data\n    posture: shared-service-user\n\
             catalogs:\n  - name: dict\n    kind: {kind}\n    dir: /nowhere\n    data_dir: /nowhere\n    version: dict-1\n{}{connection}",
        indent(entry, "    ")
    )
}

pub(super) fn load(overlay: &str) -> Result<Settings, crate::settings::SettingsLoadError> {
    Settings::load(&Sources::defaults(Environment::Development).with_overlay(overlay))
}

/// Moved here from `catalog.rs`'s own test module; see the `Weakens-Test:` trailer on the commit
/// that moved it for why the gate cannot pair this body with its pre-move self.
#[test]
fn a_separate_description_mapping_is_parsed_as_prose_only_with_a_closed_predicate() {
    let text = overlay("rdbms", ENTRY, Some(CONNECTION));
    let text = format!(
        "{text}    descriptions_from:\n      - connection: {{host: 127.0.0.1, port: 5432, database: docs, user: reader, password_file: /run/secrets/docs, transport_mode: plaintext}}\n        table: inventory.column_docs\n        key: {{schema: owner, table: object_name, column: field_name}}\n        description: prose\n        live_row_predicate: {{column: deleted, operator: equals, value: 'N'}}\n"
    );
    let settings = load(&text).expect("a checked standalone mapping loads");
    let entry = settings.catalogs().each().next().unwrap();
    let sources = entry.rdbms().unwrap().descriptions_from();
    let crate::catalog::rdbms::DescriptionSource::Table(mapping) = sources.first().unwrap() else {
        panic!("a mapping")
    };
    assert_eq!(mapping.table().to_string(), "inventory.column_docs");
    assert_eq!(
        mapping.predicates().len(),
        1,
        "a singular live_row_predicate is exactly one entry"
    );
    let predicate = mapping.predicates().first().unwrap().predicate();
    assert_eq!(predicate.column().to_string(), "deleted");
    assert_eq!(predicate.operator(), crate::catalog::rdbms::PredicateOperator::Equals);
    assert_eq!(predicate.value(), Some("N"));
    load(&text.replace(
        "live_row_predicate: {column: deleted, operator: equals, value: 'N'}",
        "live_row: \"deleted = 'N'\"",
    ))
    .expect_err("raw SQL is not a declared mapping field");
}

#[test]
fn referenced_catalogs_are_prose_only_and_unrelated_contributors_are_retained() {
    let base = overlay("rdbms", ENTRY, Some(CONNECTION));
    let first = format!("{base}    descriptions_from: [{{catalog: second}}]\n");
    let entry = base.split_once("catalogs:\n").unwrap().1;
    let second = entry.replace("name: dict", "name: second");
    let third = entry.replace("name: dict", "name: third");
    let settings = load(&format!("{first}{second}{third}")).expect("declared prose reference");
    let structural = settings.catalogs().structural_catalogs().unwrap();
    assert_eq!(
        structural.each().map(|entry| entry.name().as_str()).collect::<Vec<_>>(),
        ["dict", "third"]
    );
}
