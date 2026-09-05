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
//! `Cardinality` mirrors `DataHub`'s `ERModelRelationshipCardinality` (declared-and-empty in this
//! adapter, produced only by a deployment-defined dimension with a `via`), and `N_1`-to-`N_1` is
//! refused by the conversion rather than carried, because `JoinType` has no many-to-many shape - a
//! relationship nobody vouched for licenses nothing.
//!
//! The fields are private with constructors and accessors, per the workspace's `check-boundaries`
//! rule that a library crate's types are its contract. The guards are not in the constructors -
//! these are carriers of already-typed values, and where a value is untyped (a `column`, a
//! `model`) it is parsed during the conversion, not here - they are in the typed serde fields and
//! the two closedness claims (an unknown key is refused by `deny_unknown_fields`, a value that is
//! not a usable identifier is refused by the conversion's parse) that a `pub` field would let a
//! struct literal walk past.

use serde::Deserialize;

use sutura_domain::calendar::TimeRange;
use sutura_domain::catalog::{Anchor, AnchorValue, Description, Dimension, DimensionValue};
use sutura_domain::measure::{Measure, RequiredFilter};
use sutura_domain::model::{ColumnName, DimensionName, Grain, RelationshipName};

/// Everything a reader fetched, before any of it is converted.
///
/// `deny_unknown_fields` here too - the three aspect groups are the whole of what a reader must
/// extract, and a snapshot carrying a fourth is a reader this adapter has not been told to expect,
/// which is exactly the silent-acceptance the attribute exists to refuse on the nested shapes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
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
/// names.
///
/// A metric that also carries the deployment-defined [`SuturaContent`] becomes a **certified**
/// metric here; one that does not stays the promotion candidate whose measure is `expression` and
/// is never converted. The two are the same entity and the distinction is an `Option` because which
/// one a given metric is depends on what the deployment defined, not on anything this adapter
/// decides.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricAspect {
    name: String,
    dialect: String,
    expression: String,
    /// The deployment-defined certified content, in the shape `DataHub` can actually store: one
    /// scalar structured property. `None` for a metric that carries no custom shape.
    sutura: Option<SuturaProperty>,
}

impl MetricAspect {
    /// A promotion-candidate metric aspect, whose measure is a raw expression string.
    pub const fn new(name: String, dialect: String, expression: String) -> Self {
        Self {
            name,
            dialect,
            expression,
            sutura: None,
        }
    }

    /// A certified metric aspect: the expression string beside a deployment-defined content.
    ///
    /// The two are both carried because `docs/adr/0016`'s *reconcile, never assume* rule still
    /// applies - the raw expression is a promotion candidate's other half and remains readable even
    /// where the structured property is what this adapter certifies. The content is serialized into
    /// the scalar form [`SuturaProperty`] stores, which is the shape the wire and the recorded
    /// fixtures carry.
    #[expect(
        clippy::expect_used,
        reason = "serializing a value of plain serde data cannot fail; keep in hand so a malformed \
                  serial output would be a visible panic rather than a silently embedded document"
    )]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "a constructor takes its content by value; serializing then discarding it is the shape \
                  that releases the caller's owned content rather than borrowing it"
    )]
    pub fn with_sutura(name: String, dialect: String, expression: String, sutura: SuturaContent) -> Self {
        let json = serde_json::to_string(&sutura).expect("a metric's certified content is plain data and always serializes");
        Self {
            name,
            dialect,
            expression,
            sutura: Some(SuturaProperty::new(json)),
        }
    }

    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The promotion candidate's other half - the raw expression string in a dialect nothing here
    /// renders.
    ///
    /// No consumer today, and `docs/adr/0016` says so rather than pretending otherwise: the aspect
    /// is decoded and a metric without the `sutura` property is set aside, never converted, and its
    /// expression is not re-read. These accessors and [`Self::dialect`] are the readable shape a
    /// future reporter would use.
    #[inline]
    pub fn expression(&self) -> &str {
        &self.expression
    }

    /// The dialect the raw expression string is written in.
    ///
    /// Part of the promotion candidate's other half; see [`Self::expression`] for why nothing reads
    /// it yet.
    #[inline]
    pub fn dialect(&self) -> &str {
        &self.dialect
    }

    /// The deployment-defined certified content, when the metric carries it.
    #[inline]
    pub const fn sutura(&self) -> Option<&SuturaProperty> {
        self.sutura.as_ref()
    }
}

/// `DataHub`'s view of the deployment-defined metric content: ONE structured property, whose single
/// scalar value is the JSON document below.
///
/// **This is what makes the scalar-only constraint literal rather than prose.** `DataHub`'s
/// `structuredProperty` has no nested or record value type, so a deployment cannot define a nested
/// object at all; what it can define is one string-valued property, and [`Self::assemble`] is the
/// step that turns that scalar into the nested [`SuturaContent`] - the issue #202 mechanism,
/// implemented here and exercised by a fixture recorded in this flat form rather than left to a
/// sentence.
///
/// **The property's NAME is the deployment's and does not appear here.** `sutura` is the field
/// [`MetricAspect`] carries this under on the adapter's own canonical shape; which structured
/// property a reader maps onto it is `docs/adr/0016` decision 7's *not ours to say*, and
/// `tests/provisioned.rs` registers one whose name shares nothing with this field precisely so the
/// independence is measured.
///
/// The scalar payload is bounded by the value-type limits a deployment's `DataHub` enforces - the
/// platform names its own as `structuredProperties.keywordMaxLength`, because the value is indexed
/// as an Elasticsearch keyword, so the bound is an index setting rather than a constant here.
/// What is measured is that the refusal NAMES that setting; nothing has raised it and retried,
/// so whether a deployment can move it is `DataHub`'s documentation and not this repository's
/// measurement. This crate adds no bound of its own.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuturaProperty {
    /// The scalar value of the deployment's structured property: the metric content as JSON text.
    string_value: String,
}

impl SuturaProperty {
    /// A structured property whose string value is `json`.
    pub const fn new(string_value: String) -> Self {
        Self { string_value }
    }

    /// The deployment-defined metric content, decoded from the scalar.
    ///
    /// The one place `DataHub`'s scalar store becomes this adapter's nested statement of what the
    /// deployment meant. A malformed value - which cannot arise from `with_sutura`, but can from
    /// any wire - is a refusal naming nothing, which is why the conversion maps it into a typed
    /// `DataHubError` rather than letting it fall through as a serde string.
    pub fn assemble(&self) -> Result<SuturaContent, serde_json::Error> {
        serde_json::from_str(&self.string_value)
    }

    #[inline]
    pub fn string_value(&self) -> &str {
        &self.string_value
    }
}

/// What a metric needs to be certified, in the JSON document a deployment's `sutura` property
/// carries.
///
/// [`SuturaProperty`] is why the deployment's view is one scalar and this is the reader's decoded
/// statement of what that scalar means.
///
/// **Everything a markdown metric can carry arrives here, over the domain's own closed
/// vocabularies.** The `measure` is the `sutura-domain` [`Measure`] type itself - the closed set,
/// written exactly as this repository writes it - the `grains` and `required_filters` are the
/// closed `Grain` and `RequiredFilter` enums, and a `dimension` and an `anchor` mirror the markdown
/// document's shapes. `deny_unknown_fields` sits on this document, on the measure and on the term
/// inside it, on a filter, on a dimension, on the anchor and on the range inside the anchor, and
/// refuses a property this adapter does not recognise rather than guessing. An aggregate out of the
/// closed set, an unknown operator, an unparseable value, an unknown grain, an unknown key at any of
/// those levels - all fail the decode before the conversion sees them, naming the key.
///
/// **The range is where that used to stop**, which is worth recording because the claim read *at
/// every depth* while it was one depth short: `sutura_domain::calendar::TimeRangeInput` carried no
/// `deny_unknown_fields`, so a key written INSIDE the range object was discarded in silence rather
/// than named, and the metric was certified from a document nobody had read in full. The attribute
/// is on that domain shape now, which closes the same hole on the markdown catalog and question
/// paths that decode the same type, and
/// `a_key_inside_an_anchor_range_is_refused_through_the_load_path` is what holds it here.
///
/// The three free strings - `model`, `time_column`, and each nested `column` - are the one thing
/// this shape cannot close, and they are parsed as domain identifier types during the conversion
/// (through the same `try_from` every catalog identifier uses) rather than at decode, which is what
/// turns an unparseable name into a typed `DataHubError::Identifier` naming the metric.
///
/// The original issue #202 scope carried the measure, the time column and the grains, and this
/// container of the rest of a metric is the closure of that scope: definitional filters, dimensions
/// with their allowlists, an anchor and prose all arrive the same way a markdown document carries
/// them, because `docs/adr/0011` closes the route by which any OTHER source could add them to a
/// metric this adapter defines. The one absence that stays is `cardinality`, which `DataHub`
/// carries but this adapter refuses to represent (see the crate header).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct SuturaContent {
    /// The model the metric measures - a `model` this snapshot also carries, so a metric naming a
    /// model that is absent is refused by `Definitions::assemble` as `UnknownModel`.
    model: String,
    /// The closed-vocabulary measure. This is what a certified metric is built from; `expression`
    /// is the promotion candidate's other half.
    measure: Measure,
    /// The column the metric is asked along.
    time_column: String,
    /// The grains the metric may be asked at. Non-empty, enforced by `Definitions::assemble`.
    grains: Vec<Grain>,
    /// Prose about the metric, exactly as a markdown metric carries it.
    #[serde(default)]
    description: String,
    /// Predicates that are part of what the metric MEANS, applied to every question about it.
    #[serde(default)]
    required_filters: Vec<RequiredFilter>,
    /// The dimensions the metric may be grouped by, named the way a markdown metric names them.
    ///
    /// A sequence rather than a map, and for the same reason the markdown document's is: a map made
    /// of two entries under one name would silently keep the last, where a sequence survives to
    /// `Definitions::assemble` to be refused.
    #[serde(default)]
    dimensions: Vec<SuturaDimension>,
    /// The number this metric is expected to produce, so the anchor mechanism can re-run it.
    #[serde(default)]
    anchor: Option<SuturaAnchor>,
}

impl SuturaContent {
    /// The certified content of a metric, with the optional shapes defaulted to absent.
    pub const fn new(model: String, measure: Measure, time_column: String, grains: Vec<Grain>) -> Self {
        Self {
            model,
            measure,
            time_column,
            grains,
            description: String::new(),
            required_filters: Vec::new(),
            dimensions: Vec::new(),
            anchor: None,
        }
    }

    /// The certified content of a metric in full, optional shapes included.
    pub const fn full(
        model: String,
        measure: Measure,
        time_column: String,
        grains: Vec<Grain>,
        description: String,
        required_filters: Vec<RequiredFilter>,
        dimensions: Vec<SuturaDimension>,
        anchor: Option<SuturaAnchor>,
    ) -> Self {
        Self {
            model,
            measure,
            time_column,
            grains,
            description,
            required_filters,
            dimensions,
            anchor,
        }
    }

    #[inline]
    pub fn model(&self) -> &str {
        &self.model
    }

    #[inline]
    pub const fn measure(&self) -> &Measure {
        &self.measure
    }

    #[inline]
    pub fn time_column(&self) -> &str {
        &self.time_column
    }

    #[inline]
    pub fn grains(&self) -> &[Grain] {
        &self.grains
    }

    #[inline]
    pub fn description(&self) -> &str {
        &self.description
    }

    #[inline]
    pub fn required_filters(&self) -> &[RequiredFilter] {
        &self.required_filters
    }

    #[inline]
    pub fn dimensions(&self) -> &[SuturaDimension] {
        &self.dimensions
    }

    /// The certified number, when the deployment declared one.
    #[inline]
    pub const fn anchor(&self) -> Option<&SuturaAnchor> {
        self.anchor.as_ref()
    }
}

/// One dimension, spelled exactly as a markdown metric's dimension is.
///
/// A list entry with its own `name`, so a deployment declaring one name twice survives to
/// `Definitions::assemble` to be refused rather than silently collapsing.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct SuturaDimension {
    name: DimensionName,
    column: ColumnName,
    #[serde(default)]
    via: Option<RelationshipName>,
    /// The values a filter may use. Absent means "group by this, do not filter on it".
    #[serde(default)]
    allowed_values: Option<std::collections::BTreeSet<DimensionValue>>,
    #[serde(default)]
    description: Description,
}

impl SuturaDimension {
    /// A dimension, in the order `Definitions::assemble` wants it.
    pub const fn new(
        name: DimensionName,
        column: ColumnName,
        via: Option<RelationshipName>,
        allowed_values: Option<std::collections::BTreeSet<DimensionValue>>,
        description: Description,
    ) -> Self {
        Self {
            name,
            column,
            via,
            allowed_values,
            description,
        }
    }

    /// Into the domain type `Definitions::assemble` holds.
    pub fn into_domain(self) -> Dimension {
        Dimension::new(self.name, self.column, self.via, self.allowed_values, self.description)
    }
}

/// The number a metric is expected to produce, spelled exactly as a markdown metric's anchor is.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct SuturaAnchor {
    range: TimeRange,
    /// The certified value, read as text whatever the deployment wrote - the anchor comparison is
    /// over text.
    ///
    /// An [`AnchorValue`] rather than a `String`, so the deployment-defined property is held to the
    /// domain's character rule where it is deserialized rather than after conversion. A property
    /// carrying an unparseable value therefore fails as a `DataHubError::Sutura` naming the metric,
    /// which is the same refusal an unparseable `DimensionValue` in the same property already gives.
    value: AnchorValue,
}

impl SuturaAnchor {
    /// An anchor.
    pub const fn new(range: TimeRange, value: AnchorValue) -> Self {
        Self { range, value }
    }

    #[inline]
    pub const fn range(&self) -> TimeRange {
        self.range
    }

    #[inline]
    pub fn value(&self) -> &str {
        self.value.as_str()
    }

    /// Into the domain type `Definitions::assemble` holds.
    pub fn into_domain(self) -> Anchor {
        Anchor::new(self.range, self.value)
    }
}
