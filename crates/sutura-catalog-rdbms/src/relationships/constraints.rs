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

#[derive(Debug, thiserror::Error)]
#[error("native constraint metadata is incomplete, repeated or inconsistent")]
struct InvalidConstraint;
fn invalid() -> RdbmsError {
    RdbmsError::Read(Box::new(InvalidConstraint))
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
        let required = |value: Option<String>| value.filter(|value| !value.is_empty()).ok_or_else(invalid);
        let arity = required(arity)?.parse::<usize>().map_err(|_cause| invalid())?;
        let ordinal = required(ordinal)?.parse::<usize>().map_err(|_cause| invalid())?;
        if arity == 0 || ordinal == 0 || ordinal > arity {
            return Err(invalid());
        }
        let kind = required(kind)?;
        let declared = kind == "d";
        let foreign = match kind.as_str() {
            "p" => false,
            "f" | "d" => true,
            _ => return Err(invalid()),
        };
        let (target, target_kind) = if foreign {
            let target = TableAddress::new(target_catalog, required(target_schema)?, required(target_table)?);
            let kind = match required(target_kind)?.as_str() {
                "p" if !declared => Some(SingleColumnTargetUniqueness::PrimaryKey),
                "u" if !declared => Some(SingleColumnTargetUniqueness::UniqueConstraint),
                "d" if declared => None,
                _ => return Err(invalid()),
            };
            (Some(target), kind)
        } else {
            (None, None)
        };
        if foreign && (target_column.as_ref().is_none_or(String::is_empty) || key_column.as_ref().is_none_or(String::is_empty)) {
            return Err(invalid());
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
                    return Err(invalid());
                }
                slot.into_mut()
            }
        };
        if key
            .members
            .insert(ordinal, (required(column)?, target_column, key_column))
            .is_some()
        {
            return Err(invalid());
        }
        Ok(())
    }

    pub(crate) fn apply(self, dictionary: &mut Dictionary) -> Result<Vec<crate::DeclaredRelationship>, RdbmsError> {
        let mut declared = Vec::new();
        for key in self.0.into_values() {
            if key.members.len() != key.header.arity || key.members.keys().copied().ne(1..=key.header.arity) {
                return Err(invalid());
            }
            if let Some(target) = key.header.target {
                let mut pairs = Vec::new();
                let mut target_key = Vec::new();
                for (origin, target, referenced) in key.members.into_values() {
                    pairs.push((origin, target.ok_or_else(invalid)?));
                    target_key.push(referenced.ok_or_else(invalid)?);
                }
                if key.header.declared {
                    declared.push(crate::DeclaredRelationship::new(
                        Relationship::declared(Some(key.header.name), key.header.origin, target, pairs)?,
                        None,
                    ));
                } else {
                    let kind = key.header.target_kind.ok_or_else(invalid)?;
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
                    return Err(invalid());
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
