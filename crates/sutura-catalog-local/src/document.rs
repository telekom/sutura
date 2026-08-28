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
//!
//! [`sutura_domain::measure`] is the one exception, and the `measure` field of [`MetricDoc`] argues
//! for it where a reader will be standing when they wonder. In short: those types already carry
//! exactly this format's representation, and mirroring its variants here would buy nothing but a
//! place to forget the next one.

use std::collections::{BTreeMap, BTreeSet};

use sutura_domain::calendar::TimeRange;
use sutura_domain::catalog::{Anchor, Description, Dimension, DimensionValue, Metric, Model, Relationship};
use sutura_domain::measure::{Measure, RequiredFilter};
use sutura_domain::model::{
    ColumnName, DimensionName, Grain, JoinType, MetricName, ModelName, RelationshipName, SourceName, TableName,
};

// The four knowledge documents. Their own module because this file is already at two thirds of the
// thousand-line limit `cargo xtask max-lines` enforces, and because they are a separate concern: a
// definition decides what executes and a note decides what a reader understands.
pub mod knowledge;

/// What a document declares itself to be.
///
/// Required in every document rather than inferred from the directory it sits in. A file in the
/// wrong directory is then an error naming the mismatch, instead of a metric that was quietly never
/// loaded, and the loader can walk one tree instead of trusting a layout convention.
///
/// **Seven kinds now, and the split between them is worth reading as two groups.** The first three
/// are definitions: they decide what executes, and `sutura_domain::catalog` checks them. The last
/// four are knowledge: they decide what a reader understands, and `sutura_domain::knowledge` checks
/// them. Nothing in the loader treats the two groups differently - one walk, one tag, one dispatch -
/// which is what keeps "which directory is this in" from becoming part of the format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Model,
    Relationship,
    Metric,
    /// One entry of the business glossary.
    Glossary,
    /// Something a reader has to know before trusting a number.
    Caveat,
    /// A term this catalog deliberately does not define.
    ///
    /// The word an author writes is `not_defined`, which says what they are doing; the domain type
    /// is `Absence`, which says what the thing is. Two names for two audiences, and the format's one
    /// is the one that appears in an error about a file.
    NotDefined,
    /// A worked question: how somebody asked it, and what to send.
    Example,
}

impl DocumentKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Relationship => "relationship",
            Self::Metric => "metric",
            Self::Glossary => "glossary",
            Self::Caveat => "caveat",
            Self::NotDefined => "not_defined",
            Self::Example => "example",
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
    pub fn into_domain(self, description: Description) -> Model {
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
    values: Option<BTreeSet<DimensionValue>>,
    #[serde(default)]
    description: Description,
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
    /// The domain's [`Measure`], deserialized directly rather than restated by a local `*Doc` shape.
    ///
    /// The one departure from this module's rule that the file format is its own thing, and it is
    /// chosen rather than inherited. [`sutura_domain::measure`] already derives `Deserialize` with
    /// exactly the representation this format wants, and each part of that is load-bearing here:
    /// the shape is externally tagged, so `simple:` or `ratio:` is a word an author writes rather
    /// than something inferred from which fields are present; `deny_unknown_fields` holds at every
    /// depth, on the variant and on the struct behind a term, so a misspelled key nested inside
    /// `simple:` is still an error naming the typo; and `zero_denominator` has no default, so its
    /// absence is a missing-field error naming the field instead of a silent pick between two
    /// defensible behaviours. A mirror would be two shapes, two terms and four operators of
    /// restatement, and its failure mode is the expensive one: a shape or a term added to the domain
    /// and forgotten here is one no document can express, with nothing anywhere failing to say so.
    ///
    /// The rule still holds for everything else. `Model`, `Relationship`, `Metric` and `Dimension`
    /// derive only `Serialize`, and that asymmetry is the domain saying which of its types it also
    /// intends as a wire format. When a catalog file needs to spell a measure differently from the
    /// domain, this field grows a `MeasureDoc` and a conversion, and that diff is the discussion.
    ///
    /// `singleton_map` is the one thing this costs, and it is a YAML fact rather than a design
    /// choice. An externally tagged enum in `serde_norway` is a YAML *tag*: `measure: !simple {..}`.
    /// Nobody writing a catalog file spells a shape with a `!`, and the failure without this
    /// adapter is `invalid type: map, expected a YAML tag starting with '!'`, which tells an author
    /// nothing about the document they wrote. `singleton_map` reads the one-key mapping form that
    /// the rest of the format already looks like, and leaves every field inside it, including
    /// `deny_unknown_fields`, to the ordinary derive.
    ///
    /// Non-recursive, and that is now a statement about the shapes rather than an accident. The two
    /// enums nested inside a measure need no adapter: a term is a flat mapping read through a
    /// `try_from` struct, and `zero_denominator` is a unit variant, which `serde_norway` already
    /// spells as a plain scalar. `singleton_map_recursive` would reach into both and is not needed
    /// by either.
    #[serde(with = "serde_norway::with::singleton_map")]
    measure: Measure,
    /// Predicates that are part of the definition, applied to every question about the metric.
    ///
    /// Defaulted to empty, because most metrics have none and requiring the key on every document
    /// would make the common case noisy. The direction that must never be defaulted is the other
    /// one: absent means "no predicate", never "not yet decided".
    ///
    /// `singleton_map_recursive` rather than `singleton_map` because the enum is inside a sequence,
    /// and the non-recursive adapter applies to the value it is attached to. Recursion is safe here:
    /// a [`RequiredFilter`] payload holds a column name and a value, both of them newtypes over one
    /// scalar with a `try_from`, so there is no nested enum for it to reinterpret.
    #[serde(default, with = "serde_norway::with::singleton_map_recursive")]
    required_filters: Vec<RequiredFilter>,
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
    pub fn into_domain(self, description: Description) -> Result<Metric, InvalidMetricDocument> {
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
            self.measure,
            self.required_filters,
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
    use super::{Description, DocumentKind, InvalidMetricDocument, KindProbe, MetricDoc, ModelDoc};
    use sutura_domain::catalog::DimensionValue;
    use sutura_domain::measure::{AggregatedColumn, Measure, RequiredFilter, Term, ZeroDenominator};
    use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName};

    fn metric_doc(yaml: &str) -> Result<MetricDoc, serde_norway::Error> {
        serde_norway::from_str(yaml)
    }

    fn description(raw: &str) -> Description {
        Description::parse(raw).expect("a test description is a description")
    }

    fn column(raw: &str) -> ColumnName {
        ColumnName::parse(raw).expect("a test column is a column")
    }

    fn value(raw: &str) -> DimensionValue {
        DimensionValue::parse(raw).expect("a test value is a value")
    }

    fn aggregated(aggregate: Aggregate, raw: &str) -> Term {
        Term::Aggregate(AggregatedColumn::new(aggregate, column(raw)))
    }

    const MINIMAL_METRIC: &str = "
kind: metric
name: revenue
model: orders
measure:
  simple: { aggregate: sum, column: amount_cents }
time_column: order_date
grains: [month]
";

    /// The minimal document with a different measure block substituted in.
    ///
    /// `measure` is written the way it appears in a file, already indented under `measure:` and
    /// ending in a newline, so each measure test below reads as one statement about one shape
    /// instead of a second copy of every other field.
    fn metric_measuring(measure: &str) -> String {
        format!("kind: metric\nname: revenue\nmodel: orders\nmeasure:\n{measure}time_column: order_date\ngrains: [month]\n")
    }

    #[test]
    fn a_minimal_metric_document_parses() {
        let doc = metric_doc(MINIMAL_METRIC).expect("a minimal metric is a metric");
        let metric = doc
            .into_domain(description("Net revenue."))
            .expect("no dimensions cannot be duplicated");
        assert_eq!(metric.name(), &MetricName::parse("revenue").expect("a name"));
        assert_eq!(metric.measure(), &Measure::Simple(aggregated(Aggregate::Sum, "amount_cents")));
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
    fn a_count_if_term_has_a_word_of_its_own_in_the_format() {
        // The number this prevents: "how many rows are true" written as a count of a boolean column.
        // `COUNT(col)` counts non-null rows, so it counts the `false` ones too, and the wrong answer
        // arrives with no error anywhere on the way. A format with no word for the thing that was
        // meant is a format that pushes the author towards the spelling that silently lies.
        let yaml = metric_measuring("  simple: { count_if: churned_in_month }\n");
        let metric = metric_doc(&yaml)
            .expect("count_if is a term")
            .into_domain(Description::default())
            .expect("no dimensions to duplicate");
        assert_eq!(
            metric.measure(),
            &Measure::Simple(Term::CountIf {
                column: column("churned_in_month"),
            })
        );
    }

    #[test]
    fn a_ratio_measure_keeps_its_two_terms_apart() {
        // The number this prevents: a ratio collapsed into `avg`. `sum(mrr) / count(distinct
        // customer)` is not the mean of a column, and a document that could only say `avg` would
        // send whoever wanted the real figure to a hand-written statement outside this catalog.
        let yaml = metric_measuring(concat!(
            "  ratio:\n",
            "    numerator: { aggregate: sum, column: mrr_eur }\n",
            "    denominator: { aggregate: count_distinct, column: customer_key }\n",
            "    zero_denominator: yields_null\n",
        ));
        let metric = metric_doc(&yaml)
            .expect("a ratio is a measure shape")
            .into_domain(Description::default())
            .expect("no dimensions to duplicate");
        assert_eq!(
            metric.measure(),
            &Measure::Ratio {
                numerator: aggregated(Aggregate::Sum, "mrr_eur"),
                denominator: aggregated(Aggregate::CountDistinct, "customer_key"),
                zero_denominator: ZeroDenominator::Null,
            }
        );
    }

    #[test]
    fn a_conditional_count_can_be_written_as_a_ratio_numerator() {
        // The document that could not be written before, and the reason the vocabulary was
        // refactored: a churn rate is a conditional count over a distinct count. While `count_if`
        // was a sibling of `ratio` rather than a term inside one, this file had to be split into two
        // certified metrics and a division somebody did by hand.
        let yaml = metric_measuring(concat!(
            "  ratio:\n",
            "    numerator: { count_if: churned_in_month }\n",
            "    denominator: { aggregate: count_distinct, column: subscription_key }\n",
            "    zero_denominator: yields_null\n",
        ));
        let metric = metric_doc(&yaml)
            .expect("a conditional count is a term like any other")
            .into_domain(Description::default())
            .expect("no dimensions to duplicate");
        assert_eq!(
            metric.measure(),
            &Measure::Ratio {
                numerator: Term::CountIf {
                    column: column("churned_in_month"),
                },
                denominator: aggregated(Aggregate::CountDistinct, "subscription_key"),
                zero_denominator: ZeroDenominator::Null,
            }
        );
    }

    #[test]
    fn an_unknown_measure_shape_is_refused_by_name() {
        // What external tagging buys, and the reason the measure is not inferred from which fields
        // are present: an unrecognised shape fails naming the word that was written. Inferred from
        // fields, the same document would fail with "data did not match any variant", which names
        // nothing and sends the reader to guess which of two shapes was nearly right.
        let yaml = metric_measuring("  median: { column: amount_cents }\n");
        let err = metric_doc(&yaml).expect_err("median is not a measure shape");
        assert!(err.to_string().contains("median"), "{err}");
    }

    #[test]
    fn an_unknown_term_is_refused_by_name_too() {
        // A term is flat rather than externally tagged, so `deny_unknown_fields` on the struct
        // behind it is what keeps the same property one level down: an unrecognised term names the
        // word that was written and lists the words that exist, instead of "data did not match any
        // variant" - which is exactly what `#[serde(untagged)]` on the term would have produced.
        let yaml = metric_measuring("  simple: { median: amount_cents }\n");
        let err = metric_doc(&yaml).expect_err("median is not a term");
        let message = err.to_string();
        assert!(message.contains("median"), "{message}");
        assert!(message.contains("count_if"), "{message}");
    }

    #[test]
    fn half_a_term_is_refused_by_what_is_missing() {
        // The one cost of the flat form, and it is paid in a sentence rather than in a guess. An
        // aggregate with no column and a column with no aggregate are both documents somebody meant
        // something by, so each is named for what it is short of.
        let no_column = metric_measuring("  simple: { aggregate: sum }\n");
        let err = metric_doc(&no_column).expect_err("an aggregate with no column is not a term");
        assert!(err.to_string().contains("no `column`"), "{err}");

        let no_aggregate = metric_measuring("  simple: { column: amount_cents }\n");
        let err = metric_doc(&no_aggregate).expect_err("a column with no aggregate is not a term");
        assert!(err.to_string().contains("no `aggregate`"), "{err}");

        // Both at once is refused rather than resolved by precedence: the document means one of
        // them, and picking one would certify a number nobody asked for.
        let both = metric_measuring("  simple: { aggregate: sum, column: amount_cents, count_if: paid }\n");
        let err = metric_doc(&both).expect_err("two terms in one mapping is not a term");
        assert!(err.to_string().contains("a term is one thing"), "{err}");
    }

    #[test]
    fn a_ratio_without_zero_denominator_is_an_error_and_not_a_default() {
        // Both behaviours are defensible: a rate over a period with no rows is arguably null and
        // arguably a failure. So a default here would pick one on the author's behalf and the
        // document would not record which. The failure that hides behind it is a metric that reads
        // as null in one deployment and refuses in the next, with identical definitions on disk.
        let yaml = metric_measuring(concat!(
            "  ratio:\n",
            "    numerator: { aggregate: sum, column: mrr_eur }\n",
            "    denominator: { aggregate: count_distinct, column: customer_key }\n",
        ));
        let err = metric_doc(&yaml).expect_err("zero_denominator has no default to fall back on");
        assert!(err.to_string().contains("zero_denominator"), "{err}");
    }

    #[test]
    fn the_null_case_is_spelled_yields_null_because_yaml_owns_the_word_null() {
        // Why the two words are not `null` and `fail`. In YAML a bare `null` is the null literal, so
        // the most natural spelling in the whole vocabulary would hand the deserializer a unit value
        // and fail with a type error about a line that looks right. Named after what a zero
        // denominator does instead, so the field and its value read as one sentence and neither of
        // them can collide with a scalar YAML resolves itself.
        let ratio = |zero_denominator: &str| {
            metric_measuring(&format!(
                concat!(
                    "  ratio:\n",
                    "    numerator: {{ aggregate: sum, column: mrr_eur }}\n",
                    "    denominator: {{ aggregate: count_distinct, column: customer_key }}\n",
                    "    zero_denominator: {}\n",
                ),
                zero_denominator
            ))
        };
        for (word, expected) in [("yields_null", ZeroDenominator::Null), ("fails", ZeroDenominator::Fail)] {
            let metric = metric_doc(&ratio(word))
                .expect("both words are words")
                .into_domain(Description::default())
                .expect("no dimensions to duplicate");
            assert_eq!(
                metric.measure(),
                &Measure::Ratio {
                    numerator: aggregated(Aggregate::Sum, "mrr_eur"),
                    denominator: aggregated(Aggregate::CountDistinct, "customer_key"),
                    zero_denominator: expected,
                }
            );
        }
        // And the spelling that would have been the trap: refused, rather than read as the variant
        // whose Rust name it happens to match.
        let err = metric_doc(&ratio("null")).expect_err("a YAML null is not a zero-denominator behaviour");
        assert!(err.to_string().contains("zero_denominator"), "{err}");
    }

    #[test]
    fn a_misspelled_key_inside_a_term_is_refused_too() {
        // `deny_unknown_fields` has to hold at every depth, not only on the outer document. Without
        // it on the term, `agregate:` is dropped and the error becomes one about a term that says
        // nothing, printed next to a line that plainly names an aggregate - which sends the reader
        // looking for a field they can see rather than at the typo in it.
        let yaml = metric_measuring("  simple: { agregate: sum, column: amount_cents }\n");
        let err = metric_doc(&yaml).expect_err("a misspelled key nested in a term is not a field");
        assert!(err.to_string().contains("agregate"), "{err}");
    }

    #[test]
    fn every_required_filter_operator_is_written_as_a_named_operator() {
        // The four predicates a definition may carry, pinned as a set. A filter whose operator did
        // not parse would drop out of the metric, and a metric that quietly lost its predicate
        // answers every question with a larger number under the same certified name.
        let filters = concat!(
            "required_filters:\n",
            "  - equals: { column: channel, value: web }\n",
            "  - not_equals: { column: channel, value: store }\n",
            "  - is_true: { column: is_paid }\n",
            "  - is_not_null: { column: shipped_at }\n",
        );
        let metric = metric_doc(&format!("{MINIMAL_METRIC}{filters}"))
            .expect("all four operators are operators")
            .into_domain(Description::default())
            .expect("no dimensions to duplicate");
        let expected = vec![
            RequiredFilter::Equals {
                column: column("channel"),
                value: value("web"),
            },
            RequiredFilter::NotEquals {
                column: column("channel"),
                value: value("store"),
            },
            RequiredFilter::IsTrue {
                column: column("is_paid"),
            },
            RequiredFilter::IsNotNull {
                column: column("shipped_at"),
            },
        ];
        assert_eq!(metric.required_filters(), expected.as_slice());
    }

    #[test]
    fn an_unknown_required_filter_operator_is_refused_by_name() {
        // The same closed-vocabulary check as for measure shapes. `greater_than` is a reasonable
        // thing to want, and this is what says so, rather than a predicate nobody generated an arm
        // for being dropped from a definition that claims to carry it.
        let yaml = format!("{MINIMAL_METRIC}required_filters:\n  - greater_than: {{ column: amount_cents, value: 0 }}\n");
        let err = metric_doc(&yaml).expect_err("greater_than is not one of the four operators");
        assert!(err.to_string().contains("greater_than"), "{err}");
    }

    #[test]
    fn a_required_filter_value_is_parsed_by_the_document_it_arrives_in() {
        // The wired path for the third authored string. A definitional filter's value used to be a
        // `String` with `deny_unknown_fields` around it and no character rule inside it, so a
        // right-to-left override in this line rendered as `status = "active"` in `sutura
        // definitions` and bound something else - the finding already closed for an authored SQL
        // fragment and a glossary phrase, at a channel that still had it. `DimensionValue`'s
        // `try_from` is what makes the frontmatter reader the enforcement point.
        for (value, needle) in [
            ("act\u{202E}ive", "invisible"),
            ("\"  active\"", "spacing"),
            ("\"\"", "empty"),
        ] {
            let yaml = format!("{MINIMAL_METRIC}required_filters:\n  - equals: {{ column: status, value: {value} }}\n");
            let err = metric_doc(&yaml).expect_err("a value that is not a dimension value is not one");
            assert!(err.to_string().contains(needle), "{value}: {err}");
        }
    }

    #[test]
    fn required_filters_default_to_empty_when_the_key_is_absent() {
        // Most metrics carry no definitional predicate, so the key is optional. The direction that
        // must not be confused is the other one: absent has to mean "no predicate", never "not
        // decided yet", because a filter list that could be unset is a filter list something
        // downstream would eventually treat as a hint.
        let metric = metric_doc(MINIMAL_METRIC)
            .expect("a minimal metric is a metric")
            .into_domain(Description::default())
            .expect("no dimensions to duplicate");
        assert!(metric.required_filters().is_empty());
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
            doc.into_domain(Description::default()).unwrap_err(),
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
                .into_domain(Description::default())
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
