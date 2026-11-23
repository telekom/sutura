//! Closed many-to-one operator assertions with optional bounded boot verification.
use super::InvalidRdbmsCatalog;
use std::collections::BTreeSet;
use std::num::NonZeroU64;
use sutura_domain::model::{ColumnName, QualifiedTable, RelationshipName};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawRelationship {
    name: Option<String>,
    from: RawEndpoint,
    to: RawEndpoint,
    cardinality: String,
    check_target_uniqueness: Option<RawCheck>,
    on_missing_endpoint: Option<String>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEndpoint {
    table: String,
    columns: Vec<String>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCheck {
    max_rows: u64,
    max_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredRelationship {
    name: Option<RelationshipName>,
    from: RelationshipEndpoint,
    to: RelationshipEndpoint,
    check: Option<UniquenessCheck>,
    on_missing_endpoint: MissingEndpointPolicy,
}

/// What a declared relationship's endpoint not being visible to the reading identity does.
///
/// `Refuse` is the default: the exact behaviour before this field existed, so a written catalog
/// with no `on_missing_endpoint` key reads unchanged. `Omit` is the opt-in `telekom/sutura#2039`
/// asks for, per relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MissingEndpointPolicy {
    #[default]
    Refuse,
    Omit,
}
impl MissingEndpointPolicy {
    const ALL: [Self; 2] = [Self::Refuse, Self::Omit];
    pub(crate) fn parse(written: &str) -> Option<Self> {
        let written = written.trim();
        Self::ALL.into_iter().find(|policy| policy.as_str() == written)
    }
    const fn as_str(self) -> &'static str {
        match self {
            Self::Refuse => "refuse",
            Self::Omit => "omit",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationshipEndpoint {
    table: QualifiedTable,
    columns: Vec<ColumnName>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UniquenessCheck {
    max_rows: NonZeroU64,
    max_bytes: NonZeroU64,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvalidRelationship {
    #[error("relationships needs at most 64 many_to_one declarations with equal nonempty endpoint columns (at most 32)")]
    Shape,
    #[error("relationship endpoints must use exact schema-qualified tables and distinct exact columns")]
    Endpoint,
    #[error("relationship name is not usable")]
    Name,
    #[error("target uniqueness check requires nonzero max_rows and max_bytes")]
    Bounds,
    #[error("relationship on_missing_endpoint must be one of refuse, omit")]
    MissingEndpointPolicy,
}
const fn invalid(cause: InvalidRelationship) -> InvalidRdbmsCatalog {
    InvalidRdbmsCatalog::Relationship { cause }
}
fn endpoint(raw: &RawEndpoint) -> Result<RelationshipEndpoint, InvalidRdbmsCatalog> {
    let exact = |raw: &str| !raw.chars().any(char::is_whitespace);
    if !exact(&raw.table) {
        return Err(invalid(InvalidRelationship::Endpoint));
    }
    let table = QualifiedTable::parse(&raw.table).map_err(|_cause| invalid(InvalidRelationship::Endpoint))?;
    if table.qualifier().is_none() {
        return Err(invalid(InvalidRelationship::Endpoint));
    }
    let mut seen = BTreeSet::new();
    let columns = raw
        .columns
        .iter()
        .map(|column| {
            if !exact(column) || !seen.insert(column) {
                return Err(invalid(InvalidRelationship::Endpoint));
            }
            ColumnName::parse(column).map_err(|_cause| invalid(InvalidRelationship::Endpoint))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RelationshipEndpoint { table, columns })
}
pub(crate) fn parse(
    raw: &[RawRelationship],
    default_on_missing_endpoint: MissingEndpointPolicy,
) -> Result<Vec<DeclaredRelationship>, InvalidRdbmsCatalog> {
    if raw.len() > 64 {
        return Err(invalid(InvalidRelationship::Shape));
    }
    raw.iter()
        .map(|raw| {
            if raw.cardinality != "many_to_one"
                || raw.from.columns.is_empty()
                || raw.from.columns.len() > 32
                || raw.from.columns.len() != raw.to.columns.len()
            {
                return Err(invalid(InvalidRelationship::Shape));
            }
            Ok(DeclaredRelationship {
                name: raw
                    .name
                    .as_deref()
                    .map(RelationshipName::parse)
                    .transpose()
                    .map_err(|_cause| invalid(InvalidRelationship::Name))?,
                from: endpoint(&raw.from)?,
                to: endpoint(&raw.to)?,
                check: raw
                    .check_target_uniqueness
                    .as_ref()
                    .map(|check| {
                        Ok(UniquenessCheck {
                            max_rows: NonZeroU64::new(check.max_rows).ok_or_else(|| invalid(InvalidRelationship::Bounds))?,
                            max_bytes: NonZeroU64::new(check.max_bytes).ok_or_else(|| invalid(InvalidRelationship::Bounds))?,
                        })
                    })
                    .transpose()?,
                on_missing_endpoint: raw
                    .on_missing_endpoint
                    .as_deref()
                    .map(|written| {
                        MissingEndpointPolicy::parse(written).ok_or_else(|| invalid(InvalidRelationship::MissingEndpointPolicy))
                    })
                    .transpose()?
                    .unwrap_or(default_on_missing_endpoint),
            })
        })
        .collect()
}
impl DeclaredRelationship {
    pub const fn name(&self) -> Option<&RelationshipName> {
        self.name.as_ref()
    }
    pub const fn from(&self) -> &RelationshipEndpoint {
        &self.from
    }
    pub const fn to(&self) -> &RelationshipEndpoint {
        &self.to
    }
    pub const fn check(&self) -> Option<UniquenessCheck> {
        self.check
    }
    pub const fn on_missing_endpoint(&self) -> MissingEndpointPolicy {
        self.on_missing_endpoint
    }
}
impl RelationshipEndpoint {
    pub const fn table(&self) -> &QualifiedTable {
        &self.table
    }
    pub fn columns(&self) -> &[ColumnName] {
        &self.columns
    }
}
impl UniquenessCheck {
    pub const fn max_rows(self) -> NonZeroU64 {
        self.max_rows
    }
    pub const fn max_bytes(self) -> NonZeroU64 {
        self.max_bytes
    }
}
