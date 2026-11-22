//! Operator assertions checked after selection; a probe shares the catalog reader's transaction.
use crate::{Dictionary, DictionaryBounds, RdbmsError, Relationship, TableAddress};
use sutura_domain::catalog::ColumnType;
use sutura_domain::warehouse::schema::{TypeDialect, UnboundedNumber};

/// The dictionary plus the declarations that actually resolved - an endpoint `Omit` dropped is
/// not among them, so a caller probing target uniqueness afterward never selects from what
/// resolution just decided this reading identity cannot see.
pub(crate) type AppliedDeclarations = (Dictionary, Vec<DeclaredRelationship>);

/// What a declared relationship's endpoint not being visible to the reading identity does.
///
/// The database grants decide visibility under `shared-service-user`, so a missing grant and a
/// typo look identical at load time. `Refuse` is the conservative default - the exact behaviour
/// this crate had before the choice existed, so an existing deployment reads unchanged - and
/// `Omit` is the opt-in that lets one declaration serve several environments whose service users
/// hold different grants. Neither widens what a reading identity can see: an omitted relationship
/// just never joins what the grants already hid, and `describe_catalog` never lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MissingEndpointPolicy {
    #[default]
    Refuse,
    Omit,
}

#[derive(Debug, Clone)]
pub struct DeclaredRelationship {
    relationship: Relationship,
    check: Option<DictionaryBounds>,
    on_missing_endpoint: MissingEndpointPolicy,
    checked: std::sync::OnceLock<()>,
}
#[derive(Debug, thiserror::Error)]
pub enum DeclarationError {
    #[error("declared relationship endpoint table {table} is absent or excluded")]
    TableMissing { table: TableAddress },
    #[error("declared relationship endpoint column {table}.{column} is absent on a visible table")]
    ColumnMissing { table: TableAddress, column: String },
    #[error("declared relationship columns {origin}.{origin_column} and {target}.{target_column} lack compatible known types")]
    Types {
        origin: Box<TableAddress>,
        origin_column: String,
        target: Box<TableAddress>,
        target_column: String,
    },
    #[error("target uniqueness check exceeded its returned-row or logical-key-byte budget")]
    ProbeBounds,
    #[error("target uniqueness check found duplicate nonnull full keys")]
    DuplicateTarget,
    #[error("target uniqueness check returned invalid evidence")]
    ProbeEvidence,
}
impl DeclaredRelationship {
    pub const fn new(relationship: Relationship, check: Option<DictionaryBounds>) -> Self {
        Self::with_missing_endpoint_policy(relationship, check, MissingEndpointPolicy::Refuse)
    }
    pub const fn with_missing_endpoint_policy(
        relationship: Relationship,
        check: Option<DictionaryBounds>,
        on_missing_endpoint: MissingEndpointPolicy,
    ) -> Self {
        Self {
            relationship,
            check,
            on_missing_endpoint,
            checked: std::sync::OnceLock::new(),
        }
    }
    pub fn check(&self) -> Option<DictionaryBounds> {
        self.checked.get().is_none().then_some(self.check).flatten()
    }
    pub const fn on_missing_endpoint(&self) -> MissingEndpointPolicy {
        self.on_missing_endpoint
    }
    #[cfg(feature = "live")]
    pub(crate) fn mark_checked(&self) {
        self.checked.get_or_init(|| ());
    }

    /// The cap bounds returned rows and logical key bytes, not the database's scan/sort or time.
    #[cfg(feature = "live")]
    pub(crate) fn probe_statement(&self, oracle: bool) -> Option<String> {
        let bounds = self.check()?;
        let quote = |name: &str| format!("\"{}\"", name.replace('"', "\"\""));
        let table = self.relationship.target_table();
        let table = table
            .catalog()
            .into_iter()
            .chain([table.schema(), table.table()])
            .map(quote)
            .collect::<Vec<_>>()
            .join(".");
        let keys: Vec<_> = self.relationship.pairs().map(|(_, target)| quote(target)).collect();
        let partition = keys.join(", ");
        let nonnull = keys
            .iter()
            .map(|key| format!("{key} IS NOT NULL"))
            .collect::<Vec<_>>()
            .join(" AND ");
        let sizes = keys
            .iter()
            .map(|key| {
                if oracle {
                    format!("LENGTHB(TO_CHAR({key}))")
                } else {
                    format!("octet_length({key}::text)")
                }
            })
            .collect::<Vec<_>>()
            .join(" + ");
        let limit = bounds.max_rows().get().saturating_add(1);
        Some(if oracle {
            format!(
                "SELECT TO_CHAR(COUNT(*) OVER (PARTITION BY {partition}), 'FM99999999999999999990'), TO_CHAR({sizes}, 'FM99999999999999999990') FROM {table} WHERE {nonnull} FETCH FIRST {limit} ROWS ONLY"
            )
        } else {
            format!(
                "SELECT (COUNT(*) OVER (PARTITION BY {partition}))::text AS multiplicity, ({sizes})::text AS key_bytes FROM {table} WHERE {nonnull} LIMIT {limit}"
            )
        })
    }
}

/// Applies every declaration, returning the dictionary and the declarations that actually
/// resolved - an endpoint `Omit` dropped might still be in the input slice, but a probe over it
/// would select from the very table resolution just decided this reading identity cannot see.
pub(crate) fn apply(
    mut dictionary: Dictionary,
    declarations: &[DeclaredRelationship],
    dialect: TypeDialect,
    unbounded_number: Option<UnboundedNumber>,
) -> Result<AppliedDeclarations, RdbmsError> {
    let mut resolved = Vec::new();
    for declaration in declarations {
        if let Some(relationship) = resolve(&dictionary, declaration, dialect, unbounded_number)? {
            dictionary.relationships.push(relationship);
            resolved.push(declaration.clone());
        }
    }
    Ok((dictionary, resolved))
}

/// Checks one declaration against the already-selected dictionary. `Ok(None)` is the `Omit`
/// policy's whole effect: the relationship is simply never pushed, so `describe_catalog` lists
/// nothing for it - never an endpoint identity disclosed past what the dictionary selection
/// already exposed to this reading identity.
fn resolve(
    dictionary: &Dictionary,
    declaration: &DeclaredRelationship,
    dialect: TypeDialect,
    unbounded_number: Option<UnboundedNumber>,
) -> Result<Option<Relationship>, RdbmsError> {
    let relationship = &declaration.relationship;
    for (origin_column, target_column) in relationship.pairs() {
        let find = |address: &TableAddress, column: &str| {
            let table = dictionary
                .tables
                .iter()
                .find(|table| table.address() == address)
                .ok_or_else(|| DeclarationError::TableMissing { table: address.clone() })?;
            if table.columns().iter().any(|name| name == column) {
                Ok(table)
            } else {
                Err(DeclarationError::ColumnMissing {
                    table: address.clone(),
                    column: column.to_owned(),
                })
            }
        };
        let origin = match find(relationship.origin_table(), origin_column) {
            Ok(table) => table,
            Err(cause) => return omit_or_refuse(declaration, cause),
        };
        let target = match find(relationship.target_table(), target_column) {
            Ok(table) => table,
            Err(cause) => return omit_or_refuse(declaration, cause),
        };
        let mapped = |table: &crate::Table, column| {
            let declared = table
                .column_metadata(column)
                .and_then(crate::ColumnMetadata::data_type)
                .and_then(|raw| ColumnType::parse(raw).ok())?;
            dialect.arrow_with_unbounded_number(&declared, unbounded_number)
        };
        let compatible = mapped(origin, origin_column)
            .zip(mapped(target, target_column))
            .is_some_and(|(left, right)| left == right || left.is_integer() && right.is_integer());
        if !compatible {
            // A type mismatch is a wrong declaration, never a missing grant, so it stays a refusal
            // regardless of the endpoint policy.
            return Err(DeclarationError::Types {
                origin: Box::new(relationship.origin_table().clone()),
                origin_column: origin_column.to_owned(),
                target: Box::new(relationship.target_table().clone()),
                target_column: target_column.to_owned(),
            }
            .into());
        }
    }
    Ok(Some(relationship.clone()))
}

fn omit_or_refuse(declaration: &DeclaredRelationship, cause: DeclarationError) -> Result<Option<Relationship>, RdbmsError> {
    // A visible table with no such column is a wrong declaration (a typo), never a missing grant -
    // it stays a refusal under either policy, the same way a type mismatch does.
    if matches!(cause, DeclarationError::ColumnMissing { .. }) {
        return Err(cause.into());
    }
    match declaration.on_missing_endpoint {
        MissingEndpointPolicy::Refuse => Err(cause.into()),
        MissingEndpointPolicy::Omit => {
            tracing::warn!(
                relationship = declaration.relationship.name().unwrap_or("<unnamed>"),
                cause = %cause,
                "omitting declared relationship: endpoint table is absent or excluded from this reading identity's dictionary"
            );
            Ok(None)
        }
    }
}

#[cfg(feature = "live")]
pub(crate) struct Probe {
    bounds: DictionaryBounds,
    rows: u64,
    bytes: u64,
}
#[cfg(feature = "live")]
impl Probe {
    pub(crate) const fn new(bounds: DictionaryBounds) -> Self {
        Self {
            bounds,
            rows: 0,
            bytes: 0,
        }
    }
    pub(crate) fn admit(&mut self, multiplicity: &str, key_bytes: &str) -> Result<(), RdbmsError> {
        let multiplicity = multiplicity
            .parse::<u64>()
            .map_err(|_cause| DeclarationError::ProbeEvidence)?;
        let key_bytes = key_bytes.parse::<u64>().map_err(|_cause| DeclarationError::ProbeEvidence)?;
        self.rows = self.rows.saturating_add(1);
        self.bytes = self
            .bytes
            .saturating_add(key_bytes)
            .saturating_add((multiplicity.to_string().len() + key_bytes.to_string().len()) as u64);
        if self.rows > self.bounds.max_rows().get() || self.bytes > self.bounds.max_bytes().get() {
            return Err(DeclarationError::ProbeBounds.into());
        }
        match multiplicity {
            1 => Ok(()),
            0 => Err(DeclarationError::ProbeEvidence.into()),
            _ => Err(DeclarationError::DuplicateTarget.into()),
        }
    }
}
