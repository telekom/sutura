//! What a table path is, and the four things about it that are load-bearing elsewhere.

use crate::model::{
    DatasetName, Hyphens, InvalidIdentifier, InvalidQualifiedTable, ProjectName, Qualification, QualifiedTable, TableName,
    TableQualifier, parse_name,
};

fn table(name: &str) -> TableName {
    TableName::parse(name).expect("a fixture table is a table")
}

#[test]
fn a_bare_name_parses_to_no_qualifier_at_all() {
    // The compatibility claim, at the type. A model naming only a table has to be untouched by this
    // type existing - every existing catalog, every existing golden, and the digest over both.
    let parsed = QualifiedTable::parse("orders").expect("a bare name is a path");
    assert_eq!(parsed.qualifier(), None);
    assert_eq!(parsed.name(), &table("orders"));
    assert_eq!(parsed.qualification(), Qualification::TableOnly);
    assert_eq!(parsed.to_string(), "orders");
    // And the other way in agrees: `From<TableName>` is what every `impl Into<QualifiedTable>` call
    // site in the workspace goes through, so it had better mean the same thing as parsing the name.
    assert_eq!(QualifiedTable::from(table("orders")), parsed);
}

#[test]
fn a_two_part_path_is_a_dataset_and_a_table() {
    let parsed = QualifiedTable::parse("sales.orders").expect("a two-part path is a path");
    let qualifier = parsed.qualifier().expect("a two-part path carries a qualifier");
    assert_eq!(qualifier.project(), None);
    assert_eq!(qualifier.dataset().as_str(), "sales");
    assert_eq!(parsed.name().as_str(), "orders");
    assert_eq!(parsed.qualification(), Qualification::Dataset);
    assert_eq!(parsed.to_string(), "sales.orders");
}

#[test]
fn a_three_part_path_is_a_project_a_dataset_and_a_table() {
    // The hyphen is the point of the fixture rather than decoration: a real project id has one, and
    // it is the reason `ProjectName` is not a seventh `identifier_newtype`.
    let parsed = QualifiedTable::parse("analytics-prod.sales.orders").expect("a three-part path is a path");
    let qualifier = parsed.qualifier().expect("a three-part path carries a qualifier");
    assert_eq!(
        qualifier.project().map(ProjectName::as_str),
        Some("analytics-prod"),
        "the project part is parsed, hyphen and all"
    );
    assert_eq!(qualifier.dataset().as_str(), "sales");
    assert_eq!(parsed.name().as_str(), "orders");
    assert_eq!(parsed.qualification(), Qualification::ProjectAndDataset);
    assert_eq!(parsed.to_string(), "analytics-prod.sales.orders");
}

#[test]
fn a_project_without_a_dataset_is_unrepresentable_rather_than_refused() {
    // There is no check to test, and that is the assertion. `TableQualifier` has no constructor
    // taking a project alone and no optional dataset field, so `project..table` cannot be built - the
    // nearest spelling of it is a path with an empty middle part, which fails on the DATASET being
    // empty rather than on some rule about combinations.
    let refused = QualifiedTable::parse("analytics-prod..orders").expect_err("an empty dataset is not a dataset");
    assert_eq!(
        refused,
        InvalidQualifiedTable::Dataset {
            value: String::from("analytics-prod..orders"),
            cause: InvalidIdentifier::Empty,
        }
    );
    // And the constructor a caller does have makes the dataset mandatory by type, which is what
    // makes the paragraph above true rather than merely currently true.
    let qualifier = TableQualifier::in_project(
        ProjectName::parse("analytics-prod").expect("a project is a project"),
        DatasetName::parse("sales").expect("a dataset is a dataset"),
    );
    assert_eq!(qualifier.dataset().as_str(), "sales");
}

#[test]
fn a_refusal_names_which_part_of_the_path_was_wrong() {
    // The whole reason the error is one variant per position: a message that said only "not a name"
    // makes an author count dots.
    let quoted = QualifiedTable::parse("sa\"les.orders").expect_err("a quote is not a dataset");
    assert_eq!(
        quoted,
        InvalidQualifiedTable::Dataset {
            value: String::from("sa\"les.orders"),
            cause: InvalidIdentifier::IllegalCharacter {
                value: String::from("sa\"les"),
                offending: '"',
            },
        }
    );
    // The cause survives `#[source]`, which is what a reader following the chain gets.
    assert!(
        core::error::Error::source(&quoted).is_some(),
        "the position's own parse failure did not survive #[source]: {quoted:?}"
    );

    let bad_project = QualifiedTable::parse("1st.sales.orders").expect_err("a leading digit is not a project");
    assert!(
        matches!(bad_project, InvalidQualifiedTable::Project { .. }),
        "{bad_project:?}"
    );
    let bad_table = QualifiedTable::parse("sales.or ders").expect_err("a space is not a table");
    assert!(matches!(bad_table, InvalidQualifiedTable::Table { .. }), "{bad_table:?}");
}

#[test]
fn a_path_deeper_than_three_parts_is_refused_rather_than_truncated() {
    // Truncating would pick a table the author did not write, which is the wrong-number failure this
    // whole module exists to close.
    let refused = QualifiedTable::parse("a.b.c.d").expect_err("nothing names a table four deep");
    assert_eq!(
        refused,
        InvalidQualifiedTable::TooManyParts {
            value: String::from("a.b.c.d"),
            parts: 4,
            limit: 3,
        }
    );
}

#[test]
fn a_legacy_domain_scoped_project_is_refused_rather_than_supported() {
    // `example.com:project` is a real spelling for a legacy domain-scoped project, and accepting it
    // would put a `:` and a second `.` back inside one part - which is precisely the defect this
    // module is designed against. So it is refused, and the refusal is the documented outcome rather
    // than an accident of the character set.
    let refused = QualifiedTable::parse("example.com:project.sales.orders").expect_err("a domain-scoped project is refused");
    assert!(matches!(refused, InvalidQualifiedTable::TooManyParts { .. }), "{refused:?}");
    // Even written as one part, the colon is not a project character.
    let colon = ProjectName::parse("com:project").expect_err("a colon is not a project character");
    assert_eq!(
        colon,
        InvalidIdentifier::IllegalCharacter {
            value: String::from("com:project"),
            offending: ':',
        }
    );
}

#[test]
fn no_part_of_a_path_can_carry_a_quote_which_is_what_the_golden_stripping_rests_on() {
    // `AGENTS.md`'s forced-quoting and no-injection rows are asserted over the corpus by stripping
    // quoted spans with a single toggle, and that is sound only while no name can contain a quote
    // character. `ProjectName` is the one name shape here that admits a character `parse_identifier`
    // does not, so it is the one that has to be held to this explicitly.
    for offending in ['"', '\'', '`'] {
        let raw = format!("proj{offending}ect");
        let refused = ProjectName::parse(&raw).expect_err("a quote character is not a project character");
        assert_eq!(
            refused,
            InvalidIdentifier::IllegalCharacter { value: raw, offending },
            "a {offending:?} reached a project name"
        );
    }
    // And a dot, which is what would turn one part back into a path.
    assert_eq!(
        ProjectName::parse("a.b").expect_err("a dot is not a project character"),
        InvalidIdentifier::IllegalCharacter {
            value: String::from("a.b"),
            offending: '.',
        }
    );
}

#[test]
fn a_hyphen_is_a_project_character_and_is_not_a_dataset_or_table_character() {
    // The asymmetry, asserted rather than described. A BigQuery project id carries hyphens; a dataset
    // id is letters, digits and underscore. Widening the wrong one of these is how a name shape stops
    // being what its consumers assume.
    drop(ProjectName::parse("analytics-prod").expect("a hyphen is a project character"));
    assert_eq!(
        DatasetName::parse("sales-eu").expect_err("a hyphen is not a dataset character"),
        InvalidIdentifier::IllegalCharacter {
            value: String::from("sales-eu"),
            offending: '-',
        }
    );
    assert_eq!(
        TableName::parse("or-ders").expect_err("a hyphen is not a table character"),
        InvalidIdentifier::IllegalCharacter {
            value: String::from("or-ders"),
            offending: '-',
        }
    );
}

#[test]
fn a_trailing_hyphen_is_refused_and_only_the_hyphen_admitting_parser_can_say_so() {
    // A name ending in `-` is not a valid project id at any target, so refusing it at load is the
    // difference between an authoring error and a 400 from a service. And the variant is unreachable
    // for the seven ordinary identifier types, which is asserted so the enum does not look like it
    // carries a case nothing produces.
    assert_eq!(
        ProjectName::parse("analytics-").expect_err("a trailing hyphen is refused"),
        InvalidIdentifier::TrailingHyphen {
            value: String::from("analytics-"),
        }
    );
    assert_eq!(
        parse_name("analytics-", Hyphens::Rejected).expect_err("a hyphen is illegal here, not trailing"),
        InvalidIdentifier::IllegalCharacter {
            value: String::from("analytics-"),
            offending: '-',
        },
        "under Hyphens::Rejected a trailing hyphen is refused one guard earlier, so TrailingHyphen is \
         unreachable for an ordinary identifier"
    );
}

#[test]
fn a_serialized_path_deserializes_back() {
    // The `try_from`/`Serialize` asymmetry that shipped here as a bug on `Date`: `serde(try_from)`
    // affects `Deserialize` alone, so a derived `Serialize` would write a struct this type's own
    // `Deserialize` refuses. It matters because the definition digest is taken over the serialized
    // form - it would cover a field layout that appears in no catalog file rather than the text an
    // author wrote.
    //
    // A round trip rather than a comparison against an expected string, so it cannot pass while both
    // halves are wrong in the same way.
    for path in ["orders", "sales.orders", "analytics-prod.sales.orders"] {
        let parsed = QualifiedTable::parse(path).expect("a fixture path is a path");
        let json = serde_json::to_string(&parsed).expect("a path serializes");
        assert_eq!(json, format!("\"{path}\""), "a path serializes as its own text");
        let back: QualifiedTable = serde_json::from_str(&json).expect("what this type wrote, it reads");
        assert_eq!(back, parsed);
    }
}

#[test]
fn deserialization_goes_through_the_constructor() {
    // A catalog document is untrusted input, and this is the one path that carries one.
    let refused: Result<QualifiedTable, serde_json::Error> = serde_json::from_str("\"a.b.c.d\"");
    drop(refused.expect_err("a four-part path must not deserialize"));
    let refused: Result<QualifiedTable, serde_json::Error> = serde_json::from_str("\"sa\\\"les.orders\"");
    drop(refused.expect_err("a quote must not deserialize into a path"));
    let refused: Result<QualifiedTable, serde_json::Error> = serde_json::from_str("\"\"");
    drop(refused.expect_err("empty must not deserialize into a path"));
}

#[test]
fn deeper_is_greater_because_the_comparison_is_what_decides_a_refusal() {
    // The derived `Ord` on an enum is DECLARATION ORDER, and rendering asks
    // `name.qualification() <= dialect.qualification()`. Reordering the declaration would invert
    // every such comparison silently, so the order is pinned here rather than left to be read off the
    // source.
    assert!(Qualification::TableOnly < Qualification::Dataset);
    assert!(Qualification::Dataset < Qualification::ProjectAndDataset);
    // And the spellings, which reach a `GenerateError`'s message.
    assert_eq!(Qualification::TableOnly.as_str(), "table_only");
    assert_eq!(Qualification::Dataset.to_string(), "dataset");
    assert_eq!(Qualification::ProjectAndDataset.as_str(), "project_and_dataset");
}

#[test]
fn surrounding_whitespace_is_not_part_of_a_path() {
    let parsed = QualifiedTable::parse("  sales.orders\n").expect("a trimmed path is a path");
    assert_eq!(parsed.to_string(), "sales.orders");
}
