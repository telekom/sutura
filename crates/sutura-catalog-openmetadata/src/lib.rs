#![forbid(unsafe_code)]
//! A [`SemanticCatalog`] over an `OpenMetadata` deployment: the richest of the three measured metadata
//! sources, and still a **declaring** one.
//!
//! `docs/what-openmetadata-can-carry.md` is the finding that decides this adapter's whole shape.
//! `OpenMetadata` carries a typed metric entity and declared relationship cardinality - both richer
//! than `DataHub` or `Frictionless Table Schema` - but keeps **two half-a-definition slots**: a measure's
//! aggregation-to-column binding lives in `measures[].expression` / `metricExpression.code` as free
//! text in a dialect set (`SQL`/`Java`/`JavaScript`/`Python`/`External`) that does not intersect this
//! repository's typed `Term`, and the required filter is a raw SQL `where`. ADR 0016's refusal fires
//! on both: a measure carried as a raw expression string is half a definition, and taking it would
//! certify a foreign dialect's free text.
//!
//! So this adapter is a `CatalogKind::Declaring` source that provides the physical model, the
//! descriptions, and the declared non-duplicating joins, and treats the kind it cannot faithfully
//! express exactly as the finding says: a metric whose measure is an expression string is
//! **reported and not defined** - its `metricType` is decidable but its bound column is not
//! resolvable from the foreign text, so it is decoded and set aside, never minted into a domain
//! `Measure`. `capabilities()` declares the deployment-dependent kinds (`Metrics`, `Grains` reached
//! through those metrics, and `Cardinality` observed only through a dimension reached via a
//! relationship) as **declared-and-empty may-provide kinds**, the 0011 state
//! [`DefinitionCapabilities::of_may_provide`] exists for.
//!
//! # What is built here, and what is NOT
//!
//! This crate contains everything [`OpenMetadataCatalog`] DECIDES about the documents a reader
//! extracts. It is tested against a fake reader that serves recorded documents - the port gets a fake,
//! not mocked HTTP. [`SnapshotReader`] is the seam a real reader over `OpenMetadata`'s `REST` API
//! (`/api/v1/tables`, `/api/v1/metrics`, …) implements, with a bearer credential; that HTTP reader is
//! deliberately NOT in this first PR, so the crate stays green (a service has no network in the nix
//! sandbox). The `http` reader + the live provisioned leg are the recorded follow-up.
//!
//! # The declaration, and what it means for the bundle
//!
//! `Structure`, `Descriptions` and `Relationships` are provided unconditionally: a `Table` with its
//! `columns[]` becomes a model, its `description` the model's description, and a `tableConstraint` /
//! `foreignKey` whose `relationshipType` is declared `ONE_TO_ONE` / `MANY_TO_ONE` / `ONE_TO_MANY` a
//! relationship. A relationship whose cardinality is absent **or** `MANY_TO_MANY` is refused naming
//! it - this is the pleasant surprise the finding records: `OpenMetadata` declares cardinality when it
//! is there and stays silent when it is not, so an undeclared relationship licenses nothing, and a
//! row-duplicating one is refused by this adapter rather than defaulted in either direction. A bundle
//! whose relationships are all unconstrained therefore carries none, lawfully.
//!
//! `Metrics`, `Grains` and `Cardinality` are declared-and-empty may-provide kinds: whether a bundle
//! carries any is the deployment's decision (it defined a metric whose binding resolves, or it did
//! not), so absence is faithful rather than an aspirational claim. The metric entity IS decoded and
//! `metricType` + `granularity` + `dimensions[].type` are read, but `Measure` is minted only where a
//! column binding resolves to a domain `Term` without certifying a foreign dialect's free text - which
//! the recorded fixtures deliberately do not - so today those kinds arrive empty and the expression
//! strings stay reported-not-defined, exactly as the ADR 0016 refusal demands.
//!
//! `RequiredFilters`, `AllowedValues` and `Anchors` are not declared at all: the filter is a raw SQL
//! `where` never parsed into `RequiredFilter`, a dimension carries no allowlist, and the metric/table
//! entities carry no `Anchor`. `KnowledgeCapabilities::none()` on the knowledge half, because only
//! prose travels (descriptions/tags) and none of the referent-bearing kinds is read.

pub mod document;
pub mod fixture;

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{
    Column, Definitions, Description, InconsistentDefinitions, InvalidDescription, Model, Relationship,
};
use sutura_domain::definitions::NotDigestible;
use sutura_domain::knowledge::{InconsistentKnowledge, Knowledge, KnowledgeCapabilities, KnowledgeInput};
use sutura_domain::model::{ColumnName, JoinType, ModelName, RelationshipName, SourceName, TableName};
use sutura_domain::pinned::{
    CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog,
};

/// The two halves of a bundle, read and checked but not yet pinned.
type Content = (Definitions, Knowledge);

/// Where the documents a [`OpenMetadataCatalog`] decides over come from.
///
/// **The fake seam.** Everything above this trait is decided and tested against recorded documents;
/// a real implementor speaks to `OpenMetadata`'s `REST` API, decodes into [`document::Snapshot`], and maps
/// its own failures into [`OpenMetadataError::Read`]. Every other test reads against the recorded
/// source. A port rather than a method on the catalog, for the same reason the warehouse port exists:
/// a catalog that could be swapped for a live source without the conversion changing is the point.
pub trait SnapshotReader {
    type Error: std::error::Error + 'static;

    fn read(&self) -> Result<document::Snapshot, Self::Error>;
}

/// Why a record could not be read as a catalog.
///
/// Every variant is a typed contract rather than a message; the variant is what a caller can branch
/// on. The mapping variants carry the entity name they were refused on, because a catalog is many
/// entities and "invalid identifier" with no name sends a reader back to all of them.
#[derive(Debug, thiserror::Error)]
pub enum OpenMetadataError {
    #[error("the reader could not produce a snapshot")]
    Read(#[source] Box<dyn std::error::Error + 'static>),
    #[error("the {kind} of {on} is not a name")]
    Identifier {
        kind: &'static str,
        value: String,
        on: String,
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    #[error("the table {model} is on no source this deployment reads ({platform})")]
    UnknownSource { model: String, platform: String },
    #[error("the table {on} carries no description to describe the model")]
    MissingDescription { on: String },
    #[error("the description of {on} is not usable")]
    Description {
        on: String,
        #[source]
        cause: InvalidDescription,
    },
    #[error("the description of column {column} on {on} is not usable")]
    ColumnDescription {
        on: String,
        column: ColumnName,
        #[source]
        cause: InvalidDescription,
    },
    #[error(
        "the relationship {name} carries an unrepresentable cardinality ({cardinality}) - declared or not, it licences nothing"
    )]
    CardinalityUnrepresentable { name: String, cardinality: &'static str },
    #[error("the catalog does not hold together")]
    Inconsistent {
        #[source]
        cause: InconsistentDefinitions,
    },
    #[error("the catalog's knowledge does not hold together")]
    Knowledge {
        #[source]
        cause: InconsistentKnowledge,
    },
    #[error("the definitions could not be hashed")]
    Digest {
        #[source]
        cause: NotDigestible,
    },
}

/// A catalog read from an `OpenMetadata` deployment's documents.
///
/// Generic in its reader rather than holding a boxed one, matching the warehouse adapters: there is
/// one per composition and a generic keeps the chosen reader visible. It carries the deployment's
/// source mapping - which service/database alias answers to which [`SourceName`] - and the declared
/// name and version it is recorded under.
#[derive(Debug, Clone)]
pub struct OpenMetadataCatalog<R> {
    name: SourceName,
    version: DefinitionVersion,
    sources: std::collections::BTreeMap<String, SourceName>,
    reader: R,
}

impl<R: SnapshotReader> OpenMetadataCatalog<R> {
    /// Opens a catalog over an `OpenMetadata` reader.
    ///
    /// `sources` is the deployment's mapping from a service/database alias to the `sources.<alias>` a
    /// model on that platform reads from. A model on a platform with no entry is refused at load
    /// ([`OpenMetadataError::UnknownSource`]) rather than guessed.
    pub const fn new(
        name: SourceName,
        version: DefinitionVersion,
        sources: std::collections::BTreeMap<String, SourceName>,
        reader: R,
    ) -> Self {
        Self {
            name,
            version,
            sources,
            reader,
        }
    }

    /// The conversion: a snapshot into the two halves of a bundle.
    ///
    /// Kept as a method that takes a `document::Snapshot` so the whole decision half is testable
    /// against a reader that returns one.
    fn assemble(&self, snapshot: &document::Snapshot) -> Result<Content, OpenMetadataError> {
        let mut models = Vec::with_capacity(snapshot.tables().len());
        for table in snapshot.tables() {
            models.push(self.convert_model(table)?);
        }
        let mut relationships = Vec::with_capacity(snapshot.relationship_count());
        for (name, relationship) in snapshot.relationships() {
            relationships.push(Self::convert_relationship(name, relationship)?);
        }
        // A metric is deliberately NOT converted here: its `metricType` is decidable but its bound
        // column is not resolvable from a foreign-dialect expression string, and certifying one would
        // be ADR 0016's half-a-definition refusal. The metric is reported (the document describes it)
        // and not defined (no domain `Metric` is minted), which is what the reported-not-defined cell
        // holds. The `Metrics`/`Grains`/`Cardinality` kinds are declared-and-empty may-provide, so the
        // bundle agrees with the declaration either way.

        let definitions = Definitions::assemble(models, relationships, Vec::new())
            .map_err(|cause| OpenMetadataError::Inconsistent { cause })?;
        let knowledge =
            Knowledge::assemble(&definitions, KnowledgeInput::none()).map_err(|cause| OpenMetadataError::Knowledge { cause })?;
        Ok((definitions, knowledge))
    }

    /// One `Table` document into a domain [`Model`].
    fn convert_model(&self, table: &document::Table) -> Result<Model, OpenMetadataError> {
        let source = self
            .sources
            .get(table.service())
            .cloned()
            .ok_or_else(|| OpenMetadataError::UnknownSource {
                model: table.name().to_owned(),
                platform: table.service().to_owned(),
            })?;
        let name = Self::identifier(table.name(), |raw| ModelName::parse(raw), "model", "a table")?;
        let table_name = Self::identifier(table.name(), |raw| TableName::parse(raw), "table", table.name())?;
        let mut columns = Vec::with_capacity(table.columns().len());
        for column in table.columns() {
            let column_name = Self::identifier(column, |raw| ColumnName::parse(raw), "column", table.name())?;
            let metadata = table.column_metadata(column);
            let column = Column::from_metadata(
                column_name.clone(),
                metadata.and_then(document::ColumnMetadata::data_type),
                metadata.and_then(document::ColumnMetadata::description),
                None,
            )
            .map_err(|cause| OpenMetadataError::ColumnDescription {
                on: table.name().to_owned(),
                column: column_name.clone(),
                cause,
            })?;
            columns.push(column);
        }
        // A model's description is supplied (Descriptions is a provided kind); one without a
        // description would leave the declaration unproduced for it. Refuse rather than default.
        let description_text = table
            .description()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .ok_or_else(|| OpenMetadataError::MissingDescription {
                on: table.name().to_owned(),
            })?;
        let description = Description::parse(description_text).map_err(|cause| OpenMetadataError::Description {
            on: table.name().to_owned(),
            cause,
        })?;
        let primary_key = table
            .primary_key()
            .iter()
            .map(|column| Self::identifier(column, |raw| ColumnName::parse(raw), "column", table.name()))
            .collect::<Result<Vec<_>, _>>()?;
        Model::new(name, source, table_name, columns, description)
            .with_primary_key(primary_key)
            .map_err(|cause| OpenMetadataError::Inconsistent { cause })
    }

    /// One relationship (a `name` → structural endpoints) into a domain [`Relationship`].
    fn convert_relationship(
        name: &str,
        relationship: &document::StructuralRelationship,
    ) -> Result<Relationship, OpenMetadataError> {
        let name = Self::identifier(name, |raw| RelationshipName::parse(raw), "relationship", "a relationship")?;
        let origin_model = Self::identifier(
            relationship.origin_model(),
            |raw| ModelName::parse(raw),
            "model",
            name.as_str(),
        )?;
        let origin_column = Self::identifier(
            relationship.origin_column(),
            |raw| ColumnName::parse(raw),
            "column",
            name.as_str(),
        )?;
        let target_model = Self::identifier(
            relationship.target_model(),
            |raw| ModelName::parse(raw),
            "model",
            name.as_str(),
        )?;
        let target_column = Self::identifier(
            relationship.target_column(),
            |raw| ColumnName::parse(raw),
            "column",
            name.as_str(),
        )?;
        let join_type = match relationship.relationship_type() {
            Some(document::RelationshipType::OneToOne) => JoinType::OneToOne,
            Some(document::RelationshipType::ManyToOne) => JoinType::ManyToOne,
            Some(document::RelationshipType::OneToMany) => JoinType::OneToMany,
            // `OpenMetadata` declares cardinality when it is present and stays silent when it is not;
            // either way an unrepresentable one licenses no join here.
            Some(document::RelationshipType::ManyToMany) | None => {
                let cardinality = match relationship.relationship_type() {
                    Some(document::RelationshipType::ManyToMany) => "many-to-many",
                    _ => "undeclared",
                };
                return Err(OpenMetadataError::CardinalityUnrepresentable {
                    name: name.to_string(),
                    cardinality,
                });
            }
        };
        Ok(Relationship::new(
            name,
            origin_model,
            origin_column,
            target_model,
            target_column,
            join_type,
        ))
    }

    /// Parses one identifier, mapping the domain refusal into this adapter's typed error.
    fn identifier<T>(
        raw: &str,
        parse: impl Fn(&str) -> Result<T, sutura_domain::model::InvalidIdentifier>,
        kind: &'static str,
        on: &str,
    ) -> Result<T, OpenMetadataError> {
        parse(raw).map_err(|cause| OpenMetadataError::Identifier {
            kind,
            value: raw.to_owned(),
            on: on.to_owned(),
            cause,
        })
    }
}

impl<R> SemanticCatalog for OpenMetadataCatalog<R>
where
    R: SnapshotReader,
{
    type Error = OpenMetadataError;

    /// A **declaring** adapter, measured against its own declaration rather than the golden oracle.
    const KIND: CatalogKind = CatalogKind::Declaring;

    /// Provides `Structure`, `Descriptions` and `Relationships` unconditionally, and declares
    /// `Metrics`, `Grains` and `Cardinality` as **declared-and-empty may-provide kinds** - the 0011
    /// state [`DefinitionCapabilities::of_may_provide`] adds - because whether a bundle carries any of
    /// them is the deployment's decision (it defined a metric whose binding resolves, or it did not).
    /// `checked_against`'s `Unprovided` direction exempts them, so a metric-free deployment is
    /// servable; their presence, the day it resolves, is still covered by the declared half.
    ///
    /// `ColumnTypes` and `ColumnDescriptions` are may-provide too, and that is the conservative
    /// choice rather than the exact one: `OpenMetadata`'s own `Column` schema makes `dataType`
    /// mandatory, so a live source could in principle vouch for it unconditionally the way
    /// `Structure` is. Nothing here refuses a column with no `column_metadata` entry, so this
    /// adapter's declaration stays honest about what IT enforces rather than about what the upstream
    /// schema happens to require.
    ///
    /// `RequiredFilters`, `AllowedValues` and `Anchors` are deliberately NOT declared: the required
    /// filter is a raw SQL `where` this adapter never parses into `RequiredFilter`, a dimension
    /// carries no allowlist, and none of the entities carries an `Anchor`. The knowledge half is empty
    /// because only prose travels and none of the referent-bearing knowledge kinds is read.
    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::of(
            DefinitionCapabilities::of([
                DefinitionKind::Structure,
                DefinitionKind::Descriptions,
                DefinitionKind::Relationships,
            ])
            .and_may_provide([
                DefinitionKind::Metrics,
                DefinitionKind::Grains,
                DefinitionKind::Cardinality,
                DefinitionKind::ColumnTypes,
                DefinitionKind::ColumnDescriptions,
            ]),
            KnowledgeCapabilities::none(),
        )
    }

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let snapshot = self.reader.read().map_err(|cause| OpenMetadataError::Read(Box::new(cause)))?;
        let (definitions, knowledge) = self.assemble(&snapshot)?;
        PinnedDefinitions::pin(
            self.version.clone(),
            definitions,
            knowledge,
            ContributionManifest::single(self.name.clone(), Contribution::of(<Self as SemanticCatalog>::capabilities())),
        )
        .map_err(|cause| OpenMetadataError::Digest { cause })
    }
}

#[cfg(test)]
mod tests;
