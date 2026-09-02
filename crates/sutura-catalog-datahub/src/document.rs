//! The ingestion shape a [`super::AspectReader`] returns: what this adapter needs from `DataHub`.
//!
//! **These are not literally `DataHub`'s wire documents, and saying which is the point.** `DataHub`
//! serves aspects inside an `OpenAPI` v3 envelope with more fields than any one consumer cares about,
//! and a document that claims to be that envelope while refusing every field it does not name would
//! be a false promise the moment a real response arrived. So the shapes here are the adapter's own
//! canonical statement of the aspect CONTENT a reader must extract and decode - the decision side of
//! `docs/adr/0016`'s transport note - and `deny_unknown_fields` holds over THIS shape and over the
//! recorded fixtures a reader decodes, rather than over `DataHub`'s envelope. A real HTTP reader maps
//! the service's document into one of these, exactly as `sutura-exec-bigquery`'s transport decodes
//! into that crate's own `wire::document` shapes.
//!
//! The fields track the aspects `docs/adr/0016` measured: a `dataset`'s `schemaMetadata` and its
//! description aspects, a `semanticModel`'s relationships with their cardinality, and a `metric`'s
//! expression. Everything an adapter DECIDES below this shape is tested against a fake reader that
//! serves recorded documents, which is the port's own rule.
//!
//! `Cardinality` mirrors `DataHub`'s `ERModelRelationshipCardinality`, and `N_1`-to-`N_1` is refused by
//! the conversion rather than carried, because `JoinType` has no many-to-many shape - the declaration
//! says so (this adapter does NOT provide cardinality), and a relationship nobody vouched for
//! licenses nothing.
//!
//! The fields are private with constructors and accessors, per the workspace's `check-boundaries`
//! rule that a library crate's types are its contract: a `pub` field lets a struct literal build a
//! value the constructor would have rejected. These carriers are decoded by `serde`, which writes
//! private fields, and built by a reader through the constructor.

use serde::Deserialize;

/// Everything a reader fetched, before any of it is converted.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Snapshot {
    datasets: Vec<DatasetAspect>,
    relationships: Vec<RelationshipAspect>,
    /// Metric entities are read and reported, never converted - see [`super::DataHubCatalog`].
    metrics: Vec<MetricAspect>,
}

impl Snapshot {
    /// A snapshot assembled from the three kinds of aspect a reader fetched.
    pub const fn new(datasets: Vec<DatasetAspect>, relationships: Vec<RelationshipAspect>, metrics: Vec<MetricAspect>) -> Self {
        Self {
            datasets,
            relationships,
            metrics,
        }
    }

    #[inline]
    pub fn datasets(&self) -> &[DatasetAspect] {
        &self.datasets
    }

    #[inline]
    pub fn relationships(&self) -> &[RelationshipAspect] {
        &self.relationships
    }

    #[inline]
    pub fn metrics(&self) -> &[MetricAspect] {
        &self.metrics
    }
}

/// What a `dataset` entity supplies a model: a table, its columns, the platform it lives on, and a
/// description.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetAspect {
    name: String,
    table: String,
    platform: String,
    columns: Vec<String>,
    description: String,
}

impl DatasetAspect {
    /// A dataset aspect.
    pub const fn new(name: String, table: String, platform: String, columns: Vec<String>, description: String) -> Self {
        Self {
            name,
            table,
            platform,
            columns,
            description,
        }
    }

    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[inline]
    pub fn table(&self) -> &str {
        &self.table
    }

    #[inline]
    pub fn platform(&self) -> &str {
        &self.platform
    }

    #[inline]
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    #[inline]
    pub fn description(&self) -> &str {
        &self.description
    }
}

/// What one `SemanticModelRelationship` supplies: the two endpoints and their columns, plus a
/// cardinality this adapter may or may not be able to represent.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipAspect {
    name: String,
    from_model: String,
    from_column: String,
    to_model: String,
    to_column: String,
    /// `None` is "not declared", which is not the same as a declared value - see the enum.
    cardinality: Option<Cardinality>,
}

impl RelationshipAspect {
    /// A `SemanticModelRelationship` aspect.
    pub const fn new(
        name: String,
        from_model: String,
        from_column: String,
        to_model: String,
        to_column: String,
        cardinality: Option<Cardinality>,
    ) -> Self {
        Self {
            name,
            from_model,
            from_column,
            to_model,
            to_column,
            cardinality,
        }
    }

    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[inline]
    pub fn origin_model(&self) -> &str {
        &self.from_model
    }

    #[inline]
    pub fn origin_column(&self) -> &str {
        &self.from_column
    }

    #[inline]
    pub fn to_model(&self) -> &str {
        &self.to_model
    }

    #[inline]
    pub fn to_column(&self) -> &str {
        &self.to_column
    }

    #[inline]
    pub const fn cardinality(&self) -> Option<Cardinality> {
        self.cardinality
    }
}

/// `DataHub`'s `ERModelRelationshipCardinality`, as a closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cardinality {
    OneOne,
    OneN,
    NOne,
    /// Many-to-many. Refused by the conversion: there is no `JoinType` for `N_1`-to-`N_1`, and a
    /// many-to-many relationship licenses no join.
    NN,
}

/// What a `metric` entity's `MetricInfo.expression` carries: a raw string in a dialect `DataHub`
/// names. Never converted into a `Measure`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricAspect {
    name: String,
    dialect: String,
    expression: String,
}

impl MetricAspect {
    /// A metric aspect, whose measure is a raw expression string.
    pub const fn new(name: String, dialect: String, expression: String) -> Self {
        Self {
            name,
            dialect,
            expression,
        }
    }

    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[inline]
    pub fn dialect(&self) -> &str {
        &self.dialect
    }

    #[inline]
    pub fn expression(&self) -> &str {
        &self.expression
    }
}
