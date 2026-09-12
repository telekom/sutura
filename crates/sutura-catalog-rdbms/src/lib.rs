//! A [`SemanticCatalog`] over an RDBMS dictionary - the narrowest declaration, and the one that
//! needs no service.
//!
//! `docs/adr/0011-pluggable-by-declaration.md` defines the role this conversion implements. A
//! database dictionary is mainly DDL and comments: the tables, the columns, their constraints, and
//! table prose. It is not a
//! semantic layer and does not pretend to be one - which is the whole point of a **declaring**
//! adapter. It says which kinds it provides and which it does not, and it is measured against that
//! declaration rather than against the golden adapters' oracle.
//!
//! This crate implements the dictionary conversion from `github.com/telekom/sutura#151`. A live
//! reader remains outside it; the runtime prompt derives the zero-metric physical-schema guidance
//! from the pinned bundle rather than coupling the application to this adapter.
//!
//! # What a real dictionary yields
//!
//! ADR 0016's method, applied here: read a real dictionary before writing the adapter. A throwaway
//! reader, measured once against a two-table Postgres 18 schema (two tables, a primary key each, one
//! foreign key, and table comments), reported the following:
//!
//! | What | Count |
//! | --- | --- |
//! | tables | 2 |
//! | columns | 6 |
//! | tables carrying a comment | 2 |
//! | foreign keys | 1 |
//! | primary/unique constraints | 2 |
//!
//! Three findings, and two of them are the declaration's content:
//!
//! 1. **A dictionary yields structure and prose, and nothing else.** Tables and columns are the
//!    [`Structure`](sutura_domain::capabilities::DefinitionKind::Structure) half; table comments are
//!    the [`Descriptions`](sutura_domain::capabilities::DefinitionKind::Descriptions)
//!    half. A dictionary carries **no measure, no grain, no definitional filter, no value allowlist
//!    and no anchor** - those are declared by a human in a semantic layer, which is what ADR 0011's
//!    table says ("certified metrics, measures, grains, allowed values: **no** - a human declares
//!    those elsewhere").
//! 2. **A foreign key carries no metric cardinality.** It names the source and target columns, so
//!    this adapter declares the `Cardinality` *capability* absent: a dictionary carries no metric to
//!    reach a dimension `via` a relationship.
//! 3. **A single-column primary or unique key is evidence, and only the safe direction.** ADR
//!    0011's "part worth having this connector for" is the one-direction uniqueness argument: a
//!    reader must supply a [`SingleColumnTargetUniqueness`] before the foreign key maps to
//!    [`JoinType::ManyToOne`]. Membership in a composite constraint is not evidence that one column
//!    is unique. Without the single-column evidence, loading refuses rather than asserting the
//!    relationship. The variant records what the reader found; this converter does not re-derive
//!    it from the constraint itself.
//!
//! # The declaration, and what it means for the bundle
//!
//! [`SemanticCatalog::capabilities`] provides `Structure` and may provide `Descriptions` and
//! `Relationships`. A sparse dictionary - structure with no comments or foreign keys - is therefore
//! faithful without making structure optional. What is declared is nothing more: no `Cardinality`
//! (a foreign key vouches for no metric fan-out), no `Metrics`, no
//! `Grains`, no `RequiredFilters`, no `AllowedValues`, no `Anchors`, and an empty knowledge half.
//! A bundle from this source therefore **loads, pins and validates with zero metrics**, and answers
//! no certified question - which is issue #115's shape and the whole reason the declaration exists:
//! a deployment whose whole model is a physical schema must not be told it has metrics it does not.
//!
//! # What is built here, and what is NOT
//!
//! This crate contains the conversion [`RdbmsCatalog`] applies to dictionary records, and it is
//! tested against a fake reader that serves a recorded dictionary - the port gets a fake,
//! not mocked SQL (`github.com/telekom/sutura#151`'s thing 4). What it does not contain is a
//! database client in the library closure: [`DictionaryReader`] is the seam a real reader over a
//! Postgres socket will implement, and the only implementor today is the recorded fixture source in
//! [`fixture`]. A production reader is outside this crate's current scope.
//!
//! **And nothing serves it:** no composition root links this crate (its only dependant is
//! `sutura-app`, as a dev-dependency), so this is a registered, declaring catalog rather than a
//! served one - exactly the state `sutura-catalog-datahub` holds, which is the precedent copied.

pub mod fixture;

use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{Definitions, Description, Model, Relationship as DomainRelationship};
use sutura_domain::knowledge::{Knowledge, KnowledgeCapabilities};
use sutura_domain::model::{
    ColumnName, DatasetName, InvalidIdentifier, JoinType, ModelName, ProjectName, QualifiedTable, RelationshipName, SourceName,
    TableName, TableQualifier,
};
use sutura_domain::pinned::{
    CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog,
};

/// The two halves of a bundle, read and checked but not yet pinned.
type Content = (Definitions, Knowledge);
/// One parsed physical address and the semantic model built over it.
type ConvertedModel = (QualifiedTable, Model);

/// Leaves room for `__` and a 16-character fingerprint under the 63-character name limit.
const MAX_RELATIONSHIP_PREFIX_LEN: usize = 45;

/// Refuses whitespace before the shared parser can trim it from a physical identifier.
fn exact_physical_identifier(raw: &str) -> Result<&str, InvalidIdentifier> {
    if let Some(offending) = raw.chars().find(|character| character.is_whitespace()) {
        return Err(InvalidIdentifier::IllegalCharacter {
            value: String::from(raw),
            offending,
        });
    }
    Ok(raw)
}

/// Where a dictionary's records come from.
///
/// **The fake seam.** Everything above this trait is decided and tested against a recorded
/// dictionary served by [`fixture::FixtureReader`]; a real implementor speaks to a Postgres socket,
/// reads `information_schema` / `pg_catalog`, decodes into [`Dictionary`], and maps its own failures
/// into [`RdbmsError::Read`]. A port rather than a method on [`RdbmsCatalog`] for the same reason
/// the warehouse port exists: a catalog that could be swapped for a live source without the
/// conversion changing is the point.
///
/// A [`Relationship`] carries exactly one origin column and one target column, so a composite
/// (multi-column) foreign key is not representable in it. A real implementor must therefore either
/// refuse the whole read or omit that one foreign key when it encounters one, and must document
/// which of the two it does - the conversion below never sees a foreign key that a reader omitted,
/// so it cannot enforce or even detect either choice.
pub trait DictionaryReader {
    /// Reads the deployment's dictionary, in whatever shape this crate defines.
    fn read_dictionary(&self) -> Result<Dictionary, RdbmsError>;
}

/// Why a dictionary could not be read as a catalog.
///
/// Every variant is a typed contract rather than a message; the message is for a human and the
/// variant is what a caller can branch on. The mapping variants carry the table or column they were
/// refused on, because a dictionary is many rows and "invalid identifier" with no table name sends
/// a reader back to all of them.
#[derive(Debug, thiserror::Error)]
pub enum RdbmsError {
    /// The reader could not fetch the dictionary.
    #[error("reading the dictionary failed: {0}")]
    Read(#[source] Box<dyn std::error::Error + Send + Sync>),
    /// The reader returned no table, so this bundle cannot honour its required `Structure` claim.
    #[error("the dictionary contains no visible table")]
    NoVisibleTables,
    /// A table's semantic name did not parse as a model name.
    #[error("model {model} for table {table} is not a usable model name: {cause}")]
    ModelName {
        table: String,
        model: String,
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    /// A table's catalog did not parse as the top part of a qualified table name.
    #[error("catalog {catalog} of table {table} is not usable: {cause}")]
    CatalogName {
        table: String,
        catalog: String,
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    /// A table's schema did not parse as the middle part of a qualified table name.
    #[error("schema {schema} of table {table} is not usable: {cause}")]
    SchemaName {
        table: String,
        schema: String,
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    /// A physical table's own name did not parse.
    #[error("physical table {table} is not usable: {cause}")]
    TableName {
        table: String,
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    /// Two semantic models named the same physical table.
    #[error("physical table {table} is declared twice")]
    DuplicateTable { table: String },
    /// A foreign key named a physical table absent from the dictionary.
    #[error("foreign key endpoint {table} is not a table in the dictionary")]
    UnknownRelationshipTable { table: String },
    /// A column's name did not parse.
    #[error("column {column} on table {table} is not a usable column name: {cause}")]
    ColumnName {
        table: String,
        column: String,
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    /// A foreign key's name did not parse.
    #[error("foreign key {relationship} is not a usable relationship name: {cause}")]
    RelationshipName {
        relationship: String,
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    /// A table description did not pass the authored-prose rule.
    #[error("the description of table {table} is not usable: {cause}")]
    Description {
        table: String,
        #[source]
        cause: sutura_domain::catalog::InvalidDescription,
    },
    /// The referenced column had no single-column primary or unique-key evidence.
    #[error("referenced column {table}.{column} has no single-column primary or unique-key evidence")]
    TargetUniquenessUnknown { table: String, column: String },
    /// The assembled definitions did not hold together.
    #[error("the dictionary definitions do not hold together: {cause}")]
    Inconsistent {
        #[source]
        cause: sutura_domain::catalog::InconsistentDefinitions,
    },
    /// Pinning failed.
    #[error("the dictionary bundle could not be pinned: {cause}")]
    Digest {
        #[source]
        cause: sutura_domain::definitions::NotDigestible,
    },
}

/// A catalog read from an RDBMS dictionary.
///
/// Generic in its reader rather than holding a boxed one, matching the warehouse adapters: there is
/// one per composition and a generic keeps the chosen reader visible. It carries the declared name
/// and version it is recorded under, the same way the other catalog adapters carry their own.
#[derive(Debug, Clone)]
pub struct RdbmsCatalog<R> {
    name: SourceName,
    version: DefinitionVersion,
    reader: R,
}

impl<R> RdbmsCatalog<R> {
    /// Opens a catalog over a dictionary reader.
    pub const fn new(name: SourceName, version: DefinitionVersion, reader: R) -> Self {
        Self { name, version, reader }
    }
}

impl<R: DictionaryReader> RdbmsCatalog<R> {
    /// The conversion: a [`Dictionary`] into the two halves of a bundle.
    ///
    /// Kept as a method that takes a [`Dictionary`] so the whole decision half is testable against
    /// a reader that returns one, and a reader's only job is to produce a dictionary.
    fn assemble(&self, dictionary: &Dictionary) -> Result<Content, RdbmsError> {
        if dictionary.tables.is_empty() {
            return Err(RdbmsError::NoVisibleTables);
        }
        let mut models = Vec::with_capacity(dictionary.tables.len());
        let mut models_by_table = BTreeMap::new();
        for table in &dictionary.tables {
            let (physical_table, model) = self.convert_model(table)?;
            if models_by_table.insert(physical_table.clone(), model.name().clone()).is_some() {
                return Err(RdbmsError::DuplicateTable {
                    table: physical_table.to_string(),
                });
            }
            models.push(model);
        }
        let mut relationships = Vec::with_capacity(dictionary.relationships.len());
        for relationship in &dictionary.relationships {
            relationships.push(Self::convert_relationship(relationship, &models_by_table)?);
        }

        let definitions =
            Definitions::assemble(models, relationships, Vec::new()).map_err(|cause| RdbmsError::Inconsistent { cause })?;
        Ok((definitions, Knowledge::none()))
    }

    /// A table record into a [`Model`].
    fn convert_model(&self, table: &Table) -> Result<ConvertedModel, RdbmsError> {
        let physical_table = Self::convert_table_address(table.address())?;
        let name = ModelName::parse(table.model()).map_err(|cause| RdbmsError::ModelName {
            table: physical_table.to_string(),
            model: table.model().to_owned(),
            cause,
        })?;
        let columns = table
            .columns
            .iter()
            .map(|column| {
                exact_physical_identifier(column)
                    .and_then(ColumnName::parse)
                    .map_err(|cause| RdbmsError::ColumnName {
                        table: physical_table.to_string(),
                        column: column.clone(),
                        cause,
                    })
            })
            .collect::<Result<BTreeSet<ColumnName>, _>>()?;
        let description = table
            .description()
            .map(|raw| {
                Description::parse(raw).map_err(|cause| RdbmsError::Description {
                    table: physical_table.to_string(),
                    cause,
                })
            })
            .transpose()?
            .unwrap_or_default();
        let model = Model::new(name, self.name.clone(), physical_table.clone(), columns, description);
        Ok((physical_table, model))
    }

    fn convert_table_address(address: &TableAddress) -> Result<QualifiedTable, RdbmsError> {
        let rendered = address.to_string();
        let table = exact_physical_identifier(address.table())
            .and_then(TableName::parse)
            .map_err(|cause| RdbmsError::TableName {
                table: rendered.clone(),
                cause,
            })?;
        let schema = exact_physical_identifier(address.schema())
            .and_then(DatasetName::parse)
            .map_err(|cause| RdbmsError::SchemaName {
                table: rendered.clone(),
                schema: address.schema().to_owned(),
                cause,
            })?;
        let qualifier = if let Some(catalog) = address.catalog() {
            let catalog = exact_physical_identifier(catalog)
                .and_then(ProjectName::parse)
                .map_err(|cause| RdbmsError::CatalogName {
                    table: rendered,
                    catalog: catalog.to_owned(),
                    cause,
                })?;
            TableQualifier::in_project(catalog, schema)
        } else {
            TableQualifier::in_dataset(schema)
        };
        Ok(QualifiedTable::new(Some(qualifier), table))
    }

    /// A foreign key into a [`Relationship`].
    ///
    /// A single-column primary or unique-key constraint on the referenced column is required before
    /// this maps the join to [`JoinType::ManyToOne`]. Membership in a composite constraint does not
    /// qualify. The adapter still declares no `Cardinality` capability: the dictionary carries no
    /// metric for a dimension to reach through the relationship.
    fn convert_relationship(
        relationship: &Relationship,
        models_by_table: &BTreeMap<QualifiedTable, ModelName>,
    ) -> Result<DomainRelationship, RdbmsError> {
        if relationship.target_uniqueness.is_none() {
            return Err(RdbmsError::TargetUniquenessUnknown {
                table: relationship.target_table().to_string(),
                column: relationship.target_column().to_owned(),
            });
        }
        let origin_table = Self::convert_table_address(relationship.origin_table())?;
        let target_table = Self::convert_table_address(relationship.target_table())?;
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
        let origin_column = exact_physical_identifier(relationship.origin_column())
            .and_then(ColumnName::parse)
            .map_err(|cause| RdbmsError::ColumnName {
                table: origin_table.to_string(),
                column: relationship.origin_column().to_owned(),
                cause,
            })?;
        let target_column = exact_physical_identifier(relationship.target_column())
            .and_then(ColumnName::parse)
            .map_err(|cause| RdbmsError::ColumnName {
                table: target_table.to_string(),
                column: relationship.target_column().to_owned(),
                cause,
            })?;
        let name = Self::relationship_name(
            relationship.name(),
            &origin_table,
            &origin_column,
            &target_table,
            &target_column,
        )?;
        Ok(DomainRelationship::new(
            name,
            origin_model,
            origin_column,
            target_model,
            target_column,
            JoinType::ManyToOne,
        ))
    }

    fn relationship_name(
        constraint: Option<&str>,
        origin_table: &QualifiedTable,
        origin_column: &ColumnName,
        target_table: &QualifiedTable,
        target_column: &ColumnName,
    ) -> Result<RelationshipName, RdbmsError> {
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
        let fingerprint = stable_fingerprint([
            constraint.unwrap_or(""),
            origin_table.as_str(),
            origin_column.as_str(),
            target_table.as_str(),
            target_column.as_str(),
        ]);
        prefix.truncate(prefix.len().min(MAX_RELATIONSHIP_PREFIX_LEN));
        let namespaced = format!("{prefix}__{fingerprint:016x}");
        RelationshipName::parse(&namespaced).map_err(|cause| RdbmsError::RelationshipName {
            relationship: namespaced,
            cause,
        })
    }
}

/// A deterministic, non-cryptographic fingerprint for a relationship's canonical identity.
///
/// The `0xff` separator cannot occur in UTF-8, so adjacent fields remain unambiguous. A collision
/// remains a duplicate relationship and is refused by [`Definitions::assemble`]; it can never
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

impl<R> SemanticCatalog for RdbmsCatalog<R>
where
    R: DictionaryReader,
{
    type Error = RdbmsError;

    /// A **declaring** adapter, measured against its own declaration rather than the golden oracle.
    const KIND: CatalogKind = CatalogKind::Declaring;
    /// Provides `Structure`, `Descriptions` and `Relationships` - exactly what a dictionary *can*
    /// yield - and nothing else.
    ///
    /// `Structure` is unconditional; descriptions and relationships are declared-and-conditional.
    /// A foreign key whose target lacks single-column primary or unique-key evidence refuses the
    /// whole load with [`RdbmsError::TargetUniquenessUnknown`] rather than being dropped; a schema
    /// with no foreign key lawfully carries no relationship. This is the single-column case only -
    /// a composite (multi-column) foreign key is not representable in [`DomainRelationship`], so a
    /// [`DictionaryReader`] must drop or refuse it before it ever reaches this conversion.
    ///
    /// What is declared is nothing more. No
    /// [`Cardinality`](sutura_domain::capabilities::DefinitionKind::Cardinality) - a foreign key
    /// vouches for no fan-out in the dangerous direction, and a dictionary carries no metric for a
    /// dimension to be reached `via` one, so `produced` observes `Cardinality` absent and the
    /// declaration agrees. No `Metrics`, no `Grains`, no `RequiredFilters`, no `AllowedValues`, no
    /// `Anchors`. **The knowledge half is empty for the same reason `sutura-catalog-datahub`'s
    /// is**: nothing here reads a glossary-like aspect, so a standalone bundle carries no
    /// `Knowledge` referent for a phrase or a caveat to attach to.
    ///
    /// Written as `of([..])` plus `and_may_provide([..])` with two explicit lists, the way an
    /// adapter over a fixed external schema must, so a tenth definition kind or a fifth knowledge
    /// capability leaves this declaration alone rather than silently widening it.
    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::of(
            DefinitionCapabilities::of([DefinitionKind::Structure])
                .and_may_provide([DefinitionKind::Descriptions, DefinitionKind::Relationships]),
            KnowledgeCapabilities::none(),
        )
    }

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let dictionary = self.reader.read_dictionary()?;
        let (definitions, knowledge) = self.assemble(&dictionary)?;
        PinnedDefinitions::pin(
            self.version.clone(),
            definitions,
            knowledge,
            ContributionManifest::single(self.name.clone(), Contribution::of(<Self as SemanticCatalog>::capabilities())),
        )
        .map_err(|cause| RdbmsError::Digest { cause })
    }
}

/// A table's physical address as a dictionary reports it.
///
/// A schema is required because it is the first part that distinguishes same-named tables in one
/// database. The catalog is optional because not every target renders a three-part table path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableAddress {
    catalog: Option<String>,
    schema: String,
    table: String,
}

impl TableAddress {
    /// A physical table address, split into the dictionary fields that own its identity.
    pub const fn new(catalog: Option<String>, schema: String, table: String) -> Self {
        Self { catalog, schema, table }
    }

    /// A table in a schema of the connected catalog.
    pub const fn in_schema(schema: String, table: String) -> Self {
        Self::new(None, schema, table)
    }

    /// The catalog above the schema, when the dictionary reports one for generated statements.
    #[inline]
    pub fn catalog(&self) -> Option<&str> {
        self.catalog.as_deref()
    }

    /// The schema immediately above the table.
    #[inline]
    pub fn schema(&self) -> &str {
        &self.schema
    }

    /// The table's own name.
    #[inline]
    pub fn table(&self) -> &str {
        &self.table
    }
}

impl core::fmt::Display for TableAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if let Some(catalog) = self.catalog() {
            write!(f, "{catalog}.")?;
        }
        write!(f, "{}.{}", self.schema, self.table)
    }
}

/// One semantic model, the physical table it selects, and the prose written against it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    model: String,
    address: TableAddress,
    columns: Vec<String>,
    description: Option<String>,
}

impl Table {
    /// A semantic model over a physical table.
    pub const fn new(model: String, address: TableAddress, columns: Vec<String>, description: Option<String>) -> Self {
        Self {
            model,
            address,
            columns,
            description,
        }
    }

    /// The semantic model name assigned to this table.
    #[inline]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// The physical table address, as the dictionary spells each part.
    #[inline]
    pub const fn address(&self) -> &TableAddress {
        &self.address
    }

    /// The columns the table exposes.
    #[inline]
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// The table's comment, if a human wrote one.
    #[inline]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

/// A dictionary: the tables, and the relationships a foreign key between them records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dictionary {
    tables: Vec<Table>,
    relationships: Vec<Relationship>,
}

impl Dictionary {
    /// A dictionary assembled from what a reader fetched.
    pub const fn new(tables: Vec<Table>, relationships: Vec<Relationship>) -> Self {
        Self { tables, relationships }
    }

    /// The tables the dictionary names.
    #[inline]
    pub fn tables(&self) -> &[Table] {
        &self.tables
    }

    /// The relationships a foreign key records.
    #[inline]
    pub fn relationships(&self) -> &[Relationship] {
        &self.relationships
    }
}

/// Why the target column of a foreign key is known to be individually unique.
///
/// This variant records what the reader found in the dictionary; it carries no constraint name and
/// no column list, so this converter checks only that a variant is present and cannot re-derive or
/// verify that the underlying constraint is truly single-column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SingleColumnTargetUniqueness {
    /// The target column is the sole column of a primary key.
    PrimaryKey,
    /// The target column is the sole column of a unique constraint.
    UniqueConstraint,
}

/// A join a foreign key records: the two endpoints, their columns, and single-column target-key
/// evidence.
///
/// [`SingleColumnTargetUniqueness`] is evidence for the safe `ManyToOne` direction, not a metric
/// cardinality. Without it, loading refuses before a domain relationship is emitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    name: Option<String>,
    origin_table: TableAddress,
    origin_column: String,
    target_table: TableAddress,
    target_column: String,
    target_uniqueness: Option<SingleColumnTargetUniqueness>,
}

impl Relationship {
    /// A relationship's endpoints, with target-key evidence if the reader has any.
    ///
    /// Loading refuses this value when `target_uniqueness` is `None`.
    pub const fn new(
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
            origin_column,
            target_table,
            target_column,
            target_uniqueness,
        }
    }

    /// The foreign key's name, if the dictionary named it.
    #[inline]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The table the foreign key starts from.
    #[inline]
    pub const fn origin_table(&self) -> &TableAddress {
        &self.origin_table
    }

    /// The column the foreign key starts from.
    #[inline]
    pub fn origin_column(&self) -> &str {
        &self.origin_column
    }

    /// The table the foreign key points to.
    #[inline]
    pub const fn target_table(&self) -> &TableAddress {
        &self.target_table
    }

    /// The column the foreign key points to.
    #[inline]
    pub fn target_column(&self) -> &str {
        &self.target_column
    }
}

#[cfg(test)]
mod tests;
