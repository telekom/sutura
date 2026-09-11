//! A [`SemanticCatalog`] over an RDBMS dictionary - the narrowest declaration, and the one that
//! needs no service.
//!
//! `docs/adr/0011-pluggable-by-declaration.md` specifies this connector in full, and this crate is
//! what that specification schedules. A database dictionary is mainly DDL and comments: the tables,
//! the columns, their constraints, and whatever prose somebody wrote against them. It is not a
//! semantic layer and does not pretend to be one - which is the whole point of a **declaring**
//! adapter. It says which kinds it provides and which it does not, and it is measured against that
//! declaration rather than against the golden adapters' oracle.
//!
//! `github.com/telekom/sutura#151` is the issue, and its *What becomes provable* list is this
//! crate's acceptance suite.
//!
//! # What a real dictionary yields (the spike, `spike/read-a-dictionary`)
//!
//! ADR 0016's method, applied here: read a real dictionary before writing the adapter. A throwaway
//! reader pointed at this worktree's provisioned `postgresql_18` reported the following against a
//! representative schema (two tables, a primary key each, one foreign key, comments on tables and
//! columns):
//!
//! | What | Count |
//! | --- | --- |
//! | tables | 2 |
//! | columns | 6 |
//! | tables carrying a comment | 2 |
//! | columns carrying a comment | 3 |
//! | foreign keys | 1 |
//! | primary/unique constraints | 2 |
//!
//! Three findings, and two of them are the declaration's content:
//!
//! 1. **A dictionary yields structure and prose, and nothing else.** Tables and columns are the
//!    [`Structure`](sutura_domain::capabilities::DefinitionKind::Structure) half; table and column
//!    comments are the [`Descriptions`](sutura_domain::capabilities::DefinitionKind::Descriptions)
//!    half. A dictionary carries **no measure, no grain, no definitional filter, no value allowlist
//!    and no anchor** - those are declared by a human in a semantic layer, which is what ADR 0011's
//!    table says ("certified metrics, measures, grains, allowed values: **no** - a human declares
//!    those elsewhere").
//! 2. **A foreign key carries no cardinality.** The Postgres catalog's
//!    [`pg_constraint`](https://www.postgresql.org/docs/current/catalog-pg-constraint.html) row for a
//!    foreign-key constraint has no cardinality field: it names the source and target columns and
//!    nothing about how many rows match - so this adapter maps a foreign key to a
//!    [`JoinType::ManyToOne`] (the referenced column is provably unique, see finding 3) and declares
//!    the `Cardinality` *capability* absent, because a dictionary carries no metric to reach a
//!    dimension `via` a relationship.
//! 3. **A primary or unique key is evidence, and only the safe direction.** ADR 0011's "part worth
//!    having this connector for" is the one-direction uniqueness argument: a unique or primary-key
//!    constraint on the referenced column *proves* that side is unique, so a foreign key that
//!    references it maps to a relationship that does **not** duplicate rows (the `ManyToOne`
//!    direction). A foreign key therefore licences no dimension in the *unsafe* direction: there is
//!    no `OneToMany` reachable, and the domain's
//!    [`JoinWouldDuplicateRows`](sutura_domain::catalog::InconsistentDefinitions::JoinWouldDuplicateRows)
//!    refusal is unreachable - which is exactly the honest claim ADR 0011 makes ("confirm the safe
//!    direction and refuse a declaration that is provably over-cautious").
//!
//! # The declaration, and what it means for the bundle
//!
//! [`SemanticCatalog::capabilities`] provides `Structure`, `Descriptions` and `Relationships` as
//! **declared-and-conditional** kinds - the 0011 state
//! [`DefinitionCapabilities::and_may_provide`] adds, whose whole job is exactly this: a dictionary
//! is whatever the database documents about itself, so whether a bundle carries table comments or
//! a foreign key is a fact about the schema rather than a claim the adapter may over-state. A
//! sparse dictionary - an FK with no comments, a schema with no FK - is therefore a FAITHFUL
//! bundle, and `checked_against`'s `Unprovided` direction exempts the absent half. What is declared
//! is nothing more: no `Cardinality` (a foreign key vouches for no fan-out), no `Metrics`, no
//! `Grains`, no `RequiredFilters`, no `AllowedValues`, no `Anchors`, and an empty knowledge half.
//! A bundle from this source therefore **loads, pins and validates with zero metrics**, and answers
//! no certified question - which is issue #115's shape and the whole reason the declaration exists:
//! a deployment whose whole model is a physical schema must not be told it has metrics it does not.
//!
//! # What is built here, and what is NOT
//!
//! This crate contains everything [`RdbmsCatalog`] DECIDES about the records a dictionary yields,
//! and it is tested against a fake reader that serves a recorded dictionary - the port gets a fake,
//! not mocked SQL (`github.com/telekom/sutura#151`'s thing 4). What it does not contain is a
//! database client in the library closure: [`DictionaryReader`] is the seam a real reader over a
//! Postgres socket will implement, and the only implementor today is the recorded fixture source in
//! [`fixture`]. The spike's throwaway reader proved the read path is cheap and gate-reachable; the
//! production reader is what ADR 0011's *[the raw SQL tool](0013-a-raw-sql-tool-off-by-default.md)*
//! companion would drive, and is deliberately out of this crate's scope.
//!
//! **And nothing serves it:** no composition root links this crate (its only dependant is
//! `sutura-app`, as a dev-dependency), so this is a registered, declaring catalog rather than a
//! served one - exactly the state `sutura-catalog-datahub` holds, which is the precedent copied.

pub mod fixture;

use std::collections::BTreeSet;

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{Definitions, Description, Model, Relationship as DomainRelationship};
use sutura_domain::knowledge::{Knowledge, KnowledgeCapabilities, KnowledgeInput};
use sutura_domain::model::{ColumnName, JoinType, ModelName, RelationshipName, SourceName, TableName};
use sutura_domain::pinned::{CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog};

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
    ModelName { table: String, #[source] cause: sutura_domain::model::InvalidIdentifier },
    /// A column's name did not parse.
    #[error("column {column} on table {table} is not a usable column name: {cause}")]
    ColumnName { table: String, column: String, #[source] cause: sutura_domain::model::InvalidIdentifier },
    /// A foreign key's name did not parse.
    #[error("foreign key {relationship} is not a usable relationship name: {cause}")]
    RelationshipName { relationship: String, #[source] cause: sutura_domain::model::InvalidIdentifier },
    /// A table description did not pass the authored-prose rule.
    #[error("the description of table {table} is not usable: {cause}")]
    Description { table: String, #[source] cause: sutura_domain::catalog::InvalidDescription },
    /// The assembled definitions did not hold together.
    #[error("the dictionary definitions do not hold together: {cause}")]
    Inconsistent { cause: sutura_domain::catalog::InconsistentDefinitions },
    /// The knowledge did not assemble (a dictionary produces none, so unreachable unless a reader
    /// produces undeclared content).
    #[error("the dictionary knowledge did not assemble: {cause}")]
    Knowledge { cause: sutura_domain::knowledge::InconsistentKnowledge },
    /// Pinning failed.
    #[error("the dictionary bundle could not be pinned: {cause}")]
    Digest { cause: sutura_domain::definitions::NotDigestible },
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

        let definitions = Definitions::assemble(models, relationships, Vec::new())
            .map_err(|cause| RdbmsError::Inconsistent { cause })?;
        // A dictionary records no knowledge: nothing here reads a glossary aspect, so the bundle
        // carries none and the declaration agrees. The `Knowledge::assemble` call is what would
        // refuse undeclared content the day a reader produced any.
        let knowledge =
            Knowledge::assemble(&definitions, KnowledgeInput::none()).map_err(|cause| RdbmsError::Knowledge { cause })?;
        Ok((definitions, knowledge))
    }

    /// A table record into a [`Model`].
    fn convert_model(&self, table: &Table) -> Result<Model, RdbmsError> {
        let name = ModelName::parse(&table.name)
            .map_err(|cause| RdbmsError::ModelName { table: table.name.clone(), cause })?;
        let table_name = TableName::parse(&table.name)
            .map_err(|cause| RdbmsError::ModelName { table: table.name.clone(), cause })?;
        let columns = table
            .columns
            .iter()
            .map(|column| {
                ColumnName::parse(column)
                    .map_err(|cause| RdbmsError::ColumnName { table: table.name.clone(), column: column.clone(), cause })
            })
            .collect::<Result<BTreeSet<ColumnName>, _>>()?;
        let description = if let Some(raw) = table.description() {
            Some(
                Description::parse(raw)
                    .map_err(|cause| RdbmsError::Description { table: table.name.clone(), cause })?,
            )
        } else {
            None
        }
        .unwrap_or_default();
        Ok(Model::new(name, self.name.clone(), table_name, columns, description))
    }

    /// A foreign key into a [`Relationship`].
    ///
    /// Postgres guarantees the referenced column of a foreign key is unique (a foreign key must
    /// reference a primary key or a unique constraint - verified in the spike), so this maps the
    /// join to [`JoinType::ManyToOne`]: joining from the origin side to the target side cannot
    /// duplicate rows. That is the evidence-backed, safe direction ADR 0011 names. What this does
    /// NOT do is claim a `Cardinality` capability - the dictionary carries no metric for a
    /// dimension to be reached `via` one, so `produced` still observes `Cardinality` absent and the
    /// declaration agrees.
    fn convert_relationship(relationship: &Relationship) -> Result<DomainRelationship, RdbmsError> {
        let name = match relationship.name() {
            Some(raw) => RelationshipName::parse(raw)
                .map_err(|cause| RdbmsError::RelationshipName { relationship: raw.to_owned(), cause })?,
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
        let origin_model = ModelName::parse(relationship.origin_table())
            .map_err(|cause| RdbmsError::ModelName { table: relationship.origin_table().to_owned(), cause })?;
        let origin_column = ColumnName::parse(relationship.origin_column())
            .map_err(|cause| RdbmsError::ColumnName {
                table: relationship.origin_table().to_owned(),
                column: relationship.origin_column().to_owned(),
                cause,
            })?;
        let target_model = ModelName::parse(relationship.target_table())
            .map_err(|cause| RdbmsError::ModelName { table: relationship.target_table().to_owned(), cause })?;
        let target_column = ColumnName::parse(relationship.target_column())
            .map_err(|cause| RdbmsError::ColumnName {
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
    /// All three are **declared-and-conditional** ([`DefinitionCapabilities::of`] plus
    /// `and_may_provide`): a dictionary is whatever the database documents about itself, so whether
    /// a bundle actually carries prose or a foreign key is a fact about the schema, not a claim this
    /// adapter may over-state. A relationship IS read with the evidence-backed, safe direction - the
    /// referenced column is provably unique, so the join maps to [`JoinType::ManyToOne`] and cannot
    /// duplicate rows - but a schema with no foreign key carries no relationship, lawfully. The
    /// conditional marking is what keeps a thin dictionary from tripping `checked_against`'s
    /// `Unprovided` direction: absence of a may-provide kind is a faithful bundle, not an
    /// aspirational declaration.
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
            DefinitionCapabilities::of([
                DefinitionKind::Structure,
                DefinitionKind::Descriptions,
                DefinitionKind::Relationships,
            ])
            .and_may_provide([
                DefinitionKind::Structure,
                DefinitionKind::Descriptions,
                DefinitionKind::Relationships,
            ]),
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
        Self { name, columns, description }
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

/// A join a foreign key records: the two endpoints and their columns.
///
/// Deliberately carries **no** cardinality: [`Relationships`](sutura_domain::capabilities::DefinitionKind::Relationships)
/// is declared but the wavelet of a foreign key is only the endpoints, and a relationship a
/// dictionary vouches for is one whose fan-out is not vouched for. The relationship is read into the
/// bundle so its endpoints are authoritative; the dimension refusal stays with the domain's
/// `JoinWouldDuplicateRows` guard, which is what a reader reaches if a metric were ever to point
/// `via` it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    name: Option<String>,
    origin_table: String,
    origin_column: String,
    target_table: String,
    target_column: String,
}

impl Relationship {
    /// A relationship a foreign key records.
    #[allow(clippy::too_many_arguments)]
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
        }
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
