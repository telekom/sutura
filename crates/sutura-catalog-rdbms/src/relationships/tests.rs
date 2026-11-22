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
    let (accepted, resolved) = dictionary()
        .apply_relationship_declarations(&[declaration("app_id")], TypeDialect::Postgres, None)
        .unwrap();
    assert_eq!(accepted.relationships().len(), 1);
    assert_eq!(resolved.len(), 1);
    assert!(matches!(
        dictionary().apply_relationship_declarations(&[declaration("missing")], TypeDialect::Postgres, None),
        Err(RdbmsError::Declaration {
            cause: DeclarationError::ColumnMissing { .. }
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

#[test]
fn omit_policy_drops_a_declaration_whose_endpoint_table_is_invisible_instead_of_refusing_the_load() {
    use crate::MissingEndpointPolicy;
    let declaration = DeclaredRelationship::with_missing_endpoint_policy(
        Relationship::declared(
            None,
            address("events"),
            address("hidden_table"),
            vec![(String::from("app_id"), String::from("id"))],
        )
        .unwrap(),
        None,
        MissingEndpointPolicy::Omit,
    );
    let (loaded, resolved) = dictionary()
        .apply_relationship_declarations(&[declaration], TypeDialect::Postgres, None)
        .expect("an omit-policy declaration never refuses the whole load");
    assert_eq!(loaded.relationships(), []);
    assert_eq!(
        resolved.len(),
        0,
        "an omitted declaration must not be returned for a uniqueness probe"
    );
}

#[test]
fn omit_policy_still_refuses_a_typo_d_column_on_a_visible_table() {
    use crate::MissingEndpointPolicy;
    let declaration = DeclaredRelationship::with_missing_endpoint_policy(
        Relationship::declared(
            None,
            address("events"),
            address("apps"),
            vec![(String::from("app_id"), String::from("typo_d_column"))],
        )
        .unwrap(),
        None,
        MissingEndpointPolicy::Omit,
    );
    assert!(matches!(
        dictionary().apply_relationship_declarations(&[declaration], TypeDialect::Postgres, None),
        Err(RdbmsError::Declaration {
            cause: DeclarationError::ColumnMissing { .. }
        })
    ));
}

/// A minimal capturing fake for [`tracing::Subscriber`]: records each event's fields as one
/// `key=debug` string per field, joined per event, so a test can assert on warning content
/// without a mocked HTTP sink or an external test-capture crate.
#[derive(Clone, Default)]
struct CapturedEvents(std::sync::Arc<parking_lot::Mutex<Vec<String>>>);

impl CapturedEvents {
    fn joined(&self) -> String {
        self.0.lock().join("\n")
    }
}

impl tracing::Subscriber for CapturedEvents {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Fields(Vec<String>);
        impl tracing::field::Visit for Fields {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                self.0.push(format!("{}={value:?}", field.name()));
            }
        }
        let mut fields = Fields(Vec::new());
        event.record(&mut fields);
        self.0.lock().push(fields.0.join(" "));
    }
    fn enter(&self, _span: &tracing::span::Id) {}
    fn exit(&self, _span: &tracing::span::Id) {}
}

#[test]
fn omitting_a_declaration_logs_the_relationship_name_and_the_missing_endpoint() {
    use crate::MissingEndpointPolicy;
    let declaration = DeclaredRelationship::with_missing_endpoint_policy(
        Relationship::declared(
            Some(String::from("events_hidden")),
            address("events"),
            address("hidden_table"),
            vec![(String::from("app_id"), String::from("id"))],
        )
        .unwrap(),
        None,
        MissingEndpointPolicy::Omit,
    );
    let captured = CapturedEvents::default();
    tracing::subscriber::with_default(captured.clone(), || {
        dictionary()
            .apply_relationship_declarations(&[declaration], TypeDialect::Postgres, None)
            .expect("an omit-policy declaration never refuses the whole load")
    });
    let logs = captured.joined();
    assert!(logs.contains("events_hidden"), "{logs}");
    assert!(logs.contains("hidden_table"), "{logs}");
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
    use super::constraints::{Constraints, Fault, InvalidConstraint};
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
    let fault = |error: RdbmsError| match error {
        RdbmsError::Read(cause) => {
            let cause = cause.downcast::<InvalidConstraint>().expect("a constraint refusal");
            assert!(cause.to_string().contains("apps_pk"), "{cause}");
            cause.fault
        }
        other => panic!("not a constraint refusal: {other}"),
    };
    let mut partial = Constraints::default();
    partial.push(row("1", "tenant")).unwrap();
    let mut keyless = Dictionary::new(vec![table("apps", &[("tenant", "bigint"), ("id", "bigint")])], Vec::new());
    assert_eq!(fault(partial.apply(&mut keyless).unwrap_err()), Fault::Incomplete);
    let mut doubled = Constraints::default();
    doubled.push(row("1", "tenant")).unwrap();
    doubled.push(row("2", "id")).unwrap();
    assert_eq!(
        fault(doubled.apply(&mut dictionary()).unwrap_err()),
        Fault::DuplicatePrimaryKey
    );
    let mut repeated = Constraints::default();
    repeated.push(row("1", "tenant")).unwrap();
    assert_eq!(fault(repeated.push(row("1", "id")).unwrap_err()), Fault::RepeatedOrdinal);
    let mut mixed = Constraints::default();
    mixed.push(row("1", "tenant")).unwrap();
    let mut changed = row("2", "id");
    changed[4] = Some(String::from("another_table"));
    assert_eq!(fault(mixed.push(changed).unwrap_err()), Fault::Header);
}
