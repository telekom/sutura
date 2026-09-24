//! The ingestion shape a [`super::SnapshotReader`] returns: what this adapter needs from `OpenMetadata`.
//!
//! **These are not literally `OpenMetadata`'s wire documents, and saying which is the point.**
//! `OpenMetadata` serves its entities inside a `REST` envelope with more fields than any one consumer
//! cares about, and a document that claimed to be that envelope while refusing every field it did not
//! name would be a false promise the moment a real response arrived. So the shapes here are the
//! adapter's own canonical statement of the entity CONTENT a reader must extract and decode - the
//! decision side of `docs/what-openmetadata-can-carry.md`'s transport note - and `deny_unknown_fields`
//! holds over THIS shape and over the recorded fixtures a reader decodes, rather than over
//! `OpenMetadata`'s envelope. A real HTTP reader maps the service's document into one of these.
//!
//! The fields track the entities `docs/what-openmetadata-can-carry.md` measured: a `Table` and its
//! `columns[]`, a `tableConstraint` / `foreignKey` with a `relationshipType` whose cardinality is
//! named when present and silent when not, and a `Metric` whose `metricType` is decidable but whose
//! bound column is not resolvable from its free-text expression. Everything an adapter DECIDES below
//! this shape is tested against a fake reader that serves recorded documents, which is the port's own
//! rule.
//!
//! `RelationshipType` mirrors `OpenMetadata`'s cardinality enumeration: `ONE_TO_ONE`, `MANY_TO_ONE`,
//! `ONE_TO_MANY` are non-duplicating and license a `JoinType`, while `MANY_TO_MANY` - or a silent
//! absence - licences nothing and is refused by the conversion, because `JoinType` has no
//! many-to-many shape and a relationship nobody vouched for licenses no join. The fields are private
//! with accessors, per the workspace's `check-boundaries` rule that a library crate's types are its
//! contract; an untyped value (a `name`, a `column`, a `service`) is parsed during the conversion,
//! not in these carriers.

use std::collections::BTreeMap;

use serde::Deserialize;

/// Everything a reader fetched, before any of it is converted.
///
/// `deny_unknown_fields` here too - the three entity groups are the whole of what a reader must
/// extract, and a snapshot carrying a fourth is a reader this adapter has not been told to expect.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    tables: Vec<Table>,
    relationships: BTreeMap<String, StructuralRelationship>,
    metrics: Vec<Metric>,
}

impl Snapshot {
    /// The tables this snapshot carries.
    pub fn tables(&self) -> &[Table] {
        &self.tables
    }

    /// The declared relationships, by name.
    ///
    /// Iterated rather than indexed, so a caller cannot name a relationship the snapshot does not
    /// carry: the pair comes from one place.
    pub fn relationships(&self) -> impl Iterator<Item = (&str, &StructuralRelationship)> {
        self.relationships
            .iter()
            .map(|(name, relationship)| (name.as_str(), relationship))
    }

    /// How many declared relationships the snapshot carries.
    pub fn relationship_count(&self) -> usize {
        self.relationships.len()
    }

    /// The metrics this snapshot carries.
    ///
    /// Read so the reported-not-defined cell can prove a metric a snapshot carries never becomes a
    /// domain `Metric`.
    pub fn metrics(&self) -> &[Metric] {
        &self.metrics
    }
}

/// What `OpenMetadata`'s `Column` schema COULD supply a reader beyond a column's name: its
/// `dataType`, its own `description`, and (via [`Table::primary_key`]) whether its `constraint` is
/// `PRIMARY_KEY`. This adapter's own canonical shape for it - no [`SnapshotReader`] but the fixture
/// and a test stub exists today, so nothing yet maps a real `constraint` value into
/// [`Table::primary_key`]; see the crate header's "What is built here, and what is NOT".
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnMetadata {
    #[serde(default)]
    data_type: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

impl ColumnMetadata {
    pub const fn new(data_type: Option<String>, description: Option<String>) -> Self {
        Self { data_type, description }
    }

    #[inline]
    pub fn data_type(&self) -> Option<&str> {
        self.data_type.as_deref()
    }

    #[inline]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

/// What a `Table` entity supplies a model: a table, its columns, the service it lives on, and a
/// description.
///
/// `column_metadata` and `primary_key` are both `#[serde(default)]`, so a recorded document that
/// predates either still deserializes - the same reason every field here has no `pub` constructor:
/// a document arrives only through `Deserialize`, and a struct literal would let a caller build a
/// `Table` the load path never checked.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    /// The service/database the table lives on, answered by the deployment's `sources` mapping.
    service: String,
    /// The table (and model) name.
    name: String,
    /// The physical column set.
    columns: Vec<String>,
    /// Free-text prose about the table, if any.
    description: Option<String>,
    /// Per-column `dataType`/`description`, keyed by column name.
    #[serde(default)]
    column_metadata: BTreeMap<String, ColumnMetadata>,
    /// Columns a reader found with `constraint: PRIMARY_KEY` - evidence only, and, as of this
    /// writing, populated by nothing but the recorded fixture and a test stub.
    #[serde(default)]
    primary_key: Vec<String>,
}

impl Table {
    /// The service this table lives on.
    pub fn service(&self) -> &str {
        &self.service
    }

    /// The table (and model) name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The physical column set.
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// Free-text prose about the table, if any.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// One column's `dataType`/`description` evidence, by name.
    pub fn column_metadata(&self, column: &str) -> Option<&ColumnMetadata> {
        self.column_metadata.get(column)
    }

    /// Which columns carry a `PRIMARY_KEY` constraint.
    pub fn primary_key(&self) -> &[String] {
        &self.primary_key
    }
}

/// `OpenMetadata`'s relationship cardinality, as a closed set.
///
/// Named when present and silent when not; `ManyToMany` is refused by the conversion because
/// `JoinType` has no many-to-many shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RelationshipType {
    OneToOne,
    ManyToOne,
    OneToMany,
    ManyToMany,
}

/// One declared relationship's structural endpoints and its cardinality.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StructuralRelationship {
    origin_model: String,
    origin_column: String,
    target_model: String,
    target_column: String,
    #[serde(default)]
    relationship_type: Option<RelationshipType>,
}

impl StructuralRelationship {
    /// The model the join starts from.
    pub fn origin_model(&self) -> &str {
        &self.origin_model
    }

    /// The column of the origin model the join is on.
    pub fn origin_column(&self) -> &str {
        &self.origin_column
    }

    /// The model the join reaches.
    pub fn target_model(&self) -> &str {
        &self.target_model
    }

    /// The column of the target model the join is on.
    pub fn target_column(&self) -> &str {
        &self.target_column
    }

    /// The cardinality, named when `OpenMetadata` declares it and silent when it does not.
    pub const fn relationship_type(&self) -> Option<RelationshipType> {
        self.relationship_type
    }
}

/// What a `Metric` entity supplies.
///
/// **Deliberately minimal and deliberately un-analysed at conversion.** `metricType` is decidable
/// but the bound column is not resolvable from `metricExpression` / `measures[].expression` - free
/// text in a foreign dialect - and a measure carried as such is reported and not defined. The metric
/// is decoded (so the reported-not-defined cell can prove it is carried and ignored) and never
/// minted into a domain `Metric`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metric {
    name: String,
    #[serde(rename = "metricType")]
    aggregation: String,
    #[serde(default)]
    granularity: Option<String>,
    #[serde(default)]
    expression: Option<String>,
}

impl Metric {
    /// The metric's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The decidable aggregation-kind word, e.g. `SUM`.
    pub fn aggregation(&self) -> &str {
        &self.aggregation
    }

    /// The declared granularity, e.g. `DAY`, if any.
    pub fn granularity(&self) -> Option<&str> {
        self.granularity.as_deref()
    }

    /// The raw expression text a deployment wrote to bind the measure to a column.
    pub fn expression(&self) -> Option<&str> {
        self.expression.as_deref()
    }
}
