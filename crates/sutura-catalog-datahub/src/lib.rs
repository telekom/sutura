//! A [`SemanticCatalog`] over `DataHub`'s entity aspects: the canonical **declaring** source.
//!
//! `docs/adr/0016-what-datahub-can-carry.md` is the measurement that decides what this adapter is.
//! `DataHub` 1.7.0 holds a measure as a raw expression string in a dialect set that does not intersect
//! this repository's, and its physical relationships default cardinality to many-to-many - so it
//! supplies the physical model, the descriptions and the join columns, and supplies no measure this
//! adapter will execute **unless the deployment defines the metric itself**, no reliable cardinality,
//! no definitional filter, no grain, no value allowlist and no anchor. That is the shape of a
//! **declaring** adapter: it says which kinds it provides and which it does not, and it is measured
//! against that declaration rather than against the golden adapters' oracle.
//!
//! **Issue #202 is the exception the previous paragraph stops at, and it lives in this crate.**
//! `DataHub`'s `structuredProperty` is scalar-only, so a deployment cannot define a nested metric
//! object; what it CAN define is one string-valued structured property named `sutura`, and the
//! reader decodes its scalar value into the closed-vocabulary content a certified metric needs -
//! the flat-to-nested assembly `document::SuturaProperty::assemble` implements and the recorded
//! fixture is kept in. Where that shape is present, this adapter reads it into a certified
//! `Metric`, turning `provides no
//! metrics` into `provides metrics for a metric that carries the custom shape`. Where it is absent,
//! the metric stays the promotion candidate `docs/adr/0016` describes. The shape is closed - the
//! measure and filter vocabularies are `sutura_domain`'s own, and `deny_unknown_fields` refuses a
//! property this adapter does not recognise rather than guessing.
//!
//! # What is built here, and what is NOT
//!
//! This crate contains everything [`DataHubCatalog`] DECIDES about the aspects it reads, and it is
//! tested against a fake reader that serves recorded documents - the port gets a fake, not mocked
//! HTTP. What it does not contain is an HTTP client: [`AspectReader`] is the seam a real reader over
//! `DataHub`'s versioned `OpenAPI` v3 entity surface will implement (with a personal access token as a
//! bearer), and the read path's cost - how many requests a whole bundle takes - is explicitly the
//! opening engineering question `docs/adr/0016` leaves open, to be measured against a provisioned
//! instance the way `sutura-exec-bigquery`'s acceptance leg was. Until that lands, the only
//! implementor of the port is the recorded fixture source in [`fixture`]. **And nothing serves it:**
//! no composition root links this crate (its only dependant is `sutura-app`, as a dev-dependency),
//! and `sutura-serve` refuses `catalog.kind: datahub` by name. Everything here is decided and
//! tested; what is not is the read path against a provisioned instance and a served composition -
//! the *Built and not wired* register in `.agents/skills/sutura/query-surface/SKILL.md` records
//! it, and that register is the one place it may be read from - it is not an invariant.
//!
//! # The declaration, and what it means for the bundle
//!
//! [`SemanticCatalog::KIND`] is [`CatalogKind::Declaring`]. [`SemanticCatalog::capabilities`]
//! provides `Structure`, `Descriptions` and `Relationships` unconditionally, and declares
//! `Metrics`, `Grains`, `RequiredFilters`, `AllowedValues` and `Anchors` as **declared-and-empty
//! may-provide kinds** - the 0011 state [`DefinitionCapabilities::of_may_provide`] adds, whose whole
//! job is exactly this: a `DataHub` metric's measure, grains, filters, dimension allowlists and anchor
//! all arrive from the deployment-defined `sutura` structured property, so whether a bundle carries
//! any of them is the deployment's decision and absence is a faithful bundle, not an aspirational
//! claim. A `DataHub`-only deployment therefore uses the physical model, the prose, the join columns
//! and whatever metrics and definitions the deployment wrote as structured properties, and a bundle
//! with none of the deployment-authored kinds still loads and validates, because
//! `Definitions::assemble` has no minimum-metric refusal.
//!
//! **The knowledge half is empty on purpose, and why is worth stating rather than glossed.**
//! `DataHub` keeps its glossary-like synonym content on a separate entity (`AiContext`), and nothing
//! here reads it - so a standalone bundle carries no `Knowledge` referent for a phrase or a caveat to
//! attach to, and the declaration says no knowledge rather than advertising a capability this crate
//! cannot satisfy alone.
//!
//! # The one nuance that is the point
//!
//! `Cardinality` arrives in `DataHub` as an unreliable default (relationships default to
//! many-to-many), so this adapter refuses a relationship it cannot vouch for rather than reading
//! one: absent or many-to-many cardinality is refused naming the relationship, not defaulted in
//! either direction. What the adapter WILL carry is a cardinality the deployment DECLARES a
//! dimension through - a dimension with a `via` is observed as *a dimension reached through a
//! relationship*, which is the only way `Cardinality` is produced here, and the declaration marks
//! it declared-and-empty for exactly that reason. A metric whose measure is a raw expression string
//! is read - [`document::MetricAspect`] is decoded - and never converted into a `Measure`,
//! because that is the promotion-candidate half; only a metric carrying the deployment-defined
//! [`document::SuturaProperty`] becomes a certified one.

pub mod document;
pub mod fixture;

use std::collections::BTreeMap;

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{
    Definitions, Description, Dimension, InconsistentDefinitions, InvalidDescription, Metric, Model, Relationship,
};
use sutura_domain::definitions::NotDigestible;
use sutura_domain::knowledge::KnowledgeCapabilities;
use sutura_domain::knowledge::{InconsistentKnowledge, Knowledge, KnowledgeInput};
use sutura_domain::model::{ColumnName, JoinType, MetricName, ModelName, RelationshipName, SourceName, TableName};
use sutura_domain::pinned::{
    CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog,
};

use crate::document::{Cardinality, RelationshipAspect, Snapshot, SuturaAnchor};

/// The two halves of a bundle, read and checked but not yet pinned.
///
/// An alias because the pair appears in a signature and `clippy.toml` tightens
/// `type-complexity-threshold`, and because it is the better name: this is what a catalog IS.
type Content = (Definitions, Knowledge);

/// Where a snapshot's aspects come from.
///
/// **The fake seam.** Everything above this trait is decided and tested against recorded documents;
/// a real implementor speaks to `DataHub`'s versioned `OpenAPI` v3 entity surface, decodes into
/// [`document::Snapshot`], and maps its own failures into [`DataHubError::Read`]. The only
/// implementor today is the recorded source in [`fixture`], which is why the read path's cost is not
/// measured here - see the crate header.
///
/// A port rather than a method on [`DataHubCatalog`] for the same reason the warehouse port exists:
/// a catalog that could be swapped for a live source without the conversion changing is the point.
pub trait AspectReader {
    /// The aspects this deployment's `DataHub` carries, as a snapshot.
    fn read(&self) -> Result<Snapshot, DataHubError>;
}

/// Why a record could not be read as a catalog.
///
/// Every variant is a typed contract rather than a message; the message is for a human and the
/// variant is what a caller can branch on. The mapping variants carry the entity name they were
/// refused on, because a catalog is many entities and "invalid identifier" with no name sends a
/// reader back to all of them.
#[derive(Debug, thiserror::Error)]
pub enum DataHubError {
    /// The source did not produce a snapshot.
    ///
    /// An owned boxed cause, the `ErasedCause` shape this repository's boundary errors use: the
    /// concrete failure belongs to whichever reader is installed, and the chain still walks.
    #[error("the DataHub source could not be read")]
    Read {
        #[source]
        cause: Box<dyn core::error::Error + Send + Sync>,
    },
    /// A model named a platform this deployment declared no `sources.<alias>` for.
    ///
    /// A `sources.<alias>` entry per platform is what lets a model's data system be opened at all -
    /// `docs/adr/0016` decision 7 - and the mapping is the deployment's, not this adapter's. A model
    /// on an unmapped platform is refused rather than guessed.
    #[error("model {model} lives on platform {platform}, and no source is declared for it")]
    UnknownPlatform { model: String, platform: String },
    /// A relationship carries no cardinality, or one this adapter cannot represent.
    ///
    /// `docs/adr/0016` decision 5: a relationship reaches a dimension only where cardinality is
    /// declared and representable, and absent or many-to-many is refused naming the relationship
    /// rather than defaulted in either direction - `ManyToOne` as a default assumes the fan-out away,
    /// and `OneToMany` refuses every dimension.
    #[error("relationship {name} has {cardinality}, which this adapter does not provide")]
    CardinalityUnrepresentable { name: String, cardinality: &'static str },
    /// A name on a snapshot did not parse as the identifier kind it claims to be.
    #[error("{kind} {value} on {on} is not a usable identifier")]
    Identifier {
        kind: &'static str,
        value: String,
        on: String,
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    /// The `sutura` structured property's scalar value is not the metric content it claims to be.
    #[error("the sutura content of metric {metric} is not a usable metric definition")]
    Sutura {
        metric: String,
        #[source]
        cause: serde_json::Error,
    },
    /// Prose on a snapshot is not a usable description.
    #[error("the description of {on} is not usable")]
    Description {
        on: String,
        #[source]
        cause: InvalidDescription,
    },
    /// The models, relationships and columns did not hold together.
    #[error("the DataHub content does not hold together")]
    Inconsistent {
        #[source]
        cause: InconsistentDefinitions,
    },
    /// The bundle's knowledge does not hold together.
    ///
    /// This adapter declares no knowledge capability, so any knowledge content it were handed would
    /// be refused here (the `UndeclaredContent` guard) rather than dropped or forwarded. Today no
    /// snapshot produces knowledge - there is no metric for a `Referent` to name - so this stays the
    /// wiring for content that cannot occur in the standalone deployment.
    #[error("the DataHub content's knowledge does not hold together")]
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

/// A catalog read from `DataHub`'s entity aspects.
///
/// Generic in its reader rather than holding a boxed one, matching the warehouse adapters: there is
/// one per composition and a generic keeps the chosen reader visible. It carries the deployment's
/// source mapping - which `dataPlatform` answers to which `SourceName` - and the declared name and
/// version it is recorded under, the same way `sutura-catalog-local` carries its own.
#[derive(Debug, Clone)]
pub struct DataHubCatalog<R> {
    name: SourceName,
    version: DefinitionVersion,
    sources: BTreeMap<String, SourceName>,
    reader: R,
}

impl<R: AspectReader> DataHubCatalog<R> {
    /// Opens a catalog over a `DataHub` reader.
    ///
    /// `sources` is the deployment's mapping from a data platform to the `sources.<alias>` a model
    /// on that platform reads from. A model on a platform with no entry is refused at load (see
    /// [`DataHubError::UnknownPlatform`]) rather than guessed, which is what makes opening the model's
    /// data system the deployment's decision rather than this adapter's.
    pub const fn new(name: SourceName, version: DefinitionVersion, sources: BTreeMap<String, SourceName>, reader: R) -> Self {
        Self {
            name,
            version,
            sources,
            reader,
        }
    }

    /// The conversion: a snapshot into the two halves of a bundle.
    ///
    /// Kept as a method that takes a [`Snapshot`] so the whole decision half is testable against a
    /// reader that returns one, and a reader's only job is to produce a snapshot.
    fn assemble(&self, snapshot: &Snapshot) -> Result<Content, DataHubError> {
        let mut models = Vec::with_capacity(snapshot.datasets().len());
        for dataset in snapshot.datasets() {
            models.push(self.convert_model(dataset)?);
        }
        let mut relationships = Vec::with_capacity(snapshot.relationships().len());
        for relationship in snapshot.relationships() {
            relationships.push(Self::convert_relationship(relationship)?);
        }
        // A metric is converted only where the deployment defined the `sutura` structured property
        // that a certified metric needs. A metric without it is the promotion candidate `docs/adr/0016`
        // decision 4 describes - its raw expression string is decoded and set aside, never converted -
        // while one carrying the closed shape becomes a certified `Metric`. Both are the same entity
        // and which half applies is the deployment's declaration, not this adapter's guess.
        let mut metrics = Vec::with_capacity(snapshot.metrics().len());
        for metric in snapshot.metrics() {
            let Some(property) = metric.sutura() else {
                continue;
            };
            metrics.push(Self::convert_metric(metric, property)?);
        }

        let definitions =
            Definitions::assemble(models, relationships, metrics).map_err(|cause| DataHubError::Inconsistent { cause })?;
        // No knowledge: there is no metric for a `Referent` to name (see the crate header), so the
        // bundle carries none and the declaration agrees. The `Knowledge::assemble` call is what
        // would refuse undeclared content the day a snapshot produced any.
        let knowledge =
            Knowledge::assemble(&definitions, KnowledgeInput::none()).map_err(|cause| DataHubError::Knowledge { cause })?;
        Ok((definitions, knowledge))
    }

    /// One `dataset` aspect into a [`Model`].
    fn convert_model(&self, dataset: &document::DatasetAspect) -> Result<Model, DataHubError> {
        let source = self
            .sources
            .get(dataset.platform())
            .cloned()
            .ok_or_else(|| DataHubError::UnknownPlatform {
                model: dataset.name().to_owned(),
                platform: dataset.platform().to_owned(),
            })?;
        let name = Self::identifier(dataset.name(), |raw| ModelName::parse(raw), "model", "a dataset")?;
        let table = Self::identifier(dataset.table(), |raw| TableName::parse(raw), "table", dataset.name())?;
        let columns = dataset
            .columns()
            .iter()
            .map(|column| Self::identifier(column, |raw| ColumnName::parse(raw), "column", dataset.name()))
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
        let description = Description::parse(dataset.description()).map_err(|cause| DataHubError::Description {
            on: dataset.name().to_owned(),
            cause,
        })?;
        Ok(Model::new(name, source, table, columns, description))
    }

    /// One `SemanticModelRelationship` into a [`Relationship`].
    fn convert_relationship(relationship: &RelationshipAspect) -> Result<Relationship, DataHubError> {
        let name = Self::identifier(
            relationship.name(),
            |raw| RelationshipName::parse(raw),
            "relationship",
            "a relationship",
        )?;
        let origin_model = Self::identifier(
            relationship.origin_model(),
            |raw| ModelName::parse(raw),
            "model",
            relationship.name(),
        )?;
        let origin_column = Self::identifier(
            relationship.origin_column(),
            |raw| ColumnName::parse(raw),
            "column",
            relationship.name(),
        )?;
        let target_model = Self::identifier(
            relationship.to_model(),
            |raw| ModelName::parse(raw),
            "model",
            relationship.name(),
        )?;
        let target_column = Self::identifier(
            relationship.to_column(),
            |raw| ColumnName::parse(raw),
            "column",
            relationship.name(),
        )?;
        let join_type = match relationship.cardinality() {
            Some(Cardinality::OneOne) => JoinType::OneToOne,
            Some(Cardinality::OneN) => JoinType::OneToMany,
            Some(Cardinality::NOne) => JoinType::ManyToOne,
            Some(Cardinality::NN) => {
                return Err(DataHubError::CardinalityUnrepresentable {
                    name: relationship.name().to_owned(),
                    cardinality: "many-to-many",
                });
            }
            None => {
                return Err(DataHubError::CardinalityUnrepresentable {
                    name: relationship.name().to_owned(),
                    cardinality: "undeclared",
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

    /// One metric aspect carrying the deployment-defined [`SuturaContent`] into a certified domain
    /// [`Metric`].
    ///
    /// The measure ALREADY is the domain's own [`Measure`], and the filters, grains, dimensions and
    /// anchor are the domain's own closed vocabularies spelled as in a markdown metric - which is
    /// what makes the namespace closed: an unknown aggregate, an unknown operator, an unparseable
    /// value, a dimension unreachable through any relationship, a grainless metric or an unknown
    /// property all fail before or at `Definitions::assemble`, never guessed at. The three free
    /// strings (`model`, `time_column`, each nested `column`) are parsed here as the identifier
    /// type each claims to be, which is what turns an unparseable name into a typed
    /// [`DataHubError::Identifier`] naming the metric. `Definitions::assemble` is what finally
    /// decides the metric holds together: an unknown model, a measure or time column the model does
    /// not have, a filter whose column the model lacks, a dimension reachable only through a
    /// relationship whose cardinality is not vouched for, or no grains are all its refusals, mapped
    /// through [`DataHubError::Inconsistent`].
    fn convert_metric(metric: &document::MetricAspect, property: &document::SuturaProperty) -> Result<Metric, DataHubError> {
        let content = property.assemble().map_err(|cause| DataHubError::Sutura {
            metric: metric.name().to_owned(),
            cause,
        })?;
        let name = Self::identifier(metric.name(), |raw| MetricName::parse(raw), "metric", "a metric")?;
        let model = Self::identifier(content.model(), |raw| ModelName::parse(raw), "model", metric.name())?;
        let time_column = Self::identifier(content.time_column(), |raw| ColumnName::parse(raw), "column", metric.name())?;
        let grains = content.grains().iter().copied().collect();
        let required_filters = content.required_filters().to_vec();
        // A vector, handed on as a vector. This used to key the sequence by name and `collect`,
        // which kept the LAST of a duplicated pair in silence - the thing `SuturaContent`'s
        // `dimensions` field says a sequence exists to prevent, and the thing the markdown adapter
        // refused. `Metric::new` takes the vector now, so neither adapter can collapse the pair.
        let dimensions: Vec<Dimension> = content
            .dimensions()
            .iter()
            .cloned()
            .map(document::SuturaDimension::into_domain)
            .collect();
        let anchor = content.anchor().cloned().map(SuturaAnchor::into_domain);
        let description = Description::parse(content.description()).map_err(|cause| DataHubError::Description {
            on: metric.name().to_owned(),
            cause,
        })?;
        Metric::new(
            name,
            model,
            content.measure().clone(),
            required_filters,
            time_column,
            grains,
            dimensions,
            anchor,
            description,
        )
        .map_err(|cause| DataHubError::Inconsistent { cause })
    }

    /// Parses one identifier, mapping the domain refusal into this adapter's typed error.
    fn identifier<T>(
        raw: &str,
        parse: impl Fn(&str) -> Result<T, sutura_domain::model::InvalidIdentifier>,
        kind: &'static str,
        on: &str,
    ) -> Result<T, DataHubError> {
        parse(raw).map_err(|cause| DataHubError::Identifier {
            kind,
            value: raw.to_owned(),
            on: on.to_owned(),
            cause,
        })
    }
}

impl<R> SemanticCatalog for DataHubCatalog<R>
where
    R: AspectReader,
{
    type Error = DataHubError;

    /// A **declaring** adapter, measured against its own declaration rather than the golden oracle.
    const KIND: CatalogKind = CatalogKind::Declaring;

    /// Provides `Structure`, `Descriptions` and `Relationships` unconditionally, and the kinds that
    /// arrive from the deployment-defined `sutura` structured property
    /// (`Metrics`, `Grains`, `RequiredFilters`, `AllowedValues`, `Anchors`) **plus `Cardinality`**
    /// as **declared-and-empty may-provide kinds** - the first use of
    /// [`DefinitionCapabilities::of_may_provide`]'s 0011 *declared-and-empty* state.
    ///
    /// That distinction is the whole of this declaration, and it is why a metric-free `DataHub`
    /// deployment stays servable: whether a bundle carries any of the may-provide kinds is the
    /// deployment's decision (it defined the namespace or it did not), so absence is faithful rather
    /// than an aspirational declaration - `checked_against`'s `Unprovided` direction exempts them -
    /// while presence is still covered by the declared half. `Cardinality` is among them because it
    /// is observed only as *a dimension reached through a relationship*, which happens exactly when
    /// a deployment declares a dimension with a `via`; a bundle whose dimensions are all local
    /// carries none, lawfully.
    ///
    /// A deployment that defined no metric content therefore loads a bundle with models, prose and
    /// joins and no metrics, which is `docs/adr/0016` decision 3's narrow deployment rather than a
    /// boot failure.
    ///
    /// Written as `of([..])` plus `and_may_provide([..])` with two explicit lists, the way an
    /// adapter over a fixed external schema must, so a tenth definition kind or a fifth knowledge
    /// capability leaves this declaration alone rather than silently widening it. Why the knowledge
    /// half is empty is the crate header's subject.
    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::of(
            DefinitionCapabilities::of([
                DefinitionKind::Structure,
                DefinitionKind::Descriptions,
                DefinitionKind::Relationships,
            ])
            .and_may_provide([
                DefinitionKind::Cardinality,
                DefinitionKind::Metrics,
                DefinitionKind::Grains,
                DefinitionKind::RequiredFilters,
                DefinitionKind::AllowedValues,
                DefinitionKind::Anchors,
            ]),
            KnowledgeCapabilities::none(),
        )
    }

    fn load(&self) -> Result<PinnedDefinitions, Self::Error> {
        let snapshot = self.reader.read()?;
        let (definitions, knowledge) = self.assemble(&snapshot)?;
        PinnedDefinitions::pin(
            self.version.clone(),
            definitions,
            knowledge,
            ContributionManifest::single(self.name.clone(), Contribution::of(<Self as SemanticCatalog>::capabilities())),
        )
        .map_err(|cause| DataHubError::Digest { cause })
    }
}

#[cfg(test)]
mod tests;
