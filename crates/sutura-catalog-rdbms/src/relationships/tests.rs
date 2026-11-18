use super::*;
use crate::{ColumnMetadata, DeclarationError, DeclaredRelationship, Dictionary, Table};
use sutura_domain::catalog::{DictionaryInclude, DictionaryPattern, DictionarySelection};
use sutura_domain::warehouse::schema::TypeDialect;

fn address(name: &str) -> TableAddress {
    TableAddress::in_schema(String::from("public"), name.to_owned())
}
fn table(name: &str, types: &[(&str, &str)]) -> Table {
    Table::new(
        name.to_owned(),
        address(name),
        types.iter().map(|(column, _)| (*column).to_owned()).collect(),
        None,
    )
    .with_column_metadata(
        types
            .iter()
            .map(|(column, kind)| ((*column).to_owned(), ColumnMetadata::new(Some((*kind).to_owned()), None))),
    )
}
fn key(pairs: &[(&str, &str)], target: &[&str]) -> Result<Relationship, RelationshipError> {
    Relationship::from_foreign_key(
        Some(String::from("events_apps")),
        address("events"),
        address("apps"),
        pairs
            .iter()
            .map(|(origin, target)| ((*origin).to_owned(), (*target).to_owned()))
            .collect(),
        &target.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>(),
        SingleColumnTargetUniqueness::PrimaryKey,
    )
}
fn dictionary() -> Dictionary {
    Dictionary::new(
        vec![
            table("events", &[("tenant", "bigint"), ("app_id", "integer")]),
            table("apps", &[("tenant", "bigint"), ("id", "bigint")])
                .with_primary_key(vec![String::from("tenant"), String::from("id")]),
        ],
        Vec::new(),
    )
}

#[test]
fn complete_composite_foreign_keys_keep_every_ordered_pair_and_refuse_partial_uniqueness() {
    struct Recorded(Dictionary);
    impl DictionaryReader for Recorded {
        fn read_dictionary(&self) -> Result<Dictionary, RdbmsError> {
            Ok(self.0.clone())
        }
    }
    use sutura_domain::pinned::SemanticCatalog as _;
    let relation = key(&[("app_id", "id"), ("tenant", "tenant")], &["tenant", "id"]).unwrap();
    let mut dictionary = dictionary();
    dictionary.relationships.push(relation);
    let bundle = RdbmsCatalog::new(
        sutura_domain::model::SourceName::parse("dictionary").unwrap(),
        sutura_domain::pinned::DefinitionVersion::parse("test").unwrap(),
        Recorded(dictionary),
    )
    .load()
    .unwrap();
    let relation = bundle.definitions().relationships().values().next().unwrap();
    assert_eq!(
        relation
            .keys()
            .iter()
            .map(|key| (key.origin().as_str(), key.target().as_str()))
            .collect::<Vec<_>>(),
        [("app_id", "id"), ("tenant", "tenant")]
    );
    assert_eq!(relation.join_type(), JoinType::ManyToOne);
    assert!(matches!(
        key(&[("app_id", "id")], &["tenant", "id"]),
        Err(RelationshipError::IncompleteTargetKey)
    ));
    assert!(matches!(
        key(&[("app_id", "id"), ("app_id", "tenant")], &["tenant", "id"]),
        Err(RelationshipError::DuplicateColumn)
    ));
}

#[test]
fn selection_drops_whole_composite_keys_and_links_when_one_member_is_excluded() {
    let mut dictionary = dictionary();
    dictionary
        .relationships
        .push(key(&[("tenant", "tenant"), ("app_id", "id")], &["tenant", "id"]).unwrap());
    let pattern = |raw| DictionaryPattern::parse(raw).unwrap();
    let selection = DictionarySelection::parse(
        vec![DictionaryInclude::new(pattern("public"), pattern("*"))],
        Vec::new(),
        vec![pattern("tenant")],
        Vec::new(),
    )
    .unwrap();
    let selected = dictionary.native(&selection, TypeDialect::Postgres).unwrap();
    assert_eq!(selected.tables().len(), 2);
    assert!(selected.tables().iter().all(|table| table.primary_key().is_empty()));
    assert_eq!(selected.relationships(), []);
}

#[test]
fn declared_links_require_visible_columns_and_compatible_known_types() {
    let declaration = |origin| {
        DeclaredRelationship::new(
            Relationship::declared(
                None,
                address("events"),
                address("apps"),
                vec![(String::from(origin), String::from("id"))],
            )
            .unwrap(),
            None,
        )
    };
    let accepted = dictionary()
        .apply_relationship_declarations(&[declaration("app_id")], TypeDialect::Postgres, None)
        .unwrap();
    assert_eq!(accepted.relationships().len(), 1);
    assert!(matches!(
        dictionary().apply_relationship_declarations(&[declaration("missing")], TypeDialect::Postgres, None),
        Err(RdbmsError::Declaration {
            cause: DeclarationError::Endpoint { .. }
        })
    ));
    let mut incompatible = dictionary();
    incompatible.tables[0] = table("events", &[("app_id", "text")]);
    assert!(matches!(
        incompatible.apply_relationship_declarations(&[declaration("app_id")], TypeDialect::Postgres, None),
        Err(RdbmsError::Declaration {
            cause: DeclarationError::Types { .. }
        })
    ));
    let mut unknown = dictionary();
    unknown.tables[0] = table("events", &[("app_id", "unknown_type")]);
    assert!(matches!(
        unknown.apply_relationship_declarations(&[declaration("app_id")], TypeDialect::Postgres, None),
        Err(RdbmsError::Declaration {
            cause: DeclarationError::Types { .. }
        })
    ));
}

#[cfg(feature = "live")]
#[test]
fn boot_uniqueness_evidence_refuses_duplicates_and_a_row_past_either_bound() {
    use super::declared::Probe;
    use crate::DictionaryBounds;
    use std::num::NonZeroU64;
    let bounds = |rows, bytes| DictionaryBounds::new(NonZeroU64::new(rows).unwrap(), NonZeroU64::new(bytes).unwrap());
    let mut probe = Probe::new(bounds(2, 100));
    probe.admit("1", "8").expect("a unique row fits the check budget");
    probe.admit("1", "8").expect("a unique row fits the check budget");
    assert!(matches!(
        probe.admit("1", "8"),
        Err(RdbmsError::Declaration {
            cause: DeclarationError::ProbeBounds
        })
    ));
    assert!(matches!(
        Probe::new(bounds(2, 100)).admit("2", "8"),
        Err(RdbmsError::Declaration {
            cause: DeclarationError::DuplicateTarget
        })
    ));
    assert!(matches!(
        Probe::new(bounds(2, 1)).admit("1", "8"),
        Err(RdbmsError::Declaration {
            cause: DeclarationError::ProbeBounds
        })
    ));
}

#[cfg(feature = "live")]
#[test]
fn native_constraint_assembly_refuses_incomplete_and_inconsistent_ordinals() {
    use super::constraints::Constraints;
    let row = |ordinal: &str, column: &str| {
        [
            Some("p"),
            Some("1"),
            Some("apps_pk"),
            Some("public"),
            Some("apps"),
            Some(column),
            None,
            None,
            None,
            None,
            Some(ordinal),
            Some("2"),
            None,
        ]
        .map(|value| value.map(str::to_owned))
    };
    let mut complete = Constraints::default();
    complete.push(row("2", "id")).unwrap();
    complete.push(row("1", "tenant")).unwrap();
    let mut actual = Dictionary::new(vec![table("apps", &[("tenant", "bigint"), ("id", "bigint")])], Vec::new());
    complete.apply(&mut actual).unwrap();
    assert_eq!(actual.tables()[0].primary_key(), ["tenant", "id"]);
    let mut partial = Constraints::default();
    partial.push(row("1", "tenant")).unwrap();
    partial
        .apply(&mut dictionary())
        .expect_err("an incomplete composite key refuses");
    let mut repeated = Constraints::default();
    repeated.push(row("1", "tenant")).unwrap();
    repeated.push(row("1", "id")).expect_err("a repeated ordinal refuses");
    let mut mixed = Constraints::default();
    mixed.push(row("1", "tenant")).unwrap();
    let mut changed = row("2", "id");
    changed[4] = Some(String::from("another_table"));
    mixed.push(changed).expect_err("one constraint id cannot change its table");
}
