//! The tool's arguments and its result, and the conversions to and from the domain.
//!
//! # Why this crate has its own wire type
//!
//! `sutura-http` already holds one - `QuestionBody`, with the same five fields and the same
//! `TryFrom<..> for Query`. Sharing it would mean this adapter depending on that one, and *an
//! adapter never calls another adapter* is the rule the whole layout rests on: a shape owned by one
//! transport is a shape every other transport has to reach through it. [`CatalogContent`] is the
//! same story against `sutura_http::wire::CatalogBody`.
//!
//! **So the duplication is deliberate, and it is a cost rather than an oversight.** Nothing in the
//! compiler makes two wire types stay equal. What guards them is
//! [`AskArgs`]'s own `deny_unknown_fields`, asserted through the transport in `crate::server`, plus
//! the committed schema dump in [`crate::tool`] - a widened input changes a snapshot and the
//! byte-compare fails until somebody re-accepts it, which is what puts a new field in a reviewer's
//! diff.
//!
//! **What is now mechanical across the two transports is the TOOL SET, and not these shapes.**
//! `sutura_app::Capability` is the one source both of them render, and
//! `both_transports_describe_the_same_tools` in `crate::tool` is the assertion. The field lists of
//! two wire types with the same job are still kept equal by review, and that limit is worth keeping
//! in front of a reader rather than letting the tool-set test read as covering it.
//!
//! # Why the derive is here and not on `Query`
//!
//! The schema has to be generated - a hand-written tool schema and a hand-written parser drift, and
//! the drift is invisible until a caller trusts the schema. The derive that generates it is
//! `schemars`, and `sutura-domain`'s whole transitive tree is walked by `cargo xtask
//! check-boundaries` against an allowlist, so putting a macro crate on a domain type is an
//! architecture decision rather than a convenience. The derive goes on the wire type, which is
//! where a wire format belongs anyway.
//!
//! A wire type is also allowed to be *worse* than a domain type, and should be. Every field of
//! [`AskArgs`] is a plain string, because that is what arrives; each one is then parsed into the
//! newtype that establishes its invariant, and a failure names the field.
//!
//! # `deny_unknown_fields`, and what it is for here
//!
//! The governance boundary, across a JSON parser. Without it an arguments object carrying `sql:` or
//! `table:` deserializes cleanly with the extra key dropped on the floor, and a model that believes
//! it sent SQL is answered as though it had asked the modelled question instead. With it, the
//! attempt is a named parse error. `sutura_domain::query::Query` has no field for any of that; this
//! is what keeps that true on the way in.
//!
//! It is also what `schemars` reads to emit `additionalProperties: false`, so the advertised schema
//! and the enforcement come from the same attribute rather than from two decisions that could
//! disagree.

use sutura_domain::calendar::{Date, TimeRange};
use sutura_domain::catalog::DimensionValue;
use sutura_domain::model::{DimensionName, Grain, MetricName};
use sutura_domain::pinned::{PinnedDefinitions, Provenance};
use sutura_domain::query::{Filter, Query, ToolOutcome};
use sutura_domain::warehouse::RowSet;

use crate::refusal;

/// One governed question, as a tool call carries it.
///
/// The five fields are the whole input surface of this deployment. There is no field for SQL, a
/// table, a filter expression or a row-id list, and an argument naming one is a parse error rather
/// than a key that gets ignored.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AskArgs {
    /// The metric to measure, by the name this catalog defines it under - for example `revenue`.
    metric: String,
    /// The time resolution to aggregate to: one of `day`, `week`, `month`, `quarter` or `year`, and
    /// only those the metric declares.
    grain: String,
    /// The half-open period to ask about.
    range: RangeArgs,
    /// What to group the answer by. At most four, and each must be a dimension the metric declares.
    #[serde(default)]
    dimensions: Vec<String>,
    /// Equality filters, each on a dimension the metric declares as filterable.
    #[serde(default)]
    filters: Vec<FilterArgs>,
}

/// A half-open period: `start` is included, `end` is not.
///
/// Half-open at every grain, which is what makes a month `[2026-06-01, 2026-07-01)` rather than a
/// last day that differs per month. Both dates are ISO `YYYY-MM-DD`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RangeArgs {
    /// The first day included, as `YYYY-MM-DD` - for example `2026-06-01`.
    start: String,
    /// The first day NOT included, as `YYYY-MM-DD` - for example `2026-07-01`.
    end: String,
}

/// One equality filter.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilterArgs {
    /// The dimension to filter on, by the name the metric declares it under.
    dimension: String,
    /// The value it must equal. Where the catalog declares an allowlist, only a declared value is
    /// accepted.
    value: String,
}

/// Why an arguments object is not a question.
///
/// Every variant names the field, and none of them echoes the caller's value back except where the
/// value is the thing that failed to parse as an identifier - which is a bounded character set, not
/// free text.
#[derive(Debug, thiserror::Error)]
pub enum MalformedQuestion {
    /// The arguments object did not deserialize at all: a missing field, a wrong type, or - the case
    /// this crate cares about most - a field the tool surface does not declare.
    #[error("the arguments are not a question")]
    NotAnObject {
        #[source]
        cause: serde_json::Error,
    },
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
    /// The value is not one a catalog could have declared: nothing, more than one line, a control
    /// character, an invisible or direction-changing code point, spacing a reader cannot see, or
    /// longer than `sutura_domain::catalog::MAX_DIMENSION_VALUE_CHARS`.
    ///
    /// **The one variant with no `#[source]`, and the omission is the point.**
    /// `sutura_domain::catalog::InvalidDimensionValue` carries the offending input, because it
    /// exists for the author of a catalog - and this error reaches a log and a model's own context,
    /// which is the one place `sutura_domain::query::RefusalReason` is explicit that a caller's own
    /// text must not arrive. So the field and the index are reported and the cause is dropped.
    #[error("`filters[{index}].value` is not a value this catalog could declare")]
    FilterValue { index: usize },
}

impl TryFrom<AskArgs> for Query {
    type Error = MalformedQuestion;

    fn try_from(args: AskArgs) -> Result<Self, Self::Error> {
        let metric = MetricName::parse(&args.metric).map_err(|cause| MalformedQuestion::Metric { cause })?;
        let grain = grain_of(&args.grain)?;
        let range = range_of(&args.range)?;
        let mut dimensions = Vec::with_capacity(args.dimensions.len());
        for (index, raw) in args.dimensions.iter().enumerate() {
            dimensions.push(DimensionName::parse(raw).map_err(|cause| MalformedQuestion::Dimension { index, cause })?);
        }
        let mut filters = Vec::with_capacity(args.filters.len());
        for (index, raw) in args.filters.iter().enumerate() {
            let dimension =
                DimensionName::parse(&raw.dimension).map_err(|cause| MalformedQuestion::FilterDimension { index, cause })?;
            // The discard IS the control, so it is spelled out rather than lint-silenced by accident:
            // `InvalidDimensionValue` names the offending text because it exists for the author of a
            // catalog, and this error reaches a log and a model's context.
            #[expect(
                clippy::map_err_ignore,
                reason = "the parse error carries the caller's own text, and a tool error must not \
                          reflect it back - see MalformedQuestion::FilterValue"
            )]
            let value = DimensionValue::parse(&raw.value).map_err(|_| MalformedQuestion::FilterValue { index })?;
            filters.push(Filter::new(dimension, value));
        }
        Ok(Self::new(metric, grain, range, dimensions, filters))
    }
}

/// The grain, from its name.
///
/// A hand-written match rather than the derived `Deserialize`, so the error names the accepted set
/// instead of quoting serde at a model that then has to guess.
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

fn range_of(args: &RangeArgs) -> Result<TimeRange, MalformedQuestion> {
    let start = Date::parse(&args.start).map_err(|cause| MalformedQuestion::Date { field: "start", cause })?;
    let end = Date::parse(&args.end).map_err(|cause| MalformedQuestion::Date { field: "end", cause })?;
    TimeRange::new(start, end).map_err(|cause| MalformedQuestion::Range { cause })
}

// ------------------------------------------------------------------ result ----

/// What a question produced, as the tool's structured content.
///
/// **One shape, and the rows are IN it.** A handle-plus-fetch result was considered and withdrawn:
/// the rule it came from is that a federated join is done by the engine rather than by the model,
/// which `docs/adr/0007` already decides above this port, and read as a context-window rule it would
/// have cost an agent the ability to answer a question about a number without a second call.
///
/// The `outcome` discriminator is what a client branches on, and it is the same tag and the same
/// `reason` object the HTTP surface serializes - deliberately, so the two transports describe one
/// answer even though the types are separate.
#[derive(Debug, serde::Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum OutcomeContent {
    /// The question was answered.
    Answer {
        provenance: ProvenanceContent,
        /// Column labels, in projection order.
        columns: Vec<String>,
        /// Rows, each as many cells as there are columns. Every cell is rendered as text by the one
        /// function the domain uses for it, so a number here reads the same way it does in the
        /// anchor that certified it.
        rows: Vec<Vec<String>>,
    },
    /// The question was refused. **Still an `Ok`, and still a tool RESULT** - see
    /// `crate::server::AgentSurface::call_tool`.
    Refusal { reason: RefusalContent },
}

/// Which definitions produced this answer.
///
/// Always present on an answer, and it is the reason an answer can be trusted at all: the version
/// names the snapshot and the digest is over its canonical form, so the same question against a
/// different bundle is visibly a different answer.
#[derive(Debug, serde::Serialize)]
pub struct ProvenanceContent {
    definition_version: String,
    definition_digest: String,
}

/// Why a question was refused: a stable code a caller branches on, and a sentence a person reads.
///
/// **Nothing here echoes a value the caller sent.** The domain's refusal variants already stop short
/// of that - a rejected filter value names the dimension and not the value, because reflecting
/// caller text into a message that reaches a log and a model's context is how a rejected value
/// becomes somebody else's input.
#[derive(Debug, serde::Serialize)]
pub struct RefusalContent {
    code: &'static str,
    detail: String,
}

impl RefusalContent {
    /// The code, for a test that asserts on the contract rather than on the prose.
    #[inline]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// The sentence. A test asserts it is not empty; nothing asserts its wording.
    #[inline]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl From<&ToolOutcome> for OutcomeContent {
    fn from(outcome: &ToolOutcome) -> Self {
        match *outcome {
            ToolOutcome::Answer {
                ref provenance,
                ref rows,
            } => Self::Answer {
                provenance: provenance_content(provenance),
                columns: rows.columns().to_vec(),
                rows: render(rows),
            },
            ToolOutcome::Refusal { ref reason } => {
                let (code, detail) = refusal::refused(reason);
                Self::Refusal {
                    reason: RefusalContent { code, detail },
                }
            }
        }
    }
}

impl OutcomeContent {
    /// The same outcome as text, for the content block beside the structured one.
    ///
    /// **Both are sent, and neither is redundant.** A client that reads `structuredContent` gets the
    /// typed shape above; a client that only renders content blocks - which is most of what a person
    /// actually looks at - would otherwise be shown nothing at all. The plan for this surface says
    /// so in as many words: an agent asked for a number must be able to answer without a second
    /// fetch, and a plain client must have something to display.
    ///
    /// Tab-separated, header row first, because that is the shape a model reads back without being
    /// told how. Nothing here is truncated: a result that would have been too large was refused
    /// before it reached this function.
    pub(crate) fn as_text(&self) -> String {
        match *self {
            Self::Answer {
                ref provenance,
                ref columns,
                ref rows,
            } => {
                let mut out = String::new();
                out.push_str(&columns.join("\t"));
                for row in rows {
                    out.push('\n');
                    out.push_str(&row.join("\t"));
                }
                out.push_str("\n\ndefinitions: ");
                out.push_str(&provenance.definition_version);
                out.push_str(" (digest ");
                out.push_str(&provenance.definition_digest);
                out.push(')');
                out
            }
            // The code as well as the sentence, because the code is the part that is stable and the
            // part an agent can be told to act on.
            Self::Refusal { ref reason } => format!("refused ({}): {}", reason.code, reason.detail),
        }
    }
}

fn provenance_content(provenance: &Provenance) -> ProvenanceContent {
    ProvenanceContent {
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

// --------------------------------------------------------------- catalog ----

// **A type with no fields rather than no type at all**, and the reason is `deny_unknown_fields`.
// `schemars` reads the same attribute serde does, so the advertised schema says
// `additionalProperties: false` and an arguments object carrying `metric`, `sql` or anything else is
// a named parse error rather than a key dropped on the floor. A tool that accepted any object would
// be a tool whose surface a caller could guess at, and a caller that sent `metric` would believe it
// had narrowed a listing it did not.
//
// It is also what keeps this half of the surface under the same drift guard as the other: the
// generated schema is snapshotted in `crate::tool`, so a field added here lands in a reviewer's diff.
//
// **A plain comment and not a doc comment, deliberately.** `schemars` puts a root doc comment into
// the schema's `description`, which is text a MODEL reads before it calls the tool - so the doc
// comment on a wire type is caller-facing prose and the reasoning about the type goes here. The one
// below is written for that reader.
/// This tool takes no arguments. It returns the whole of what this deployment measures, and there is
/// nothing to filter or select: send an empty object.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::empty_structs_with_brackets,
    reason = "the braces are the behaviour, not the formatting: a UNIT struct's Deserialize accepts \
              `null` and REFUSES `{}` - measured, `invalid type: map, expected unit struct` - and `{}` \
              is what a client with no arguments sends. The lint's suggestion breaks the tool"
)]
pub struct DescribeCatalogArgs {}

/// What this deployment measures, as the catalog tool's structured content.
///
/// **A second wire type beside `sutura_http::wire::CatalogBody`, with the same fields, and that is
/// the same deliberate cost [`AskArgs`] already pays.** An adapter never calls another adapter, so
/// this crate cannot import that shape; what keeps the two equal is review plus the fact that both
/// are built from the one `sutura_domain::pinned::PinnedDefinitions` accessor set, which is where a
/// missing field would show up as a missing call rather than as a silent divergence.
///
/// Descriptive content only. `sutura_domain::pinned::SemanticCatalog::load` takes no request context
/// and cannot be given one, so nothing a caller sends selects, widens or parameterizes what this
/// returns: it is the *pinned* bundle, the same one every answer is computed from.
#[derive(Debug, serde::Serialize)]
pub struct CatalogContent {
    /// Which snapshot this listing describes. The same version and digest an answer carries, so a
    /// model can tell that the metric it read about is the metric it measured.
    provenance: ProvenanceContent,
    metrics: Vec<MetricContent>,
}

/// One metric, as much of it as a caller needs to ask a valid question.
#[derive(Debug, serde::Serialize)]
pub struct MetricContent {
    name: String,
    description: String,
    /// Coarsest first, which is the order an anchor is checked at.
    grains: Vec<String>,
    dimensions: Vec<DimensionContent>,
}

/// One dimension of one metric.
#[derive(Debug, serde::Serialize)]
pub struct DimensionContent {
    name: String,
    description: String,
    /// Whether this dimension can be filtered on as well as grouped by.
    filterable: bool,
    /// The values a filter may use, where the catalog declares a set. Absent means groupable and not
    /// filterable.
    #[serde(skip_serializing_if = "Option::is_none")]
    allowed_values: Option<Vec<String>>,
}

impl From<&PinnedDefinitions> for CatalogContent {
    fn from(pinned: &PinnedDefinitions) -> Self {
        let metrics = pinned
            .definitions()
            .metrics()
            .values()
            .map(|metric| MetricContent {
                name: String::from(metric.name().as_str()),
                description: String::from(metric.description()),
                grains: {
                    let mut grains: Vec<Grain> = metric.grains().iter().copied().collect();
                    // Reversed, because `Grain`'s own ordering runs fine to coarse and a reader wants
                    // the coarsest first - the same order the HTTP surface renders.
                    grains.sort_unstable_by(|left, right| right.cmp(left));
                    grains.into_iter().map(|grain| String::from(grain.as_str())).collect()
                },
                dimensions: metric
                    .dimensions()
                    .values()
                    .map(|dimension| DimensionContent {
                        name: String::from(dimension.name().as_str()),
                        description: String::from(dimension.description()),
                        filterable: dimension.is_filterable(),
                        allowed_values: dimension
                            .allowed_values()
                            .map(|values| values.iter().map(|value| String::from(value.as_str())).collect()),
                    })
                    .collect(),
            })
            .collect();
        Self {
            provenance: provenance_content(&pinned.provenance()),
            metrics,
        }
    }
}

impl CatalogContent {
    /// The same listing as text, for the content block beside the structured one.
    ///
    /// Both are sent for the reason [`OutcomeContent::as_text`] gives: a client that renders only
    /// content blocks - which is most of what a person actually looks at - would otherwise be shown
    /// nothing.
    ///
    /// One metric per line, then its grains and its dimensions, because a model reads that back
    /// without being told how. Nothing here is truncated: the bundle is bounded at load by
    /// `sutura_domain::knowledge::MAX_KNOWLEDGE_BYTES` and by the catalog's own parses, and a listing
    /// that grew past what a context tolerates is a bundle nobody could ask about either way.
    pub(crate) fn as_text(&self) -> String {
        let mut out = String::new();
        for metric in &self.metrics {
            out.push_str(&metric.name);
            out.push_str(" - ");
            out.push_str(&metric.description);
            out.push_str("\n  grains: ");
            out.push_str(&metric.grains.join(", "));
            for dimension in &metric.dimensions {
                out.push_str("\n  dimension ");
                out.push_str(&dimension.name);
                out.push_str(if dimension.filterable {
                    " (groupable, filterable"
                } else {
                    " (groupable"
                });
                match dimension.allowed_values {
                    Some(ref values) => {
                        out.push_str(", values: ");
                        out.push_str(&values.join(", "));
                    }
                    None => out.push_str(", any value"),
                }
                out.push_str(") - ");
                out.push_str(&dimension.description);
            }
            out.push('\n');
        }
        out.push_str("\ndefinitions: ");
        out.push_str(&self.provenance.definition_version);
        out.push_str(" (digest ");
        out.push_str(&self.provenance.definition_digest);
        out.push(')');
        out
    }
}

#[cfg(test)]
mod tests {
    use sutura_domain::model::{DimensionName, Grain, MetricName};
    use sutura_domain::query::{Query, RefusalReason, ToolOutcome};

    use super::{AskArgs, MalformedQuestion, OutcomeContent};

    fn parse(json: &str) -> Result<Query, MalformedQuestion> {
        let args: AskArgs = serde_json::from_str(json).map_err(|cause| MalformedQuestion::NotAnObject { cause })?;
        Query::try_from(args)
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
    fn a_malformed_field_names_the_field_it_was() {
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
    }

    #[test]
    fn a_filter_value_a_catalog_could_not_declare_does_not_come_back_in_the_error() {
        // A zero-width space, which no allowlist entry can hold. Written as a JSON escape inside a
        // raw string - so the source stays ASCII, which this workspace denies departing from, and
        // the JSON parser is what produces the code point.
        let error = parse(
            r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},
                "filters":[{"dimension":"region","value":"nor\u200Bth"}]}"#,
        )
        .expect_err("a value a catalog could not declare is not a value");
        assert!(matches!(error, MalformedQuestion::FilterValue { index: 0 }), "{error:?}");
        let rendered = format!("{error} {error:?}");
        assert!(!rendered.contains("nor"), "{rendered}");
        assert!(rendered.contains("filters[0].value"), "{rendered}");
    }

    #[test]
    fn a_range_with_no_end_does_not_deserialize_at_all() {
        // The bound the type provides rather than the service: there is no unbounded form to send.
        let error = parse(r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01"}}"#)
            .expect_err("a range with no end is not a range");
        assert!(error.to_string().contains("question"), "{error}");
    }

    #[test]
    fn a_refusal_serializes_with_its_code_and_its_numbers() {
        let content = OutcomeContent::from(&ToolOutcome::Refusal {
            reason: RefusalReason::TimeRangeTooLong { days: 9000, limit: 3653 },
        });
        let rendered = serde_json::to_string(&content).expect("the outcome serializes");
        assert!(rendered.contains(r#""outcome":"refusal""#), "{rendered}");
        assert!(rendered.contains(r#""code":"time_range_too_long""#), "{rendered}");
        assert!(rendered.contains("9000"), "{rendered}");
        // And the text block a plain client renders says the same thing.
        assert!(content.as_text().starts_with("refused (time_range_too_long):"), "{content:?}");
    }

    #[test]
    fn an_answer_carries_its_provenance_in_both_halves_of_the_result() {
        let rows = sutura_domain::warehouse::RowSet::new(
            vec![String::from("revenue")],
            vec![vec![sutura_domain::warehouse::Value::Integer(197_122)]],
        )
        .expect("a one-cell result is a result set");
        let content = OutcomeContent::from(&ToolOutcome::Answer {
            provenance: crate::testing::bundle().provenance(),
            rows,
        });
        let rendered = serde_json::to_string(&content).expect("the outcome serializes");
        assert!(rendered.contains(r#""outcome":"answer""#), "{rendered}");
        assert!(rendered.contains(r#""definition_version":"test-1""#), "{rendered}");
        assert!(rendered.contains("197122"), "{rendered}");
        let text = content.as_text();
        assert!(text.starts_with("revenue\n197122"), "{text}");
        assert!(text.contains("definitions: test-1"), "{text}");
    }
}
