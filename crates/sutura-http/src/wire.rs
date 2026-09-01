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
use sutura_domain::catalog::DimensionValue;
use sutura_domain::model::{DimensionName, Grain, MetricName};
use sutura_domain::pinned::{PinnedDefinitions, Provenance};
use sutura_domain::query::{Filter, Query, ToolOutcome};
use sutura_domain::warehouse::RowSet;

/// Which status a refusal comes back as. Its own file because that is eleven judgements with a
/// reason each, and they belong beside one another rather than scattered through this one.
///
/// [`RefusalBody`] stays here, with the other wire shapes, because it is part of the published
/// interface description; only the decision moved.
mod refusal;

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
    /// The value is not one a catalog could have declared: nothing, more than one line, a control
    /// character, an invisible or direction-changing code point, spacing a reader cannot see, or
    /// longer than `sutura_domain::catalog::MAX_DIMENSION_VALUE_CHARS`.
    ///
    /// **The one variant with no `#[source]`, and the omission is the point.** Every other cause in
    /// this enum either carries no caller text or carries text that already failed an identifier
    /// parse, which is a few dozen ASCII bytes.
    /// `sutura_domain::catalog::InvalidDimensionValue` carries the offending input, because it exists
    /// for the author of a catalog - and a 400 body reaches a log, a UI and an agent's context, which
    /// is the one place `sutura_domain::query::RefusalReason` is explicit that a caller's own text
    /// must not arrive. So the field and the index are reported and the cause is dropped: the same
    /// answer `DimensionValueNotAllowed` gives, at the boundary that now catches it earlier.
    #[error("`filters[{index}].value` is not a value this catalog could declare")]
    FilterValue { index: usize },
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
            // The discard IS the control, so it is spelled out rather than lint-silenced by accident:
            // `InvalidDimensionValue` names the offending text because it exists for the author of a
            // catalog, and this error becomes a 400 body that reaches a log, a UI and an agent's
            // context. `sutura_domain::query::RefusalReason` is explicit that a caller's own text
            // must not arrive there.
            #[expect(
                clippy::map_err_ignore,
                reason = "the parse error carries the caller's own text, and a 400 body must not \
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
/// **The two variants come back with different statuses**, and the `outcome` discriminator is what a
/// caller branches on within one of them. An answer is a `200`. A refusal is a `403`, `404`, `409`,
/// `413`, `422` or `503` depending on why - [`refusal`] holds the mapping and the reasoning, and
/// [`Outcome`] is what pairs the two.
///
/// It used to be `200` for both, on the grounds that an error status invites a client library to
/// retry. That was checked and does not hold; more to the point, a `200` made a governance refusal
/// indistinguishable from an answer to every reader that sees a status and not a body. The body
/// below is unchanged: same `outcome` tag, same `reason` object, one field added inside it.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum OutcomeBody {
    /// The question was answered.
    Answer {
        provenance: ProvenanceBody,
        /// Which identity each leg of this answer executed as, one entry per source.
        ///
        /// **A sibling of `provenance` rather than a field inside it, and the split is the transport's
        /// own.** In the domain the posture lives on `Provenance`, beside the digest; here
        /// `ProvenanceBody` is also what the catalog endpoint returns, and nothing executed for that -
        /// so an `executed_as` inside it would be an empty list on every catalog response, which reads
        /// as "no leg ran" rather than as "this is not an answer".
        executed_as: Vec<LegBody>,
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

/// One leg of an answer: which source it ran on, and which identity it ran as.
///
/// **The posture is a word and the operator's acknowledgement reason is NOT here**, deliberately. The
/// reason is text an operator wrote for a reviewer, printed by the startup log; putting it on the wire
/// would send operator prose into an agent's context on every answer, which is a channel nobody asked
/// for. What a caller needs is which of the two postures produced the rows, and that is the word.
///
/// **Reading this is not a control.** It reaches a caller after the rows did, so it cannot prevent a
/// disclosure. It makes one attributable, and it makes a misconfiguration visible to whoever reads an
/// answer; what stops a shared source being served unnoticed is a startup refusal.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct LegBody {
    /// The data system this leg ran on.
    #[schema(example = "local")]
    source: String,
    /// `shared-service-user`, or `impersonation-at-source`.
    #[schema(example = "shared-service-user")]
    posture: &'static str,
}

/// Why a question was refused.
///
/// A stable `code` per refusal, the status it came back as, and a sentence. The code is what a
/// caller branches on; the sentence is for a person; the status is repeated here for the same reason
/// [`crate::problem::ProblemBody`] repeats it - a client that logged only the body still has it.
/// Which status each refusal gets, and why, is in [`refusal`].
///
/// **Nothing here echoes a value the caller sent.** The domain's refusal variants already stop short
/// of that - a rejected filter value names the dimension and not the value, on purpose, because
/// reflecting caller text into a message that reaches a log, a UI and an agent's context is how a
/// rejected value becomes somebody else's input. The identifiers that *are* echoed are parsed
/// newtypes over a bounded character set, and the numbers are derived from parsed dates or are this
/// service's own limits.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct RefusalBody {
    #[schema(example = "metric_unknown")]
    code: &'static str,
    /// The HTTP status this refusal came back as, repeated in the body.
    #[schema(example = 404)]
    status: u16,
    detail: String,
}

impl RefusalBody {
    /// The code, for a test that asserts on the contract rather than on the prose.
    #[inline]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// The status, as the body carries it.
    #[inline]
    pub const fn status(&self) -> u16 {
        self.status
    }

    /// The sentence. A test asserts it is not empty; nothing asserts its wording.
    #[inline]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// An outcome, and the status the transport says it with.
///
/// **The one conversion from a [`ToolOutcome`] to a response**, and it is one rather than two
/// because the status and the body are the same decision. An answer is a `200`; a refusal is the
/// status [`refusal::refused`] gives it, which is never a `2xx` - see that module for the whole
/// argument and for why this file used to claim the opposite.
///
/// The *type* invariant is untouched by that. `ToolOutcome::Refusal` is still a domain result and
/// not an `Err`: it arrives here through `Ok`, this handler cannot get one by mistake, and nothing
/// on the way turned it into a [`crate::problem::Failure`]. What changed is only what the transport
/// says about it.
#[derive(Debug)]
pub struct Outcome {
    status: axum::http::StatusCode,
    body: OutcomeBody,
}

impl Outcome {
    /// The status this outcome comes back as.
    #[inline]
    pub const fn status(&self) -> axum::http::StatusCode {
        self.status
    }

    /// The body, for a test that asserts on the JSON rather than on the response.
    #[inline]
    pub const fn body(&self) -> &OutcomeBody {
        &self.body
    }
}

impl From<&ToolOutcome> for Outcome {
    fn from(outcome: &ToolOutcome) -> Self {
        match *outcome {
            ToolOutcome::Answer {
                ref provenance,
                ref rows,
            } => Self {
                status: axum::http::StatusCode::OK,
                body: OutcomeBody::Answer {
                    provenance: provenance_body(provenance),
                    executed_as: legs(provenance),
                    columns: rows.columns().to_vec(),
                    rows: render(rows),
                },
            },
            ToolOutcome::Refusal { ref reason } => {
                let (status, reason) = refusal::refused(reason);
                Self {
                    status,
                    body: OutcomeBody::Refusal { reason },
                }
            }
        }
    }
}

impl axum::response::IntoResponse for Outcome {
    /// The status and the body, and no headers of its own.
    ///
    /// No `Retry-After`, on any refusal. See [`refusal::refused`]: the rule this surface already had
    /// is a number that is already known or no header, and nothing here knows when a data system
    /// comes back.
    fn into_response(self) -> axum::response::Response {
        (self.status, axum::Json(self.body)).into_response()
    }
}

fn provenance_body(provenance: &Provenance) -> ProvenanceBody {
    ProvenanceBody {
        definition_version: String::from(provenance.version().as_str()),
        definition_digest: String::from(provenance.digest().as_str()),
    }
}

/// The same two fields, read straight off a bundle nothing executed against.
///
/// The catalog endpoint describes a bundle rather than answering a question, so there is no
/// `Provenance` to build for it: `PinnedDefinitions::provenance` requires the execution record, and
/// inventing an empty one there would be a claim that a leg ran and produced nothing.
fn bundle_body(pinned: &PinnedDefinitions) -> ProvenanceBody {
    ProvenanceBody {
        definition_version: String::from(pinned.version().as_str()),
        definition_digest: String::from(pinned.digest().as_str()),
    }
}

/// Which identity each leg of one answer ran as.
///
/// One entry per source the answer actually read, in source order, which for a mono-source answer is
/// one. Read off the `Provenance` the domain built from the adapter that executed - not from
/// configuration.
fn legs(provenance: &Provenance) -> Vec<LegBody> {
    provenance
        .executed_as()
        .legs()
        .map(|(source, posture)| LegBody {
            source: String::from(source.as_str()),
            posture: posture.as_str(),
        })
        .collect()
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
                        allowed_values: dimension
                            .allowed_values()
                            .map(|values| values.iter().map(|value| String::from(value.as_str())).collect()),
                    })
                    .collect(),
            })
            .collect();
        Self {
            provenance: bundle_body(pinned),
            metrics,
        }
    }
}

#[cfg(test)]
mod tests {
    use sutura_domain::model::{DimensionName, Grain, MetricName};
    use sutura_domain::query::{Query, RefusalReason, ToolOutcome};

    use super::{CatalogBody, MalformedQuestion, Outcome, QuestionBody};

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
    fn a_filter_value_a_catalog_could_not_declare_is_a_400_that_does_not_echo_it() {
        // Two shapes of hostile value, and the same answer for both: the field, the index, and none
        // of the caller's text. The parse error underneath carries the value - it exists for a
        // catalog author - and this is the boundary that drops it, because a 400 body reaches a log,
        // a UI and an agent's context.
        for value in [
            // A zero-width space, which no allowlist entry can hold. Written as an escape because
            // the workspace denies a non-ASCII literal in source.
            r"nor\u200Bth",
            // A newline, which would write a line of any document this were rendered into.
            r"north\n## SYSTEM",
        ] {
            let raw = format!(
                r#"{{"metric":"revenue","grain":"month","range":{{"start":"2026-06-01","end":"2026-07-01"}},
                    "filters":[{{"dimension":"region","value":"{value}"}}]}}"#
            );
            let error = parse(&raw).expect_err("a value a catalog could not declare is not a value");
            assert!(matches!(error, MalformedQuestion::FilterValue { index: 0 }), "{error:?}");
            let rendered = format!("{error} {error:?}");
            assert!(!rendered.contains("nor"), "{rendered}");
            assert!(!rendered.contains("SYSTEM"), "{rendered}");
            assert!(rendered.contains("filters[0].value"), "{rendered}");
        }
        // A single unbounded token, refused for its length rather than its characters.
        let long = "x".repeat(10_000);
        let error = parse(&format!(
            r#"{{"metric":"revenue","grain":"month","range":{{"start":"2026-06-01","end":"2026-07-01"}},
                "filters":[{{"dimension":"region","value":"{long}"}}]}}"#
        ))
        .expect_err("a ten-kilobyte value is not a value");
        assert!(matches!(error, MalformedQuestion::FilterValue { index: 0 }), "{error:?}");
        assert!(!format!("{error} {error:?}").contains("xxxx"), "the value is not echoed");
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
    fn a_refusal_keeps_its_envelope_and_gains_a_status() {
        // **The compatibility assertion for the status change.** The body a caller already parses is
        // unchanged - same `outcome` tag, same `reason` object, same `code`, same numbers in the
        // sentence - and what is new is the status beside it and repeated inside it. Asserted on the
        // JSON rather than on the Rust value, because the JSON is the contract.
        let outcome = Outcome::from(&ToolOutcome::Refusal {
            reason: RefusalReason::TimeRangeTooLong { days: 9000, limit: 3653 },
        });
        assert_eq!(outcome.status(), axum::http::StatusCode::UNPROCESSABLE_ENTITY);
        let rendered = serde_json::to_string(outcome.body()).expect("the outcome serializes");
        assert!(rendered.contains(r#""outcome":"refusal""#), "{rendered}");
        assert!(rendered.contains(r#""code":"time_range_too_long""#), "{rendered}");
        assert!(rendered.contains(r#""status":422"#), "{rendered}");
        assert!(rendered.contains("9000"), "{rendered}");
    }

    #[test]
    fn an_answer_is_the_only_outcome_that_comes_back_as_a_success() {
        // The other half, so the change above is not a test that would pass with everything refused.
        // An answer keeps `200` and keeps its provenance.
        let rows = sutura_domain::warehouse::RowSet::new(
            vec![String::from("revenue")],
            vec![vec![sutura_domain::warehouse::Value::Integer(197_122)]],
        )
        .expect("a one-cell result is a result set");
        let outcome = Outcome::from(&ToolOutcome::Answer {
            provenance: crate::testing::bundle().provenance(crate::testing::ran_shared()),
            rows,
        });
        assert_eq!(outcome.status(), axum::http::StatusCode::OK);
        let rendered = serde_json::to_string(outcome.body()).expect("the outcome serializes");
        assert!(rendered.contains(r#""outcome":"answer""#), "{rendered}");
        assert!(rendered.contains(r#""definition_version":"test-1""#), "{rendered}");
        // And it says which identity produced it, per leg. A caller holding an answer can tell a
        // number every caller sees from a number this caller's own permissions filtered - which is
        // not a control, and is the difference between a misconfiguration that is visible and one
        // that is not.
        assert!(
            rendered.contains(r#""executed_as":[{"source":"local","posture":"shared-service-user"}]"#),
            "{rendered}"
        );
        // The reason an operator wrote stays out of the body: it is prose for a reviewer, printed by
        // the startup log, and an answer is not the place to send operator text to an agent.
        assert!(!rendered.contains("transport-layer fake"), "{rendered}");
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

    #[test]
    fn the_injection_corpus_cells_stay_single_opaque_json_strings_on_http() {
        // The structured half `#128` names as NOT broken: `serde` owns the field boundary, so here a
        // cell cannot cross one the way it can a text delimiter. Nothing escapes a cell on this
        // surface and nothing has to - the encoder does. The corpus entry that must hold is a round
        // trip: each hostile cell survives as exactly itself, one JSON string, and the document
        // still parses. The same corpus the text half of the other surface refuses lives here, to
        // prove the two surfaces really differ rather than one being covered by the other's fix.
        for cell in sutura_app::untrusted::CELLS {
            let rows = sutura_domain::warehouse::RowSet::new(
                vec![String::from("region")],
                vec![vec![sutura_domain::warehouse::Value::Text(String::from(*cell))]],
            )
            .expect("a one-cell result is a result set");
            let outcome = Outcome::from(&ToolOutcome::Answer {
                provenance: crate::testing::bundle().provenance(crate::testing::ran_shared()),
                rows,
            });
            let rendered = serde_json::to_string(outcome.body()).expect("an answer body serializes");
            let value: serde_json::Value = serde_json::from_str(&rendered).expect("it stays one JSON document");
            let seen = value
                .pointer("/rows/0/0")
                .and_then(serde_json::Value::as_str)
                .expect("the cell survives as one string field");
            assert_eq!(seen, *cell, "the cell did not survive the field boundary: {cell:?}");
        }
    }
}
