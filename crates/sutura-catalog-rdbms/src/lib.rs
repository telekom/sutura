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
//! reader and runtime prompt delivery remain outside it.
//!
//! # What a real dictionary yields (the spike, `spike/read-a-dictionary`)
//!
//! ADR 0016's method, applied here: read a real dictionary before writing the adapter. A throwaway
//! reader pointed at this worktree's provisioned `postgresql_18` reported the following against a
//! representative schema (two tables, a primary key each, one foreign key, and table comments):
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
//!    relationship.
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

use std::collections::BTreeSet;

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{Definitions, Description, Model, Relationship as DomainRelationship};
use sutura_domain::knowledge::{Knowledge, KnowledgeCapabilities};
use sutura_domain::model::{ColumnName, JoinType, ModelName, RelationshipName, SourceName, TableName};
use sutura_domain::pinned::{
    CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog,
};

/// The two halves of a bundle, read and checked but not yet pinned.
type Content = (Definitions, Knowledge);

/// Where a dictionary's records come from.
///
/// **The fake seam.** Everything above this trait is decided and tested against a recorded
/// dictionary served by [`fixture::FixtureReader`]; a real implementor speaks to a Postgres socket,
/// reads `information_schema` / `pg_catalog`, decodes into [`Dictionary`], and maps its own failures
/// into [`RdbmsError::Read`]. A port rather than a method on [`RdbmsCatalog`] for the same reason
/// the warehouse port exists: a catalog that could be swapped for a live source without the
/// conversion changing is the point.
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
    /// A table's name did not parse as a model name.
    #[error("table {table} is not a usable model name: {cause}")]
    ModelName {
        table: String,
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
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
        cause: sutura_domain::catalog::InconsistentDefinitions,
    },
    /// Pinning failed.
    #[error("the dictionary bundle could not be pinned: {cause}")]
    Digest {
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
        let mut models = Vec::with_capacity(dictionary.tables.len());
        for table in &dictionary.tables {
            models.push(self.convert_model(table)?);
        }
        let mut relationships = Vec::with_capacity(dictionary.relationships.len());
        for relationship in &dictionary.relationships {
            relationships.push(Self::convert_relationship(relationship)?);
        }

        let definitions =
            Definitions::assemble(models, relationships, Vec::new()).map_err(|cause| RdbmsError::Inconsistent { cause })?;
        Ok((definitions, Knowledge::none()))
    }

    /// A table record into a [`Model`].
    fn convert_model(&self, table: &Table) -> Result<Model, RdbmsError> {
        let name = ModelName::parse(&table.name).map_err(|cause| RdbmsError::ModelName {
            table: table.name.clone(),
            cause,
        })?;
        let table_name = TableName::parse(&table.name).map_err(|cause| RdbmsError::ModelName {
            table: table.name.clone(),
            cause,
        })?;
        let columns = table
            .columns
            .iter()
            .map(|column| {
                ColumnName::parse(column).map_err(|cause| RdbmsError::ColumnName {
                    table: table.name.clone(),
                    column: column.clone(),
                    cause,
                })
            })
            .collect::<Result<BTreeSet<ColumnName>, _>>()?;
        let description = table
            .description()
            .map(|raw| {
                Description::parse(raw).map_err(|cause| RdbmsError::Description {
                    table: table.name.clone(),
                    cause,
                })
            })
            .transpose()?
            .unwrap_or_default();
        Ok(Model::new(name, self.name.clone(), table_name, columns, description))
    }

    /// A foreign key into a [`Relationship`].
    ///
    /// A single-column primary or unique-key constraint on the referenced column is required before
    /// this maps the join to [`JoinType::ManyToOne`]. Membership in a composite constraint does not
    /// qualify. The adapter still declares no `Cardinality` capability: the dictionary carries no
    /// metric for a dimension to reach through the relationship.
    fn convert_relationship(relationship: &Relationship) -> Result<DomainRelationship, RdbmsError> {
        if relationship.target_uniqueness.is_none() {
            return Err(RdbmsError::TargetUniquenessUnknown {
                table: relationship.target_table().to_owned(),
                column: relationship.target_column().to_owned(),
            });
        }
        let name = match relationship.name() {
            Some(raw) => RelationshipName::parse(raw).map_err(|cause| RdbmsError::RelationshipName {
                relationship: raw.to_owned(),
                cause,
            })?,
            None => {
                // Postgres names every constraint, but a reader may not have read the name. Derive a
                // deterministic one from the endpoints so the bundle is repeatable.
                RelationshipName::parse(format!(
                    "{}__{}__to__{}__{}",
                    relationship.origin_table(),
                    relationship.origin_column(),
                    relationship.target_table(),
                    relationship.target_column()
                ))
                .map_err(|cause| RdbmsError::RelationshipName {
                    relationship: relationship.target_table().to_owned(),
                    cause,
                })?
            }
        };
        let origin_model = ModelName::parse(relationship.origin_table()).map_err(|cause| RdbmsError::ModelName {
            table: relationship.origin_table().to_owned(),
            cause,
        })?;
        let origin_column = ColumnName::parse(relationship.origin_column()).map_err(|cause| RdbmsError::ColumnName {
            table: relationship.origin_table().to_owned(),
            column: relationship.origin_column().to_owned(),
            cause,
        })?;
        let target_model = ModelName::parse(relationship.target_table()).map_err(|cause| RdbmsError::ModelName {
            table: relationship.target_table().to_owned(),
            cause,
        })?;
        let target_column = ColumnName::parse(relationship.target_column()).map_err(|cause| RdbmsError::ColumnName {
            table: relationship.target_table().to_owned(),
            column: relationship.target_column().to_owned(),
            cause,
        })?;
        Ok(DomainRelationship::new(
            name,
            origin_model,
            origin_column,
            target_model,
            target_column,
            JoinType::ManyToOne,
        ))
    }
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
    /// A relationship is emitted only when the reader supplies single-column primary or unique-key
    /// evidence for its target, while a schema with no foreign key carries no relationship
    /// lawfully.
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

/// One physical table the dictionary names, and the prose written against it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    name: String,
    columns: Vec<String>,
    description: Option<String>,
}

impl Table {
    /// A physical table.
    pub const fn new(name: String, columns: Vec<String>, description: Option<String>) -> Self {
        Self {
            name,
            columns,
            description,
        }
    }

    /// The table's name, as the dictionary spells it.
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
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
    origin_table: String,
    origin_column: String,
    target_table: String,
    target_column: String,
    target_uniqueness: Option<SingleColumnTargetUniqueness>,
}

impl Relationship {
    /// A relationship's endpoints, without uniqueness evidence.
    ///
    /// Loading refuses this value until [`Self::with_target_uniqueness`] records the target key.
    pub const fn new(
        name: Option<String>,
        origin_table: String,
        origin_column: String,
        target_table: String,
        target_column: String,
    ) -> Self {
        Self {
            name,
            origin_table,
            origin_column,
            target_table,
            target_column,
            target_uniqueness: None,
        }
    }

    /// Records why the target column is unique.
    #[must_use]
    pub const fn with_target_uniqueness(mut self, evidence: SingleColumnTargetUniqueness) -> Self {
        self.target_uniqueness = Some(evidence);
        self
    }

    /// The foreign key's name, if the dictionary named it.
    #[inline]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The table the foreign key starts from.
    #[inline]
    pub fn origin_table(&self) -> &str {
        &self.origin_table
    }

    /// The column the foreign key starts from.
    #[inline]
    pub fn origin_column(&self) -> &str {
        &self.origin_column
    }

    /// The table the foreign key points to.
    #[inline]
    pub fn target_table(&self) -> &str {
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
