//! The shapes on the wire, and the conversions to and from the domain.
//!
//! # Why these exist rather than serializing the domain types
//!
//! Two reasons, and the first is structural. `sutura-domain` may depend on `serde` and `thiserror`
//! and nothing else - a machine-checked rule - so it cannot carry a `utoipa::ToSchema` derive, and
//! a document generated from the handlers needs one for every body. The wire shapes live here, in
//! the adapter, which is where a wire format belongs anyway.
//!
//! The second is that a wire type is allowed to be *worse* than a domain type, and should be. Every
//! field of [`QuestionBody`] is a plain string, because that is what arrives; each one is then
//! parsed into the newtype that establishes its invariant, and a failure names the field. A body
//! deserialized straight into `Query` would give a caller `invalid type: string` and no field.
//!
//! # `deny_unknown_fields`, and what it is for here
//!
//! The same thing it is for on a catalog document, arrived at from the other side. Without it a
//! body carrying `sql:` or `table:` deserializes cleanly with the extra key dropped on the floor,
//! and a caller who believes they sent SQL gets an answer to a different question. With it, the
//! attempt is a 400 naming the field. The tool surface has no field for any of that - see
//! `sutura_domain::query` - and this is what keeps that true across a JSON parser.

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::model::{DimensionName, Grain, MetricName};
use sutura_domain::pinned::{PinnedDefinitions, Provenance};
use sutura_domain::query::{Filter, Query, RefusalReason, ToolOutcome};
use sutura_domain::warehouse::RowSet;

/// A question, as it arrives.
#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionBody {
    /// The metric to measure. Must be one this catalog defines.
    #[schema(example = "revenue")]
    metric: String,
    /// The time resolution to aggregate to.
    #[schema(example = "month")]
    grain: String,
    /// The half-open period to ask about.
    range: RangeBody,
    /// What to group by. At most four; a dimension the metric does not declare is a refusal.
    #[serde(default)]
    dimensions: Vec<String>,
    /// Equality filters, each on a dimension the metric declares as filterable.
    #[serde(default)]
    filters: Vec<FilterBody>,
}

/// A half-open period: `start` is included, `end` is not.
///
/// Half-open at every grain and in every dialect, which is what makes a month
/// `[2026-06-01, 2026-07-01)` rather than a last day that differs per month.
#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RangeBody {
    #[schema(example = "2026-06-01")]
    start: String,
    #[schema(example = "2026-07-01")]
    end: String,
}

/// One equality filter.
#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FilterBody {
    #[schema(example = "region")]
    dimension: String,
    #[schema(example = "north")]
    value: String,
}

/// Why a body is not a question.
///
/// Every variant names the field, and none of them echoes the caller's value back except where the
/// value is the thing that failed to parse as an identifier - which is a bounded character set, not
/// free text.
#[derive(Debug, thiserror::Error)]
pub enum MalformedQuestion {
    #[error("`metric` is not a metric name")]
    Metric {
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    #[error("`grain` is not one of: day, week, month, quarter, year")]
    Grain { found: String },
    #[error("`range.{field}` is not a date in `YYYY-MM-DD` form")]
    Date {
        field: &'static str,
        #[source]
        cause: sutura_domain::calendar::InvalidDate,
    },
    #[error("`range` is not a period")]
    Range {
        #[source]
        cause: sutura_domain::calendar::InvalidTimeRange,
    },
    #[error("`dimensions[{index}]` is not a dimension name")]
    Dimension {
        index: usize,
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
    #[error("`filters[{index}].dimension` is not a dimension name")]
    FilterDimension {
        index: usize,
        #[source]
        cause: sutura_domain::model::InvalidIdentifier,
    },
}

impl TryFrom<QuestionBody> for Query {
    type Error = MalformedQuestion;

    fn try_from(body: QuestionBody) -> Result<Self, Self::Error> {
        let metric = MetricName::parse(&body.metric).map_err(|cause| MalformedQuestion::Metric { cause })?;
        let grain = grain_of(&body.grain)?;
        let range = range_of(&body.range)?;
        let mut dimensions = Vec::with_capacity(body.dimensions.len());
        for (index, raw) in body.dimensions.iter().enumerate() {
            dimensions.push(DimensionName::parse(raw).map_err(|cause| MalformedQuestion::Dimension { index, cause })?);
        }
        let mut filters = Vec::with_capacity(body.filters.len());
        for (index, raw) in body.filters.iter().enumerate() {
            let dimension =
                DimensionName::parse(&raw.dimension).map_err(|cause| MalformedQuestion::FilterDimension { index, cause })?;
            filters.push(Filter::new(dimension, raw.value.clone()));
        }
        Ok(Self::new(metric, grain, range, dimensions, filters))
    }
}

/// The grain, from its name.
///
/// A hand-written match rather than the derived `Deserialize`, so the error names the accepted set
/// instead of quoting serde at a caller.
fn grain_of(raw: &str) -> Result<Grain, MalformedQuestion> {
    match raw {
        "day" => Ok(Grain::Day),
        "week" => Ok(Grain::Week),
        "month" => Ok(Grain::Month),
        "quarter" => Ok(Grain::Quarter),
        "year" => Ok(Grain::Year),
        other => Err(MalformedQuestion::Grain {
            found: String::from(other),
        }),
    }
}

fn range_of(body: &RangeBody) -> Result<TimeRange, MalformedQuestion> {
    let start = Date::parse(&body.start).map_err(|cause| MalformedQuestion::Date { field: "start", cause })?;
    let end = Date::parse(&body.end).map_err(|cause| MalformedQuestion::Date { field: "end", cause })?;
    TimeRange::new(start, end).map_err(|cause| MalformedQuestion::Range { cause })
}

// ---------------------------------------------------------------- responses ----

/// What a question produced.
///
/// **Both variants come back with `200`, and that is the contract rather than an oversight.** A
/// refusal is a *result*: the caller asked something they may not have, and the answer is no. An
/// error status would invite a client library to retry it, and retrying a governance decision until
/// it succeeds is precisely the behaviour the refusal exists to prevent. The `outcome` field is
/// what a caller branches on.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum OutcomeBody {
    /// The question was answered.
    Answer {
        provenance: ProvenanceBody,
        /// Column labels, in projection order.
        columns: Vec<String>,
        /// Rows, each as many cells as there are columns. Every cell is rendered as text by the
        /// one function the domain uses for it, so a number here reads the same way it does in an
        /// anchor comparison.
        rows: Vec<Vec<String>>,
    },
    /// The question was refused.
    Refusal { reason: RefusalBody },
}

/// Which definitions produced this answer.
///
/// Always present on an answer, and it is the reason an answer can be trusted at all: the version
/// names the snapshot and the digest is over its canonical form, so the same question against a
/// different bundle is visibly a different answer.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ProvenanceBody {
    #[schema(example = "2026.06.1")]
    definition_version: String,
    definition_digest: String,
}

/// Why a question was refused.
///
/// A stable `code` per refusal, plus a sentence. The code is what a caller branches on; the
/// sentence is for a person.
///
/// **Nothing here echoes a value the caller sent.** The domain's refusal variants already stop
/// short of that - a rejected filter value names the dimension and not the value, on purpose,
/// because reflecting caller text into a message that reaches a log, a UI and an agent's context is
/// how a rejected value becomes somebody else's input. The identifiers that *are* echoed are parsed
/// newtypes over a bounded character set, and the numbers are derived from parsed dates.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct RefusalBody {
    #[schema(example = "metric_unknown")]
    code: &'static str,
    detail: String,
}

impl RefusalBody {
    /// The code, for a test that asserts on the contract rather than on the prose.
    #[inline]
    pub const fn code(&self) -> &'static str {
        self.code
    }
}

/// The wire form of a refusal.
///
/// **The match is exhaustive with no wildcard arm, deliberately.** A refusal variant added to the
/// domain fails to compile here until it is given a code and a sentence, which is what stops a new
/// governance outcome from reaching a caller as an unnamed one.
fn refusal_body(reason: &RefusalReason) -> RefusalBody {
    let (code, detail) = match *reason {
        RefusalReason::MetricUnknown { ref metric } => {
            ("metric_unknown", format!("this catalog defines no metric called `{metric}`"))
        }
        RefusalReason::GrainNotSupported { ref metric, grain } => {
            ("grain_not_supported", format!("`{metric}` is not defined at `{grain}` grain"))
        }
        RefusalReason::DimensionNotPermitted {
            ref metric,
            ref dimension,
        } => (
            "dimension_not_permitted",
            format!("`{metric}` does not declare a dimension called `{dimension}`"),
        ),
        RefusalReason::DimensionNotFilterable {
            ref metric,
            ref dimension,
        } => (
            "dimension_not_filterable",
            format!("`{dimension}` can be grouped by on `{metric}` but not filtered on"),
        ),
        RefusalReason::DimensionValueNotAllowed {
            ref metric,
            ref dimension,
        } => (
            "dimension_value_not_allowed",
            // The value is deliberately absent. See the type documentation.
            format!("that value is not one `{metric}` declares for `{dimension}`"),
        ),
        RefusalReason::DuplicateDimension { ref dimension } => {
            ("duplicate_dimension", format!("`{dimension}` appears more than once"))
        }
        RefusalReason::TooManyDimensions { requested, limit } => (
            "too_many_dimensions",
            format!("{requested} group-by keys were asked for and the maximum is {limit}"),
        ),
        RefusalReason::TimeRangeTooLong { days, limit } => (
            "time_range_too_long",
            format!("the period spans {days} days and the maximum is {limit}"),
        ),
        RefusalReason::PlanSpansTwoSources { sources } => (
            "plan_spans_two_sources",
            format!("answering this would read from {sources} data systems, and a plan runs against one"),
        ),
        RefusalReason::SourceUnavailable { ref source } => (
            "source_unavailable",
            format!("`{source}` could not be reached as the calling subject"),
        ),
    };
    RefusalBody { code, detail }
}

impl From<&ToolOutcome> for OutcomeBody {
    fn from(outcome: &ToolOutcome) -> Self {
        match *outcome {
            ToolOutcome::Answer {
                ref provenance,
                ref rows,
            } => Self::Answer {
                provenance: provenance_body(provenance),
                columns: rows.columns().to_vec(),
                rows: render(rows),
            },
            ToolOutcome::Refusal { ref reason } => Self::Refusal {
                reason: refusal_body(reason),
            },
        }
    }
}

fn provenance_body(provenance: &Provenance) -> ProvenanceBody {
    ProvenanceBody {
        definition_version: String::from(provenance.version().as_str()),
        definition_digest: String::from(provenance.digest().as_str()),
    }
}

/// Cells as text, through the domain's own renderer.
///
/// Text and not JSON numbers, and the reason is exactness: a measure over integer minor units is an
/// `i64` that does not survive a round trip through a JSON number in every client, and an anchor is
/// compared as text. One rendering everywhere means the number in an answer is the number in the
/// anchor that certified it.
fn render(rows: &RowSet) -> Vec<Vec<String>> {
    rows.rows()
        .iter()
        .map(|row| row.iter().map(sutura_domain::warehouse::Value::render).collect())
        .collect()
}

// ------------------------------------------------------------------ catalog ----

/// What this catalog defines.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct CatalogBody {
    provenance: ProvenanceBody,
    metrics: Vec<MetricBody>,
}

/// One metric, as much of it as a caller needs to ask a valid question.
///
/// Descriptive content only. Nothing here selects, widens or parameterizes what executes - the
/// catalog port takes no request context and cannot - so this is a reader's view of a pinned
/// bundle rather than an input to one.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct MetricBody {
    name: String,
    description: String,
    /// Coarsest first, which is the order an anchor is checked at.
    grains: Vec<String>,
    dimensions: Vec<DimensionBody>,
}

/// One dimension of one metric.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct DimensionBody {
    name: String,
    description: String,
    /// Whether this dimension can be filtered on as well as grouped by.
    filterable: bool,
    /// The values a filter may use, when the catalog declares a set. Absent means the dimension is
    /// groupable and not filterable.
    #[serde(skip_serializing_if = "Option::is_none")]
    allowed_values: Option<Vec<String>>,
}

impl From<&PinnedDefinitions> for CatalogBody {
    fn from(pinned: &PinnedDefinitions) -> Self {
        let metrics = pinned
            .definitions()
            .metrics()
            .values()
            .map(|metric| MetricBody {
                name: String::from(metric.name().as_str()),
                description: String::from(metric.description()),
                grains: {
                    let mut grains: Vec<Grain> = metric.grains().iter().copied().collect();
                    grains.sort_unstable_by(|a, b| b.cmp(a));
                    grains.into_iter().map(|grain| String::from(grain.as_str())).collect()
                },
                dimensions: metric
                    .dimensions()
                    .values()
                    .map(|dimension| DimensionBody {
                        name: String::from(dimension.name().as_str()),
                        description: String::from(dimension.description()),
                        filterable: dimension.is_filterable(),
                        allowed_values: dimension.allowed_values().map(|values| values.iter().cloned().collect()),
                    })
                    .collect(),
            })
            .collect();
        Self {
            provenance: provenance_body(&pinned.provenance()),
            metrics,
        }
    }
}

#[cfg(test)]
mod tests {
    use sutura_domain::model::{DimensionName, Grain, MetricName};
    use sutura_domain::query::{Query, RefusalReason, ToolOutcome};

    use super::{CatalogBody, MalformedQuestion, OutcomeBody, QuestionBody, refusal_body};

    /// The deserialized body. Separate from [`parse`] so the `Result` that test asserts on is the
    /// conversion's, not the JSON parser's - the two failures are different tests.
    fn body(json: &str) -> QuestionBody {
        serde_json::from_str(json).expect("the fixture is valid JSON for this shape")
    }

    fn parse(json: &str) -> Result<Query, MalformedQuestion> {
        Query::try_from(body(json))
    }

    #[test]
    fn a_well_formed_question_becomes_a_query() {
        let query = parse(
            r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},
                "dimensions":["region"],"filters":[{"dimension":"region","value":"north"}]}"#,
        )
        .expect("a well formed question is a query");
        assert_eq!(query.metric(), &MetricName::parse("revenue").expect("a name"));
        assert_eq!(query.grain(), Grain::Month);
        assert_eq!(
            query.dimensions(),
            [DimensionName::parse("region").expect("a name")].as_slice()
        );
        assert_eq!(query.filters().len(), 1);
    }

    #[test]
    fn a_body_carrying_sql_is_rejected_rather_than_having_the_field_dropped() {
        // THE governance property of this shape. Without `deny_unknown_fields` this body
        // deserializes cleanly, the `sql` key is discarded, and a caller who believes they sent SQL
        // is answered as though they had asked the modelled question instead.
        let raw = r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},
                      "sql":"select * from orders"}"#;
        let error = serde_json::from_str::<QuestionBody>(raw).expect_err("`sql` is not a field of a question");
        assert!(error.to_string().contains("sql"), "{error}");
    }

    #[test]
    fn a_malformed_field_names_the_field_it_was() {
        // A caller fixing a request needs to know which field, and a serde message does not say.
        let error = parse(r#"{"metric":"revenue","grain":"fortnight","range":{"start":"2026-06-01","end":"2026-07-01"}}"#)
            .expect_err("`fortnight` is not a grain");
        assert!(matches!(error, MalformedQuestion::Grain { .. }), "{error:?}");
        assert!(error.to_string().contains("quarter"), "{error}");

        let error = parse(r#"{"metric":"revenue","grain":"month","range":{"start":"nope","end":"2026-07-01"}}"#)
            .expect_err("`nope` is not a date");
        let MalformedQuestion::Date { field, .. } = error else {
            panic!("expected a date failure, got {error:?}");
        };
        assert_eq!(field, "start");

        let error = parse(
            r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},
                             "dimensions":["region","not a name"]}"#,
        )
        .expect_err("`not a name` is not an identifier");
        let MalformedQuestion::Dimension { index, .. } = error else {
            panic!("expected a dimension failure, got {error:?}");
        };
        assert_eq!(index, 1);
    }

    #[test]
    fn a_range_with_no_end_does_not_deserialize_at_all() {
        // The bound the type provides rather than the service: there is no unbounded form to send.
        let error =
            serde_json::from_str::<QuestionBody>(r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01"}}"#)
                .expect_err("a range with no end is not a range");
        assert!(error.to_string().contains("end"), "{error}");
    }

    #[test]
    fn a_rejected_filter_value_is_not_echoed_into_the_response() {
        // The domain refuses to carry the value in its refusal reason, and this is the assertion
        // that the wire shape does not put it back: a rejected value reflected into a response
        // reaches a log, a UI and an agent's context.
        let reason = RefusalReason::DimensionValueNotAllowed {
            metric: MetricName::parse("revenue").expect("a name"),
            dimension: DimensionName::parse("region").expect("a name"),
        };
        let body = refusal_body(&reason);
        assert_eq!(body.code(), "dimension_value_not_allowed");
        let rendered = serde_json::to_string(&body).expect("the refusal serializes");
        assert!(rendered.contains("region"), "{rendered}");
        assert!(!rendered.contains("north"), "{rendered}");
    }

    #[test]
    fn a_refusal_serializes_with_an_outcome_discriminator_and_a_code() {
        // What a caller branches on. Asserted on the JSON rather than on the Rust value, because
        // the JSON is the contract.
        let outcome = ToolOutcome::Refusal {
            reason: RefusalReason::TimeRangeTooLong { days: 9000, limit: 3653 },
        };
        let rendered = serde_json::to_string(&OutcomeBody::from(&outcome)).expect("the outcome serializes");
        assert!(rendered.contains(r#""outcome":"refusal""#), "{rendered}");
        assert!(rendered.contains(r#""code":"time_range_too_long""#), "{rendered}");
        assert!(rendered.contains("9000"), "{rendered}");
    }

    #[test]
    fn the_catalog_view_lists_grains_coarsest_first_and_carries_the_digest() {
        let bundle = crate::testing::bundle();
        let body = CatalogBody::from(&bundle);
        let rendered = serde_json::to_string(&body).expect("the catalog serializes");
        assert!(rendered.contains(r#""definition_version":"test-1""#), "{rendered}");
        // Month before day: the coarsest grain is the one an anchor is checked at, so listing it
        // first is the order a reader needs.
        let month = rendered.find("month").expect("month is listed");
        let day = rendered.find(r#""day""#).expect("day is listed");
        assert!(month < day, "{rendered}");
        // A filterable dimension advertises its allowlist, which is what makes a valid filter
        // writable without guessing.
        assert!(rendered.contains(r#""filterable":true"#), "{rendered}");
        assert!(rendered.contains("north"), "{rendered}");
    }
}
