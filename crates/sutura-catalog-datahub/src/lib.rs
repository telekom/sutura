//! A [`SemanticCatalog`] over `DataHub`'s entity aspects: the canonical **declaring** source.
//!
//! `docs/adr/0016-what-datahub-can-carry.md` is the measurement that decides what this adapter is.
//! `DataHub` 1.7.0 holds a measure as a raw expression string in a dialect set that does not intersect
//! this repository's, and its physical relationships default cardinality to many-to-many - so it
//! supplies the physical model, the descriptions and the join columns, and supplies no measure this
//! adapter will execute, no reliable cardinality, no definitional filter, no grain, no value
//! allowlist and no anchor. That is the shape of a **declaring** adapter: it says which kinds it
//! provides and which it does not, and it is measured against that declaration rather than against
//! the golden adapters' oracle.
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
//! implementor of the port is the recorded fixture source in [`fixture`].
//!
//! # The declaration, and what it means for the bundle
//!
//! [`SemanticCatalog::KIND`] is [`CatalogKind::Declaring`] and [`SemanticCatalog::capabilities`]
//! provides `Structure`, `Descriptions` and `Relationships`, and no knowledge capability. A bundle a
//! `DataHub`-only deployment loads therefore uses the physical model, the prose and the join columns,
//! and carries no certified metric layer - which the prompt states as a fact derived from the bundle,
//! exactly as `docs/adr/0011` decided. It loads and validates, because `Definitions::assemble` has no
//! minimum-metric refusal.
//!
//! **The knowledge half is empty on purpose, and why is worth stating rather than glossed.** `DataHub`
//! does have glossary-like content, but every `Knowledge` referent names a metric, a dimension of one
//! or a declared value of one - and this adapter provides no metrics, so there is no referent for a
//! phrase or a caveat to attach to. `docs/adr/0016` decision 3 marks glossary and caveats *provides,
//! conditionally, where the bundle already declares a metric for a Referent to name*; that condition
//! is met only by the composition of a separately-authored metric layer, which `docs/adr/0011`'s
//! assembler (not built) is what would supply. So the honest standalone declaration is no knowledge
//! at all, and this crate says so rather than advertising a conditional it cannot satisfy alone.
//!
//! # The two absences that are the point
//!
//! `Cardinality` and `Metrics` are both *present* in `DataHub` and both declared unsupported here,
//! which is the interesting kind of absence `docs/adr/0016` calls out: reading them would be reading
//! something the source does not guarantee. A relationship whose cardinality is absent or many-to-many
//! is refused naming the relationship (not defaulted in either direction), because the `N_N` default
//! makes an unconsidered relationship indistinguishable from a considered one. A metric whose measure
//! is an expression string is read - [`document::MetricAspect`] is decoded - and never converted into
//! a `Measure`, so it does not enter the certified bundle.

pub mod document;
pub mod fixture;

use std::collections::BTreeMap;

use sutura_domain::capabilities::{DefinitionCapabilities, DefinitionKind, MetadataCapabilities};
use sutura_domain::catalog::{Definitions, Description, InconsistentDefinitions, InvalidDescription, Model, Relationship};
use sutura_domain::definitions::NotDigestible;
use sutura_domain::knowledge::KnowledgeCapabilities;
use sutura_domain::knowledge::{InconsistentKnowledge, Knowledge, KnowledgeInput};
use sutura_domain::model::{ColumnName, JoinType, ModelName, RelationshipName, SourceName, TableName};
use sutura_domain::pinned::{
    CatalogKind, Contribution, ContributionManifest, DefinitionVersion, PinnedDefinitions, SemanticCatalog,
};

use crate::document::{Cardinality, RelationshipAspect, Snapshot};

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
        // Metrics are read (the snapshot decodes `MetricAspect`) and never converted into domain
        // `Metric`s: this adapter declares it provides no metrics, so there is no `Measure` to build
        // here and the raw expression string must not become one. `docs/adr/0016` decision 4.

        let definitions =
            Definitions::assemble(models, relationships, Vec::new()).map_err(|cause| DataHubError::Inconsistent { cause })?;
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

    /// Provides `Structure`, `Descriptions` and `Relationships`, and no knowledge capability.
    ///
    /// Written as `of([..])` with two explicit lists, the way an adapter over a fixed external schema
    /// must, so a tenth definition kind or a fifth knowledge capability leaves this declaration alone
    /// rather than silently widening it. Why the knowledge half is empty is the crate header's
    /// subject; why `Cardinality` and `Metrics` are absent despite `DataHub` having the fields is
    /// `docs/adr/0016`'s.
    fn capabilities() -> MetadataCapabilities {
        MetadataCapabilities::of(
            DefinitionCapabilities::of([
                DefinitionKind::Structure,
                DefinitionKind::Descriptions,
                DefinitionKind::Relationships,
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
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use sutura_domain::capabilities::{DeclarableKind, MetadataCapabilities};
    use sutura_domain::catalog::{Definitions, Description, Metric, Model};
    use sutura_domain::knowledge::{
        Capability, GlossaryEntry, InconsistentKnowledge, Knowledge, KnowledgeCapabilities, KnowledgeInput, NoteBody, Phrase,
        Referent,
    };
    use sutura_domain::measure::{AggregatedColumn, Measure, Term};
    use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, ModelName, SourceName, TableName};
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog};

    use crate::document::{Cardinality, DatasetAspect, MetricAspect, RelationshipAspect, Snapshot};
    use crate::{AspectReader, DataHubCatalog, DataHubError};

    fn name() -> SourceName {
        SourceName::parse("local").expect("a test name is a name")
    }

    fn version() -> DefinitionVersion {
        DefinitionVersion::parse("test").expect("a test version is a version")
    }

    /// A reader that serves exactly the snapshot a test hands it.
    #[derive(Debug, Clone)]
    struct Stub(Snapshot);

    impl AspectReader for Stub {
        fn read(&self) -> Result<Snapshot, DataHubError> {
            Ok(self.0.clone())
        }
    }

    fn over(snapshot: Snapshot) -> DataHubCatalog<Stub> {
        let mut sources = BTreeMap::new();
        drop(sources.insert(String::from("bigquery"), name()));
        DataHubCatalog::new(name(), version(), sources, Stub(snapshot))
    }

    fn dataset(name: &str, table: &str, columns: &[&str], description: &str) -> DatasetAspect {
        DatasetAspect::new(
            name.to_owned(),
            table.to_owned(),
            String::from("bigquery"),
            columns.iter().map(|c| String::from(*c)).collect(),
            description.to_owned(),
        )
    }

    /// The two models and one relationship the recorded corpus carries.
    fn corpus() -> Snapshot {
        Snapshot::new(
            vec![
                dataset(
                    "orders",
                    "fct_order",
                    &["order_id", "customer_id", "amount_cents", "order_date"],
                    "Net revenue orders, in minor units.",
                ),
                dataset(
                    "customers",
                    "dim_customer",
                    &["customer_id", "segment"],
                    "The customer dimension.",
                ),
            ],
            vec![RelationshipAspect::new(
                String::from("orders_to_customer"),
                String::from("orders"),
                String::from("customer_id"),
                String::from("customers"),
                String::from("customer_id"),
                Some(Cardinality::NOne),
            )],
            Vec::new(),
        )
    }

    /// THE requirement, stated first because it is the reason this adapter works alone at all.
    ///
    /// A DataHub-only deployment reads models, descriptions and joins and no certified metric layer,
    /// and that must LOAD: `Definitions::assemble` has no minimum-metric refusal, so a bundle of
    /// models with zero metrics assembles, pins and validates. `docs/adr/0016` decision 3.
    #[test]
    fn a_bundle_of_models_and_no_metrics_loads_and_validates() {
        let pinned = over(corpus()).load().expect("the recorded corpus loads");
        assert_eq!(pinned.definitions().models().len(), 2);
        assert_eq!(pinned.definitions().relationships().len(), 1);
        assert!(
            pinned.definitions().metrics().is_empty(),
            "no metrics in a DataHub-only bundle"
        );
    }

    /// Declaration fidelity, the metrics half.
    ///
    /// `MetadataCapabilities::produced` reads what the bundle actually carries, and
    /// `checked_against` compares it in BOTH directions to what the adapter declared. This adapter
    /// declares no `Metrics`, and the bundle carries none - so the pair agrees, and a declaration
    /// that (wrongly) claimed metrics would be reported as `Unprovided`.
    #[test]
    fn it_declares_it_provides_no_metrics_and_the_bundle_has_none() {
        let catalog = over(corpus());
        let pinned = catalog.load().expect("the recorded corpus loads");
        let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());
        assert_eq!(
            <DataHubCatalog<Stub> as SemanticCatalog>::capabilities().checked_against(&produced),
            Ok(())
        );
        assert!(!produced.declares(DeclarableKind::Definition(
            sutura_domain::capabilities::DefinitionKind::Metrics
        )));
    }

    /// A metric whose measure is a raw expression string is READ and never DEFINED.
    ///
    /// `docs/adr/0016` decision 4: `MetricInfo.expression` is a promotion candidate, a string in a
    /// dialect nothing here renders, and `aggregationFunction` beside it is authored independently
    /// with nothing reconciling the two - so taking either would certify half a definition. The
    /// adapter decodes the metric aspect (it is present in the snapshot) and does not convert it
    /// into a domain `Metric`, which is exactly what `provides no metrics` means. A real instance
    /// full of metrics therefore still loads into a bundle that declares none.
    #[test]
    fn a_metric_whose_measure_is_an_expression_string_is_reported_and_not_defined() {
        let corpus = corpus();
        let snapshot = Snapshot::new(
            corpus.datasets().to_vec(),
            corpus.relationships().to_vec(),
            vec![MetricAspect::new(
                String::from("revenue"),
                String::from("ANSI_SQL"),
                String::from("SUM(amount_cents)"),
            )],
        );
        let pinned = over(snapshot).load().expect("a metric entity does not stop a load");
        assert!(pinned.definitions().metrics().is_empty(), "the metric is not defined");
    }

    /// A relationship licenses no dimension, because this adapter provides no cardinality.
    ///
    /// `DataHub` relationships carry a cardinality that this adapter declines (the `N_N` default makes
    /// an unconsidered relationship indistinguishable from a considered one), and the mere presence
    /// of a relationship here - even one mapped to a representable `JoinType` - produces no
    /// `Cardinality` capability, because a capability is observed only as *a dimension reached
    /// through the relationship*, and this adapter never creates one. Both halves are pinned: the
    /// fixture bundle carries the relationship and the declaration still holds, and a relationship
    /// whose cardinality is absent or many-to-many is refused naming it (`docs/adr/0016` decision 5).
    /// A snapshot whose one relationship carries a given cardinality.
    fn relationship_with(cardinality: Option<Cardinality>) -> Snapshot {
        let corpus = corpus();
        Snapshot::new(
            corpus.datasets().to_vec(),
            vec![RelationshipAspect::new(
                String::from("orders_to_customer"),
                String::from("orders"),
                String::from("customer_id"),
                String::from("customers"),
                String::from("customer_id"),
                cardinality,
            )],
            Vec::new(),
        )
    }

    #[test]
    fn it_declares_it_provides_no_cardinality_so_a_relationship_licenses_no_dimension() {
        let pinned = over(corpus()).load().expect("the recorded corpus loads");
        let produced = MetadataCapabilities::produced(pinned.definitions(), pinned.knowledge());
        assert!(
            !produced.declares(DeclarableKind::Definition(
                sutura_domain::capabilities::DefinitionKind::Cardinality
            )),
            "a relationship may exist without licensing a dimension"
        );

        let refused = over(relationship_with(Some(Cardinality::NN)))
            .load()
            .expect_err("a many-to-many relationship is content this adapter does not provide");
        assert!(
            matches!(
                refused,
                DataHubError::CardinalityUnrepresentable { ref name, cardinality: "many-to-many" } if name == "orders_to_customer"
            ),
            "{refused:?}"
        );

        let refused = over(relationship_with(None))
            .load()
            .expect_err("an undeclared cardinality is refused, not defaulted");
        assert!(
            matches!(
                refused,
                DataHubError::CardinalityUnrepresentable { ref name, cardinality: "undeclared" } if name == "orders_to_customer"
            ),
            "{refused:?}"
        );
    }

    /// Content for a knowledge kind the adapter did not declare fails the load.
    ///
    /// `Knowledge::assemble`'s `UndeclaredContent` guard refuses a bundle whose input carries notes
    /// for a capability the declared `KnowledgeCapabilities` do not cover - the "content for a kind
    /// it did not declare" shape. A standalone `DataHub` bundle never reaches it, because a note
    /// needs a metric for its `Referent` to name and this adapter provides no metrics; the wiring is
    /// the point here. Built and refused through the adapter's own types, so the guard is reachable
    /// the day a composed bundle feeds one in, and the failure lands as `DataHubError::Knowledge`
    /// rather than as prose.
    #[test]
    fn content_for_a_kind_it_did_not_declare_fails_the_load() {
        let model = Model::new(
            ModelName::parse("orders").expect("a model name is a name"),
            name(),
            TableName::parse("fct_order").expect("a table name is a name"),
            [
                ColumnName::parse("amount_cents").expect("a column is a name"),
                ColumnName::parse("order_date").expect("a column is a name"),
            ]
            .into_iter()
            .collect(),
            Description::parse("Net revenue orders.").expect("a description is a description"),
        );
        let metric = Metric::new(
            MetricName::parse("revenue").expect("a metric name is a name"),
            ModelName::parse("orders").expect("a model name is a name"),
            Measure::Simple(Term::Aggregate(AggregatedColumn::new(
                Aggregate::Sum,
                ColumnName::parse("amount_cents").expect("a column is a name"),
            ))),
            Vec::new(),
            ColumnName::parse("order_date").expect("a column is a name"),
            std::iter::once(Grain::Month).collect(),
            BTreeMap::new(),
            None,
            Description::parse("").expect("empty is a description"),
        );
        let definitions =
            Definitions::assemble(vec![model], Vec::new(), vec![metric]).expect("a model and a metric hold together");
        let entry = GlossaryEntry::new(
            Phrase::parse("revenue").expect("a phrase is a phrase"),
            BTreeSet::new(),
            Referent::Metric {
                metric: MetricName::parse("revenue").expect("a metric name is a name"),
            },
            NoteBody::parse("Net revenue in minor units.").expect("a note body is a body"),
        );

        let refused = Knowledge::assemble(
            &definitions,
            KnowledgeInput::new(KnowledgeCapabilities::none(), vec![entry], Vec::new(), Vec::new(), Vec::new()),
        )
        .expect_err("a glossary under a declaration that provides none is undeclared content");
        assert!(
            matches!(
                refused,
                InconsistentKnowledge::UndeclaredContent {
                    capability: Capability::Glossary,
                    ..
                }
            ),
            "{refused:?}"
        );
        // The adapter's typed error carries it, proving the load path maps it rather than swallowing
        // it.
        let _: DataHubError = DataHubError::Knowledge { cause: refused };
    }
}
