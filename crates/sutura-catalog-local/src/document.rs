//! The on-disk shape of a catalog document, and its conversion into domain types.
//!
//! These structs exist so the file format is a separate thing from the model. A domain type with
//! `Deserialize` on it would make every rename in a catalog file a breaking change to the hexagon's
//! interior, and it would put the file format's defaults inside the types the business rules are
//! written in.
//!
//! `deny_unknown_fields` is on every one of them, and it is the most useful line in this module. A
//! misspelled key would otherwise be dropped in silence, and the definition that loads is not the
//! one the author wrote: `colums:` yields a model with no columns, which then refuses every question
//! about it for a reason that says nothing about a typo.

use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::calendar::TimeRange;
use sutura_domain::catalog::{Anchor, Dimension, Metric, Model, Relationship};
use sutura_domain::model::{
    Aggregate, ColumnName, DimensionName, Grain, JoinType, Measure, MetricName, ModelName, RelationshipName, SourceName,
    TableName,
};

/// What a document declares itself to be.
///
/// Required in every document rather than inferred from the directory it sits in. A file in the
/// wrong directory is then an error naming the mismatch, instead of a metric that was quietly never
/// loaded, and the loader can walk one tree instead of trusting a layout convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Model,
    Relationship,
    Metric,
}

impl DocumentKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Relationship => "relationship",
            Self::Metric => "metric",
        }
    }
}

/// Just enough of a document to know which shape to parse it as.
///
/// A separate pass over the same few lines. The alternative is an internally tagged enum, and serde
/// cannot combine one with `deny_unknown_fields`, which is the check that makes a typo an error. Two
/// parses of a frontmatter block is not a cost worth trading that for.
#[derive(Debug, serde::Deserialize)]
pub struct KindProbe {
    kind: DocumentKind,
}

impl KindProbe {
    /// What the document says it is.
    ///
    /// An accessor rather than a public field, because the boundary gate fails a public field on a
    /// public struct in a library crate: a struct literal can build a value a constructor would
    /// have rejected, and the rule does not get to make an exception for a type that currently has
    /// no invariant to protect.
    #[inline]
    pub const fn kind(&self) -> DocumentKind {
        self.kind
    }
}

/// A value an anchor may be written as.
///
/// Untagged so `value: 197122` and `value: "197122"` both work. Without it the unquoted form fails
/// with "invalid type: integer, expected a string", which is a true statement about a file that
/// looks correct to whoever wrote it. Everything becomes text either way, because that is what an
/// anchor comparison uses.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum AnchorLiteral {
    Integer(i64),
    Text(String),
}

impl AnchorLiteral {
    fn into_text(self) -> String {
        match self {
            Self::Integer(v) => v.to_string(),
            Self::Text(v) => v,
        }
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDoc {
    #[expect(
        dead_code,
        reason = "the tag is read by KindProbe; it is declared here so deny_unknown_fields does \
                  not reject the document it dispatched on"
    )]
    kind: DocumentKind,
    name: ModelName,
    source: SourceName,
    table: TableName,
    columns: BTreeSet<ColumnName>,
}

impl ModelDoc {
    pub fn into_domain(self, description: String) -> Model {
        Model::new(self.name, self.source, self.table, self.columns, description)
    }
}

/// One end of a relationship.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointDoc {
    model: ModelName,
    column: ColumnName,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationshipDoc {
    #[expect(
        dead_code,
        reason = "the tag is read by KindProbe; it is declared here so deny_unknown_fields does \
                  not reject the document it dispatched on"
    )]
    kind: DocumentKind,
    name: RelationshipName,
    origin: EndpointDoc,
    target: EndpointDoc,
    join_type: JoinType,
}

impl RelationshipDoc {
    pub fn into_domain(self) -> Relationship {
        Relationship::new(
            self.name,
            self.origin.model,
            self.origin.column,
            self.target.model,
            self.target.column,
            self.join_type,
        )
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeasureDoc {
    aggregate: Aggregate,
    column: ColumnName,
}

/// A dimension, as a list entry with its own `name`.
///
/// A sequence rather than a map keyed by name, and that is not a style choice. A YAML mapping with
/// the same key twice keeps the last value and reports nothing, so a metric declaring `region`
/// twice would load with whichever definition came second. As a list the duplication survives to
/// where [`MetricDoc::into_domain`] can refuse it.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DimensionDoc {
    name: DimensionName,
    column: ColumnName,
    #[serde(default)]
    via: Option<RelationshipName>,
    /// The values a filter may use. Absent means "group by this, do not filter on it".
    #[serde(default)]
    values: Option<BTreeSet<String>>,
    #[serde(default)]
    description: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorDoc {
    range: TimeRange,
    value: AnchorLiteral,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricDoc {
    #[expect(
        dead_code,
        reason = "the tag is read by KindProbe; it is declared here so deny_unknown_fields does \
                  not reject the document it dispatched on"
    )]
    kind: DocumentKind,
    name: MetricName,
    model: ModelName,
    measure: MeasureDoc,
    time_column: ColumnName,
    grains: BTreeSet<Grain>,
    #[serde(default)]
    dimensions: Vec<DimensionDoc>,
    #[serde(default)]
    anchor: Option<AnchorDoc>,
}

/// Why a metric document cannot become a metric.
///
/// Only the things [`sutura_domain::catalog::Definitions`] cannot see, because by the time it runs
/// the duplication has already been collapsed by the map it holds. Everything else is checked there,
/// once, for every adapter.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InvalidMetricDocument {
    #[error("metric {metric} declares dimension {dimension} twice")]
    DuplicateDimension { metric: MetricName, dimension: DimensionName },
}

impl MetricDoc {
    pub fn into_domain(self, description: String) -> Result<Metric, InvalidMetricDocument> {
        let mut dimensions: BTreeMap<DimensionName, Dimension> = BTreeMap::new();
        for doc in self.dimensions {
            let dimension = Dimension::new(doc.name.clone(), doc.column, doc.via, doc.values, doc.description);
            if dimensions.insert(doc.name.clone(), dimension).is_some() {
                return Err(InvalidMetricDocument::DuplicateDimension {
                    metric: self.name,
                    dimension: doc.name,
                });
            }
        }
        Ok(Metric::new(
            self.name,
            self.model,
            Measure::new(self.measure.aggregate, self.measure.column),
            self.time_column,
            self.grains,
            dimensions,
            self.anchor.map(|a| Anchor::new(a.range, a.value.into_text())),
            description,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{DocumentKind, InvalidMetricDocument, KindProbe, MetricDoc, ModelDoc};
    use sutura_domain::model::{Aggregate, DimensionName, Grain, MetricName};

    fn metric_doc(yaml: &str) -> Result<MetricDoc, serde_norway::Error> {
        serde_norway::from_str(yaml)
    }

    const MINIMAL_METRIC: &str = "
kind: metric
name: revenue
model: orders
measure:
  aggregate: sum
  column: amount_cents
time_column: order_date
grains: [month]
";

    #[test]
    fn a_minimal_metric_document_parses() {
        let doc = metric_doc(MINIMAL_METRIC).expect("a minimal metric is a metric");
        let metric = doc
            .into_domain(String::from("Net revenue."))
            .expect("no dimensions cannot be duplicated");
        assert_eq!(metric.name(), &MetricName::parse("revenue").expect("a name"));
        assert_eq!(metric.measure().aggregate(), Aggregate::Sum);
        assert!(metric.supports_grain(Grain::Month));
        assert!(!metric.supports_grain(Grain::Day));
        assert_eq!(metric.description(), "Net revenue.");
        assert!(metric.anchor().is_none());
    }

    #[test]
    fn a_misspelled_key_is_an_error_and_not_a_dropped_field() {
        // The bug this prevents, and the reason `deny_unknown_fields` is on every shape here:
        // `colums:` loads a model with no columns at all. It then passes every consistency check
        // that only looks at what is declared, and refuses every question for a reason that says
        // nothing about a typo three directories away.
        let yaml = "
kind: model
name: orders
source: local
table: orders
colums: [amount_cents]
";
        let err = serde_norway::from_str::<ModelDoc>(yaml).expect_err("a misspelled key is not a field");
        assert!(err.to_string().contains("colums"), "{err}");
    }

    #[test]
    fn a_document_carrying_sql_is_refused_by_name() {
        // The load-bearing half of the first-party-models decision: there is no field a statement
        // fits in, so an attempt to add one to a catalog document fails naming the field rather
        // than being quietly ignored.
        let yaml = format!("{MINIMAL_METRIC}sql: \"SELECT 1\"\n");
        let err = metric_doc(&yaml).expect_err("sql is not a metric field");
        assert!(err.to_string().contains("sql"), "{err}");
    }

    #[test]
    fn a_free_text_expression_is_refused_by_name() {
        // The same check from the other direction: the reference modelling languages spell a
        // measure `expression: sum(amount)`, and somebody will try it here.
        let yaml = format!("{MINIMAL_METRIC}expression: \"sum(amount_cents)\"\n");
        let err = metric_doc(&yaml).expect_err("expression is not a metric field");
        assert!(err.to_string().contains("expression"), "{err}");
    }

    #[test]
    fn an_unknown_aggregate_names_the_closed_set() {
        // A measure is an aggregate from a closed set. `median` is a reasonable thing to want and
        // this is what says so, rather than generating SQL for a function nobody checked.
        let yaml = MINIMAL_METRIC.replace("aggregate: sum", "aggregate: median");
        let err = metric_doc(&yaml).expect_err("median is not one of the aggregates");
        assert!(err.to_string().contains("median"), "{err}");
    }

    #[test]
    fn a_dimension_declared_twice_is_refused_rather_than_deduplicated() {
        // Why dimensions are a list and not a map: a YAML mapping with a repeated key keeps the
        // last value silently, so the metric would load with the second definition and the author
        // would have no way to tell which one is live.
        let yaml = format!(
            "{MINIMAL_METRIC}dimensions:\n  - name: region\n    column: region_code\n  - name: region\n    column: other_code\n"
        );
        let doc = metric_doc(&yaml).expect("two list entries are valid YAML");
        assert_eq!(
            doc.into_domain(String::new()).unwrap_err(),
            InvalidMetricDocument::DuplicateDimension {
                metric: MetricName::parse("revenue").expect("a name"),
                dimension: DimensionName::parse("region").expect("a name"),
            }
        );
    }

    #[test]
    fn an_anchor_value_may_be_written_with_or_without_quotes() {
        // Both forms occur in a file a person edits, and the unquoted one failing with "invalid
        // type: integer" is a true message about a file that looks right.
        for literal in ["197122", "\"197122\""] {
            let yaml =
                format!("{MINIMAL_METRIC}anchor:\n  range: {{ start: 2026-06-01, end: 2026-07-01 }}\n  value: {literal}\n");
            let metric = metric_doc(&yaml)
                .expect("both spellings parse")
                .into_domain(String::new())
                .expect("no dimensions to duplicate");
            let anchor = metric.anchor().expect("the document declared one");
            assert_eq!(anchor.value(), "197122");
        }
    }

    #[test]
    fn an_anchor_range_must_still_be_a_range() {
        // The domain's parsing applies through the document: an inverted anchor range is refused
        // here rather than becoming an anchor that can never match.
        let yaml = format!("{MINIMAL_METRIC}anchor:\n  range: {{ start: 2026-07-01, end: 2026-06-01 }}\n  value: 1\n");
        drop(metric_doc(&yaml).expect_err("an inverted range is not a range"));
    }

    #[test]
    fn a_kind_probe_reads_the_tag_without_the_rest() {
        // The dispatch step. It has to tolerate every other field, which is exactly why it cannot
        // be the shape that denies unknown ones.
        let probe: KindProbe = serde_norway::from_str(MINIMAL_METRIC).expect("the tag is readable on its own");
        assert_eq!(probe.kind(), DocumentKind::Metric);
        assert_eq!(probe.kind().as_str(), "metric");
    }

    #[test]
    fn an_unknown_kind_is_refused() {
        let err = serde_norway::from_str::<KindProbe>("kind: dashboard\n").expect_err("a dashboard is not a catalog document");
        assert!(err.to_string().contains("dashboard"), "{err}");
    }
}
