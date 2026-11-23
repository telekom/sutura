//! Complete native constraint rows, decoded under the reader's shared snapshot and budget.

use crate::{Dictionary, RdbmsError, Relationship, SingleColumnTargetUniqueness, TableAddress};
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

pub(crate) const FIELDS: [&str; 13] = [
    "record_kind",
    "constraint_id",
    "constraint_name",
    "schema_name",
    "table_name",
    "origin_column",
    "target_schema",
    "target_table",
    "target_column",
    "target_key_column",
    "ordinal",
    "arity",
    "target_kind",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Fault {
    #[error("a field is absent or malformed")]
    Field,
    #[error("an ordinal lies outside its arity")]
    Ordinal,
    #[error("the record or target kind is not recognised")]
    Kind,
    #[error("rows of one constraint disagree on its header")]
    Header,
    #[error("an ordinal repeats")]
    RepeatedOrdinal,
    #[error("its members do not cover the declared arity")]
    Incomplete,
    #[error("the table already has a primary key")]
    DuplicatePrimaryKey,
}

#[derive(Debug, thiserror::Error)]
#[error("native constraint `{constraint}` is invalid: {fault}")]
pub(crate) struct InvalidConstraint {
    constraint: String,
    pub(crate) fault: Fault,
}
fn invalid(constraint: &str, fault: Fault) -> RdbmsError {
    RdbmsError::Read(Box::new(InvalidConstraint {
        constraint: constraint.to_owned(),
        fault,
    }))
}

#[derive(Debug, PartialEq, Eq)]
struct Header {
    name: String,
    origin: TableAddress,
    target: Option<TableAddress>,
    target_kind: Option<SingleColumnTargetUniqueness>,
    arity: usize,
    declared: bool,
}
type ConstraintMember = (String, Option<String>, Option<String>);

struct Key {
    header: Header,
    members: BTreeMap<usize, ConstraintMember>,
}

#[derive(Default)]
pub(crate) struct Constraints(BTreeMap<String, Key>);
impl Constraints {
    #[cfg(test)]
    pub(crate) fn push(&mut self, values: [Option<String>; 13]) -> Result<(), RdbmsError> {
        self.push_with_catalogs(values, None, None)
    }

    pub(crate) fn push_with_catalogs(
        &mut self,
        values: [Option<String>; 13],
        origin_catalog: Option<String>,
        target_catalog: Option<String>,
    ) -> Result<(), RdbmsError> {
        let [
            kind,
            id,
            name,
            schema,
            table,
            column,
            target_schema,
            target_table,
            target_column,
            key_column,
            ordinal,
            arity,
            target_kind,
        ] = values;
        let label = name.clone().unwrap_or_default();
        let invalid = |fault| invalid(&label, fault);
        let required = |value: Option<String>| value.filter(|value| !value.is_empty()).ok_or_else(|| invalid(Fault::Field));
        let arity = required(arity)?.parse::<usize>().map_err(|_cause| invalid(Fault::Field))?;
        let ordinal = required(ordinal)?.parse::<usize>().map_err(|_cause| invalid(Fault::Field))?;
        if arity == 0 || ordinal == 0 || ordinal > arity {
            return Err(invalid(Fault::Ordinal));
        }
        let kind = required(kind)?;
        let declared = kind == "d";
        let foreign = match kind.as_str() {
            "p" => false,
            "f" | "d" => true,
            _ => return Err(invalid(Fault::Kind)),
        };
        let (target, target_kind) = if foreign {
            let target = TableAddress::new(target_catalog, required(target_schema)?, required(target_table)?);
            let kind = match required(target_kind)?.as_str() {
                "p" if !declared => Some(SingleColumnTargetUniqueness::PrimaryKey),
                "u" if !declared => Some(SingleColumnTargetUniqueness::UniqueConstraint),
                "d" if declared => None,
                _ => return Err(invalid(Fault::Kind)),
            };
            (Some(target), kind)
        } else {
            (None, None)
        };
        if foreign && (target_column.as_ref().is_none_or(String::is_empty) || key_column.as_ref().is_none_or(String::is_empty)) {
            return Err(invalid(Fault::Field));
        }
        let header = Header {
            name: required(name)?,
            origin: TableAddress::new(origin_catalog, required(schema)?, required(table)?),
            target,
            target_kind,
            arity,
            declared,
        };
        let key = match self.0.entry(required(id)?) {
            Entry::Vacant(slot) => slot.insert(Key {
                header,
                members: BTreeMap::new(),
            }),
            Entry::Occupied(slot) => {
                if slot.get().header != header {
                    return Err(invalid(Fault::Header));
                }
                slot.into_mut()
            }
        };
        if key
            .members
            .insert(ordinal, (required(column)?, target_column, key_column))
            .is_some()
        {
            return Err(invalid(Fault::RepeatedOrdinal));
        }
        Ok(())
    }

    pub(crate) fn apply(
        self,
        dictionary: &mut Dictionary,
        on_missing_endpoint: crate::MissingEndpointPolicy,
    ) -> Result<Vec<crate::DeclaredRelationship>, RdbmsError> {
        let mut declared = Vec::new();
        for key in self.0.into_values() {
            let invalid = |fault| invalid(&key.header.name, fault);
            if key.members.len() != key.header.arity || key.members.keys().copied().ne(1..=key.header.arity) {
                return Err(invalid(Fault::Incomplete));
            }
            if let Some(target) = key.header.target {
                let mut pairs = Vec::new();
                let mut target_key = Vec::new();
                for (origin, target, referenced) in key.members.into_values() {
                    pairs.push((origin, target.ok_or_else(|| invalid(Fault::Field))?));
                    target_key.push(referenced.ok_or_else(|| invalid(Fault::Field))?);
                }
                if key.header.declared {
                    declared.push(crate::DeclaredRelationship::with_missing_endpoint_policy(
                        Relationship::declared(Some(key.header.name), key.header.origin, target, pairs)?,
                        None,
                        on_missing_endpoint,
                    ));
                } else {
                    let kind = key.header.target_kind.ok_or_else(|| invalid(Fault::Kind))?;
                    dictionary.relationships.push(Relationship::from_foreign_key(
                        Some(key.header.name),
                        key.header.origin,
                        target,
                        pairs,
                        &target_key,
                        kind,
                    )?);
                }
            } else if let Some(table) = dictionary
                .tables
                .iter_mut()
                .find(|table| table.address() == &key.header.origin)
            {
                if !table.primary_key.is_empty() {
                    return Err(invalid(Fault::DuplicatePrimaryKey));
                }
                table.primary_key = key.members.into_values().map(|(column, _, _)| column).collect();
            }
        }
        Ok(declared)
    }
}

pub(crate) fn documentation_statement(schema: &str, view: &str, oracle: bool, limit: u64) -> String {
    let (schema, view) = if oracle {
        (schema.to_ascii_uppercase(), view.to_ascii_uppercase())
    } else {
        (schema.to_owned(), view.to_owned())
    };
    let source = format!("\"{schema}\".\"{view}\"");
    let column = |name: &str| if oracle { name.to_owned() } else { format!("{name}::text") };
    let ordinal = if oracle {
        "TO_CHAR(ordinal, 'FM99990')"
    } else {
        "ordinal::text"
    };
    let arity = if oracle {
        "TO_CHAR(key_arity, 'FM99990')"
    } else {
        "key_arity::text"
    };
    let columns = [
        ("relationship_name", "constraint_id"),
        ("relationship_name", "constraint_name"),
        ("origin_schema", "schema_name"),
        ("origin_table", "table_name"),
        ("origin_column", "origin_column"),
        ("target_schema", "target_schema"),
        ("target_table", "target_table"),
        ("target_column", "target_column"),
        ("target_column", "target_key_column"),
    ]
    .map(|(name, alias)| format!("{} AS {alias}", column(name)))
    .join(", ");
    let catalogs = ["origin_catalog", "target_catalog"]
        .map(|name| format!("{} AS {name}", column(name)))
        .join(", ");
    if oracle {
        format!(
            "SELECT CASE WHEN r.cardinality = 'many_to_one' THEN 'd' ELSE 'invalid' END AS record_kind, {columns}, {ordinal} AS ordinal, {arity} AS arity, 'd' AS target_kind, {catalogs} FROM {source} r WHERE r.environment = :1 AND r.is_deleted = 0 ORDER BY r.relationship_name, r.ordinal FETCH FIRST {limit} ROWS ONLY"
        )
    } else {
        format!(
            "SELECT (CASE WHEN cardinality = 'many_to_one' THEN 'd' ELSE 'invalid' END)::text AS record_kind, {columns}, {ordinal} AS ordinal, {arity} AS arity, 'd'::text AS target_kind, {catalogs} FROM {source} WHERE environment = $1 AND is_deleted = false ORDER BY relationship_name, ordinal LIMIT {limit}"
        )
    }
}

#[cfg(test)]
mod view_missing_endpoint_tests {
    //! Pins `Constraints::apply`'s `on_missing_endpoint` parameter: a documentation-schema
    //! `relationships_view` row whose declared target table the reading identity's dictionary does
    //! not carry follows the same `MissingEndpointPolicy` a configuration declaration does.

    use super::Constraints;
    use crate::{Dictionary, MissingEndpointPolicy, RdbmsError, Table, TableAddress};

    fn declared_row(target_table: &str) -> [Option<String>; 13] {
        [
            Some(String::from("d")),
            Some(String::from("1")),
            Some(String::from("events_apps")),
            Some(String::from("public")),
            Some(String::from("events")),
            Some(String::from("app_id")),
            Some(String::from("public")),
            Some(target_table.to_owned()),
            Some(String::from("id")),
            Some(String::from("id")),
            Some(String::from("1")),
            Some(String::from("1")),
            Some(String::from("d")),
        ]
    }

    fn address(name: &str) -> TableAddress {
        TableAddress::in_schema(String::from("public"), name.to_owned())
    }

    fn dictionary_with_events_only() -> Dictionary {
        Dictionary::new(
            vec![Table::new(
                "events".to_owned(),
                address("events"),
                vec!["app_id".to_owned()],
                None,
            )],
            Vec::new(),
        )
    }

    #[test]
    fn a_view_row_whose_target_table_is_hidden_refuses_the_load_under_refuse() {
        let mut constraints = Constraints::default();
        constraints.push(declared_row("apps")).unwrap();
        let mut dictionary = dictionary_with_events_only();
        assert!(matches!(
            constraints.apply(&mut dictionary, MissingEndpointPolicy::Refuse),
            Err(RdbmsError::Declaration { .. })
        ));
    }

    /// `MissingEndpointPolicy`'s own `#[default]` - never a caller passing `Refuse` explicitly - is
    /// what an existing deployment with no opinion on the key actually gets.
    #[test]
    fn missing_endpoint_policys_own_default_is_refuse() {
        let mut constraints = Constraints::default();
        constraints.push(declared_row("apps")).unwrap();
        let mut dictionary = dictionary_with_events_only();
        assert!(matches!(
            constraints.apply(&mut dictionary, MissingEndpointPolicy::default()),
            Err(RdbmsError::Declaration { .. })
        ));
    }

    #[test]
    fn a_view_row_whose_target_table_is_hidden_is_omitted_under_the_catalogs_omit_default() {
        let mut constraints = Constraints::default();
        constraints.push(declared_row("apps")).unwrap();
        let mut dictionary = dictionary_with_events_only();
        let resolved = constraints
            .apply(&mut dictionary, MissingEndpointPolicy::Omit)
            .expect("an omit-policy view row never refuses the whole load");
        assert_eq!(
            resolved.len(),
            0,
            "an omitted view-sourced declaration must not be probed for uniqueness"
        );
        assert_eq!(dictionary.relationships(), []);
    }
}
