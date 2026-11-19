//! Complete ordered foreign keys and explicit operator assertions.

#[cfg(feature = "live")]
pub(crate) mod constraints;

pub(crate) mod declared;
pub use declared::{DeclarationError, DeclaredRelationship};

use crate::{DictionaryReader, RdbmsCatalog, RdbmsError, TableAddress, exact_physical_identifier};
use std::collections::{BTreeMap, BTreeSet};
use sutura_domain::catalog::{JoinKey, JoinKeys, Relationship as DomainRelationship};
use sutura_domain::model::{ColumnName, JoinType, ModelName, QualifiedTable, RelationshipName};

const MAX_RELATIONSHIP_PREFIX_LEN: usize = 45;

/// The complete referenced constraint kind; the constructor also checks its whole column list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKeyKind {
    PrimaryKey,
    UniqueConstraint,
}

/// The single-column convenience constructor's evidence vocabulary.
pub type SingleColumnTargetUniqueness = TargetKeyKind;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Evidence {
    Native(TargetKeyKind),
    Declared,
}

/// A whole foreign key, or the operator's explicit many-to-one assertion.
/// Native evidence licenses only this join direction, never a metric's fan-out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    name: Option<String>,
    origin_table: TableAddress,
    target_table: TableAddress,
    first: (String, String),
    rest: Vec<(String, String)>,
    evidence: Option<Evidence>,
}

#[derive(Debug, thiserror::Error)]
pub enum RelationshipError {
    #[error("a relationship needs at least one column pair")]
    Empty,
    #[error("a relationship repeats an origin or target column")]
    DuplicateColumn,
    #[error("a foreign key does not reference the whole primary or unique key")]
    IncompleteTargetKey,
}

impl Relationship {
    /// Single-column convenience; absence of evidence refuses at catalog conversion.
    pub fn new(
        name: Option<String>,
        origin_table: TableAddress,
        origin_column: String,
        target_table: TableAddress,
        target_column: String,
        target_uniqueness: Option<SingleColumnTargetUniqueness>,
    ) -> Self {
        Self {
            name,
            origin_table,
            target_table,
            first: (origin_column, target_column),
            rest: Vec::new(),
            evidence: target_uniqueness.map(Evidence::Native),
        }
    }

    /// Requires every member of the referenced key, including composites. The reader must first
    /// establish enabled, validated, immediate enforcement; a column list cannot establish that.
    pub fn from_foreign_key(
        name: Option<String>,
        origin_table: TableAddress,
        target_table: TableAddress,
        pairs: Vec<(String, String)>,
        target_key: &[String],
        kind: TargetKeyKind,
    ) -> Result<Self, RelationshipError> {
        let relationship = Self::from_pairs(name, origin_table, target_table, pairs, Evidence::Native(kind))?;
        let targets: BTreeSet<_> = relationship.pairs().map(|(_, target)| target).collect();
        let key: BTreeSet<_> = target_key.iter().map(String::as_str).collect();
        if key.len() != target_key.len() || targets != key {
            return Err(RelationshipError::IncompleteTargetKey);
        }
        Ok(relationship)
    }

    /// An operator assertion, checked against visible endpoints and types when applied.
    pub fn declared(
        name: Option<String>,
        origin_table: TableAddress,
        target_table: TableAddress,
        pairs: Vec<(String, String)>,
    ) -> Result<Self, RelationshipError> {
        Self::from_pairs(name, origin_table, target_table, pairs, Evidence::Declared)
    }

    fn from_pairs(
        name: Option<String>,
        origin_table: TableAddress,
        target_table: TableAddress,
        pairs: Vec<(String, String)>,
        evidence: Evidence,
    ) -> Result<Self, RelationshipError> {
        let mut pairs = pairs.into_iter();
        let first = pairs.next().ok_or(RelationshipError::Empty)?;
        let relationship = Self {
            name,
            origin_table,
            target_table,
            first,
            rest: pairs.collect(),
            evidence: Some(evidence),
        };
        let mut origins = BTreeSet::new();
        let mut targets = BTreeSet::new();
        for (origin, target) in relationship.pairs() {
            if !origins.insert(origin) || !targets.insert(target) {
                return Err(RelationshipError::DuplicateColumn);
            }
        }
        Ok(relationship)
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
    pub const fn origin_table(&self) -> &TableAddress {
        &self.origin_table
    }
    pub const fn target_table(&self) -> &TableAddress {
        &self.target_table
    }
    pub fn origin_column(&self) -> &str {
        &self.first.0
    }
    pub fn target_column(&self) -> &str {
        &self.first.1
    }
    pub fn pairs(&self) -> impl Iterator<Item = (&str, &str)> {
        core::iter::once(&self.first)
            .chain(&self.rest)
            .map(|(origin, target)| (origin.as_str(), target.as_str()))
    }
}

/// A foreign key into a [`Relationship`].
///
/// Native relationships require complete referenced-key evidence; declarations are checked
/// against selected types before they enter this conversion. Neither invents metric measures.
pub(crate) fn convert<R>(
    relationship: &Relationship,
    models_by_table: &BTreeMap<QualifiedTable, ModelName>,
) -> Result<DomainRelationship, RdbmsError>
where
    R: DictionaryReader,
{
    if relationship.evidence.is_none() {
        return Err(RdbmsError::TargetUniquenessUnknown {
            table: relationship.target_table().to_string(),
            column: relationship.target_column().to_owned(),
        });
    }
    let origin_table = RdbmsCatalog::<R>::convert_table_address(relationship.origin_table())?;
    let target_table = RdbmsCatalog::<R>::convert_table_address(relationship.target_table())?;
    let origin_model = models_by_table
        .get(&origin_table)
        .cloned()
        .ok_or_else(|| RdbmsError::UnknownRelationshipTable {
            table: origin_table.to_string(),
        })?;
    let target_model = models_by_table
        .get(&target_table)
        .cloned()
        .ok_or_else(|| RdbmsError::UnknownRelationshipTable {
            table: target_table.to_string(),
        })?;
    let mut pairs = Vec::new();
    for (origin, target) in relationship.pairs() {
        let parse = |raw: &str, table: &QualifiedTable| {
            exact_physical_identifier(raw)
                .and_then(ColumnName::parse)
                .map_err(|cause| RdbmsError::ColumnName {
                    table: table.to_string(),
                    column: raw.to_owned(),
                    cause,
                })
        };
        pairs.push(JoinKey::Equal {
            origin: parse(origin, &origin_table)?,
            target: parse(target, &target_table)?,
        });
    }
    let name = relationship_name(relationship, &origin_table, &target_table)?;
    let keys = JoinKeys::of(pairs).map_err(|cause| RdbmsError::RelationshipKeys { cause })?;
    Ok(DomainRelationship::new(
        name,
        origin_model,
        target_model,
        JoinType::ManyToOne,
        keys,
    ))
}

fn relationship_name(
    relationship: &Relationship,
    origin_table: &QualifiedTable,
    target_table: &QualifiedTable,
) -> Result<RelationshipName, RdbmsError> {
    let constraint = relationship.name();
    let mut prefix = if let Some(raw) = constraint {
        RelationshipName::parse(raw)
            .map_err(|cause| RdbmsError::RelationshipName {
                relationship: raw.to_owned(),
                cause,
            })?
            .to_string()
    } else {
        format!("{}_{}_fk", origin_table.name(), target_table.name())
    };
    let origin_table = origin_table.to_string();
    let target_table = target_table.to_string();
    let first = &relationship.first;
    let parts = [
        constraint.unwrap_or(""),
        origin_table.as_str(),
        first.0.as_str(),
        target_table.as_str(),
        first.1.as_str(),
    ];
    let fingerprint = stable_fingerprint(
        parts.into_iter().chain(
            relationship
                .rest
                .iter()
                .flat_map(|(origin, target)| [origin.as_str(), target.as_str()]),
        ),
    );
    prefix.truncate(prefix.len().min(MAX_RELATIONSHIP_PREFIX_LEN));
    let namespaced = format!("{prefix}__{fingerprint:016x}");
    RelationshipName::parse(&namespaced).map_err(|cause| RdbmsError::RelationshipName {
        relationship: namespaced,
        cause,
    })
}

/// A deterministic, non-cryptographic fingerprint for a relationship's canonical identity.
///
/// The `0xff` separator cannot occur in UTF-8, so adjacent fields remain unambiguous. A collision
/// remains a duplicate relationship and is refused by [`sutura_domain::catalog::Definitions::assemble`]; it can never
/// replace an existing one.
fn stable_fingerprint<'a>(parts: impl IntoIterator<Item = &'a str>) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut fingerprint = OFFSET_BASIS;
    for part in parts {
        for byte in part.bytes().chain(core::iter::once(0xff)) {
            fingerprint ^= u64::from(byte);
            fingerprint = fingerprint.wrapping_mul(PRIME);
        }
    }
    fingerprint
}

#[cfg(test)]
mod tests;
