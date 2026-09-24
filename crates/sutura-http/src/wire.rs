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

use sutura_domain::model::Grain;
use sutura_domain::pinned::view::ScopedView;
use sutura_domain::pinned::{PinnedDefinitions, Provenance};
use sutura_domain::query::{Query, ToolOutcome};
use sutura_domain::question::RawFilter;
use sutura_domain::warehouse::RowSet;

/// Why a body is not a question.
///
/// **Mostly owned by `sutura-domain::question`, not by this transport.** HTTP's and MCP's field
/// sets and typed refusals were identical - kept equal only by review - so the parse moved inward
/// of both; [`Self::Question`] is what every existing reference to the domain's own
/// `MalformedQuestion` in this crate now wraps.
///
/// **No longer a bare alias**, since `telekom/sutura#778`: a relative `range` needs a failure mode
/// the domain does not have and must not gain - [`Self::Range`] wraps
/// `sutura_runtime::relative_range::RangeResolutionError`, the resolver both transports share.
#[derive(Debug, thiserror::Error)]
pub enum MalformedQuestion {
    #[error(transparent)]
    Question(#[from] sutura_domain::question::MalformedQuestion),
    #[error(transparent)]
    Range(#[from] sutura_runtime::relative_range::RangeResolutionError),
}

/// Which status a refusal comes back as. Its own file because that is eleven judgements with a
/// reason each, and they belong beside one another rather than scattered through this one.
///
/// [`RefusalBody`] stays here, with the other wire shapes, because it is part of the published
/// interface description; only the decision moved.
mod refusal;

/// The raw SQL tool's own wire shape, kept apart from every certified shape above for the reason
/// its own module documentation gives.
pub mod raw;
pub use raw::{MalformedStatement as RawMalformedStatement, RawOutcomeBody, RunSqlBody, RunSqlOutcome};

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
    /// Filters, each on a dimension the metric declares as filterable: `in` a set of declared
    /// values, or `not_in` it.
    #[serde(default)]
    filters: Vec<FilterBody>,
    /// An order and a caller-chosen row limit, bounding a wide group-by instead of asking for
    /// every group.
    top: Option<TopBody>,
}

/// A `top` clause: rank by `by`, in `direction`, keep the first `n`.
#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TopBody {
    /// How many groups to return. Must be positive, and no more than this deployment will
    /// certify - a larger count is the same refusal an unbounded question over that many groups
    /// already gets.
    #[schema(example = 10)]
    n: u32,
    /// `metric` ranks by the question's own measure; `period` ranks by the time bucket.
    #[schema(example = "metric")]
    by: String,
    /// `desc` for the largest first, `asc` for the smallest.
    #[schema(example = "desc")]
    direction: String,
}

/// A half-open period: `start` is included, `end` is not. Either an absolute period
/// (`start`/`end`) or a period relative to today (`last`) - never both, never neither.
///
/// Half-open at every grain and in every dialect, which is what makes a month
/// `[2026-06-01, 2026-07-01)` rather than a last day that differs per month.
#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RangeBody {
    /// The first day included, as `YYYY-MM-DD`. Mutually exclusive with `last`.
    #[schema(example = "2026-06-01")]
    start: Option<String>,
    /// The first day NOT included, as `YYYY-MM-DD`. Mutually exclusive with `last`.
    #[schema(example = "2026-07-01")]
    end: Option<String>,
    /// A period ending today (or yesterday, unless `include_current`), resolved against this
    /// deployment's own clock. Mutually exclusive with `start`/`end`.
    last: Option<LastBody>,
}

/// A count of calendar periods before today, resolved at request time rather than authored as
/// dates - `telekom/sutura#778`.
#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LastBody {
    /// How many `unit`s back. Must not be zero.
    #[schema(example = 1)]
    count: u32,
    /// One of `day`, `week`, `month`, `quarter` or `year`.
    #[schema(example = "month")]
    unit: String,
    /// Whether today's own, possibly partial, period is included.
    #[serde(default)]
    include_current: bool,
}

/// One filter: a dimension, which way it compares, and the values it compares against.
#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FilterBody {
    #[schema(example = "region")]
    dimension: String,
    /// `in` or `not_in`.
    #[schema(example = "in")]
    op: String,
    /// The dimension's value must be one of these (`in`) or none of these (`not_in`). At least
    /// one, each a value the metric declares.
    values: Vec<String>,
}

impl TryFrom<QuestionBody> for Query {
    type Error = MalformedQuestion;

    /// Resolves `range` against the shipping wall clock, then hands the two ISO dates - and every
    /// other field, unchanged - to `sutura_domain::question::parse_query`. Production's only clock;
    /// [`query_of`] is what a test substitutes a fixed one into.
    fn try_from(body: QuestionBody) -> Result<Self, Self::Error> {
        query_of(body, &sutura_runtime::relative_range::SystemClock)
    }
}

/// The whole of [`TryFrom::try_from`], generic in the clock so a test can fix "today" without
/// resolving against the day it happens to run on.
fn query_of(body: QuestionBody, clock: &impl sutura_runtime::relative_range::WallClock) -> Result<Query, MalformedQuestion> {
    let filters: Vec<RawFilter<'_>> = body
        .filters
        .iter()
        .map(|filter| RawFilter::new(&filter.dimension, &filter.op, &filter.values))
        .collect();
    let last = body
        .range
        .last
        .map(|last| sutura_runtime::relative_range::LastWire::new(last.count, last.unit, last.include_current));
    let (start, end) = sutura_runtime::relative_range::resolve_range(clock, body.range.start, body.range.end, last)?;
    let top = body
        .top
        .as_ref()
        .map(|top| sutura_domain::question::RawTop::new(top.n, &top.by, &top.direction));
    Ok(sutura_domain::question::parse_query(
        &body.metric,
        &body.grain,
        &start,
        &end,
        &body.dimensions,
        &filters,
        top,
    )?)
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
    retry_after_seconds: Option<u64>,
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
                retry_after_seconds: None,
            },
            ToolOutcome::Refusal { ref reason } => {
                let retry_after_seconds = refusal::retry_after(reason);
                let (status, reason) = refusal::refused(reason);
                Self {
                    status,
                    retry_after_seconds,
                    body: OutcomeBody::Refusal { reason },
                }
            }
        }
    }
}

impl axum::response::IntoResponse for Outcome {
    /// The status and the body, plus a `Retry-After` where the refusal names a fact rather than a
    /// guess.
    ///
    /// **Two arms, not an always-present header with a sentinel** - the same shape
    /// `crate::problem::Failure::into_response` already uses, and for the same reason:
    /// `Retry-After: 0` is a promise the next request will be answered, and a header that is
    /// sometimes invented is worse than one that is sometimes absent. Every refusal but
    /// `budget_exhausted` carries `None` here: nothing else on this surface knows when its answer
    /// changes, and [`refusal::retry_after`] is the one place that decides which does.
    fn into_response(self) -> axum::response::Response {
        let outcome = match &self.body {
            OutcomeBody::Answer { rows, .. } => crate::metrics::QuestionOutcome::answered(rows.len()),
            OutcomeBody::Refusal { reason } => crate::metrics::QuestionOutcome::refused(reason.code()),
        };
        let mut response = match self.retry_after_seconds {
            Some(seconds) => (
                self.status,
                [(axum::http::header::RETRY_AFTER, seconds.to_string())],
                axum::Json(self.body),
            )
                .into_response(),
            None => (self.status, axum::Json(self.body)).into_response(),
        };
        response.extensions_mut().insert(outcome);
        response
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
///
/// # Why a structured surface reads the prose setting at all
///
/// `prompt.catalog_prose: omitted` is not a mitigation for the forgery `docs/adr/0022` is about -
/// `serde` owns the field boundary here, so a description cannot cross one whatever it spells, and
/// this body escapes nothing. It is a decision about **who may put words in front of an agent**: an
/// operator whose catalog authors are not the people who decide what their agents are told drops the
/// prose, and a description that still reached an agent through a second transport would make that
/// setting a statement about one surface rather than about the deployment. So the omission is
/// honoured wherever the prose is carried, and the escaping stays where the delimiter is.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct CatalogBody {
    provenance: ProvenanceBody,
    /// Which way the operator's `prompt.catalog_prose` setting points, so an absent description is a
    /// fact a client can read rather than one it has to infer.
    ///
    /// The operator's own spelling, echoed rather than translated, which is what keeps this body and
    /// the settings file from acquiring two vocabularies for one decision - the value is
    /// `sutura_config::CatalogProse::as_str`, so a third spelling cannot appear here without
    /// appearing in the settings parser too. It reads `quoted` on a surface that quotes nothing
    /// because the word names the SETTING and not this renderer.
    catalog_prose: &'static str,
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
    /// The author's own prose, or absent where the operator omitted it.
    ///
    /// `Option` rather than an empty string, because *this deployment ships no catalog prose* and
    /// *this metric's description is empty* are different facts, and a client rendering the second
    /// for the first would report an operator's decision as a catalog defect. `catalog_prose` on the
    /// body is what says which an absence is.
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    /// Coarsest first, which is the order an anchor is checked at.
    grains: Vec<String>,
    dimensions: Vec<DimensionBody>,
}

/// One dimension of one metric.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct DimensionBody {
    name: String,
    /// The author's own prose, or absent where the operator omitted it. See [`MetricBody`].
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    /// Whether this dimension can be filtered on as well as grouped by.
    filterable: bool,
    /// The values a filter may use, when the catalog declares a set. Absent means the dimension is
    /// groupable and not filterable.
    #[serde(skip_serializing_if = "Option::is_none")]
    allowed_values: Option<Vec<String>>,
}

impl CatalogBody {
    /// The reader's view of a caller-scoped catalog, under the prose setting this deployment was
    /// started with.
    ///
    /// **Takes a [`ScopedView`], never a bare `&PinnedDefinitions`** - `docs/adr/0028`. A metric
    /// outside the view is not in `metrics` below, so advertisement and invocation cannot disagree
    /// about which metrics exist; the provenance still names the whole bundle's version and digest,
    /// because that is what `docs/adr/0028` says the digest continues to identify.
    ///
    /// **A named constructor rather than a `From`, and the argument is the reason.**
    /// `CatalogProse::default()` is `Quoted`, so a conversion reachable without the setting fails
    /// OPEN: it ships the prose of a deployment that asked for none, which is the defect this
    /// function exists to close. A second argument cannot be left out.
    #[must_use]
    pub fn of(view: &ScopedView<'_>, prose: sutura_config::CatalogProse) -> Self {
        // Exhaustive rather than `is_quoted()` in an `if`, which is what this line was: a question
        // asked of one variant reads every future spelling as the `else`, and on this setting the
        // `else` withholds prose nobody asked to withhold. `sutura-mcp`'s twin makes the same
        // decision unrepresentable with a field type; this surface has one carrier and no text half,
        // so the match is where the whole of it fits.
        let quoted = match prose {
            sutura_config::CatalogProse::Quoted => true,
            sutura_config::CatalogProse::Omitted => false,
        };
        let metrics = view
            .metrics()
            .map(|metric| MetricBody {
                name: String::from(metric.name().as_str()),
                description: quoted.then(|| String::from(metric.description())),
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
                        description: quoted.then(|| String::from(dimension.description())),
                        filterable: dimension.is_filterable(),
                        allowed_values: dimension
                            .allowed_values()
                            .map(|values| values.iter().map(|value| String::from(value.as_str())).collect()),
                    })
                    .collect(),
            })
            .collect();
        Self {
            provenance: bundle_body(view.pinned()),
            catalog_prose: prose.as_str(),
            metrics,
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use axum::response::IntoResponse as _;
    use sutura_domain::model::{DimensionName, Grain, MetricName};
    use sutura_domain::pinned::view::ScopedView;
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
                "dimensions":["region"],"filters":[{"dimension":"region","op":"in","values":["north"]}]}"#,
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
        use sutura_domain::question::MalformedQuestion as SharedMalformedQuestion;

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
                    "filters":[{{"dimension":"region","op":"in","values":["{value}"]}}]}}"#
            );
            let error = parse(&raw).expect_err("a value a catalog could not declare is not a value");
            assert!(
                matches!(
                    error,
                    MalformedQuestion::Question(SharedMalformedQuestion::FilterValue { index: 0, .. })
                ),
                "{error:?}"
            );
            let rendered = format!("{error} {error:?}");
            assert!(!rendered.contains("nor"), "{rendered}");
            assert!(!rendered.contains("SYSTEM"), "{rendered}");
            assert!(rendered.contains("filters[0].value"), "{rendered}");
        }
        // A single unbounded token, refused for its length rather than its characters.
        let long = "x".repeat(10_000);
        let error = parse(&format!(
            r#"{{"metric":"revenue","grain":"month","range":{{"start":"2026-06-01","end":"2026-07-01"}},
                "filters":[{{"dimension":"region","op":"in","values":["{long}"]}}]}}"#
        ))
        .expect_err("a ten-kilobyte value is not a value");
        assert!(
            matches!(
                error,
                MalformedQuestion::Question(SharedMalformedQuestion::FilterValue { index: 0, .. })
            ),
            "{error:?}"
        );
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
    fn a_spent_budget_reaches_the_response_as_429_with_a_retry_after_header() {
        // `docs/adr/0030`'s own claim, at the layer that builds the actual response: the ONE
        // refusal on this surface whose `Outcome::into_response` attaches a `Retry-After`, over the
        // conversion `axum::serve` uses for real - not a helper this test writes its own copy of.
        let outcome = ToolOutcome::Refusal {
            reason: RefusalReason::BudgetExhausted { reset_after_seconds: 41 },
        };
        let response = Outcome::from(&outcome).into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry_after = response
            .headers()
            .get(axum::http::header::RETRY_AFTER)
            .expect("a spent budget must carry a Retry-After");
        assert_eq!(retry_after, "41");
    }

    #[test]
    fn an_ordinary_refusal_carries_no_retry_after() {
        // The control for the cell above: every OTHER refusal invents no header, which is the
        // property `Outcome::into_response`'s own doc comment now states.
        let response = Outcome::from(&ToolOutcome::Refusal {
            reason: RefusalReason::MetricUnknown {
                metric: MetricName::parse("revenue").expect("a test metric is a metric"),
            },
        })
        .into_response();
        assert!(
            response.headers().get(axum::http::header::RETRY_AFTER).is_none(),
            "an ordinary refusal must not invent a Retry-After"
        );
    }

    #[test]
    fn a_malformed_field_names_the_field_it_was() {
        use sutura_domain::question::MalformedQuestion as SharedMalformedQuestion;

        // A caller fixing a request needs to know which field, and a serde message does not say.
        let error = parse(r#"{"metric":"revenue","grain":"fortnight","range":{"start":"2026-06-01","end":"2026-07-01"}}"#)
            .expect_err("`fortnight` is not a grain");
        assert!(
            matches!(error, MalformedQuestion::Question(SharedMalformedQuestion::Grain)),
            "{error:?}"
        );
        assert!(error.to_string().contains("quarter"), "{error}");

        let error = parse(r#"{"metric":"revenue","grain":"month","range":{"start":"nope","end":"2026-07-01"}}"#)
            .expect_err("`nope` is not a date");
        let MalformedQuestion::Question(SharedMalformedQuestion::Date { field, .. }) = error else {
            panic!("expected a date failure, got {error:?}");
        };
        assert_eq!(field, "start");

        let error = parse(
            r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"},
                             "dimensions":["region","not a name"]}"#,
        )
        .expect_err("`not a name` is not an identifier");
        let MalformedQuestion::Question(SharedMalformedQuestion::Dimension { index, .. }) = error else {
            panic!("expected a dimension failure, got {error:?}");
        };
        assert_eq!(index, 1);
    }

    #[test]
    fn a_range_with_no_end_and_no_last_is_ambiguous() {
        // `start` with no `end` deserializes now - both are `Option` so a relative range can omit
        // them - and is refused one step later, by `range_choice`, rather than by serde.
        let error = parse(r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01"}}"#)
            .expect_err("a range with no end and no `last` is neither shape");
        assert!(matches!(error, MalformedQuestion::Range(_)), "{error:?}");
        assert!(error.to_string().contains("range"), "{error}");
    }

    #[test]
    fn a_range_naming_both_start_end_and_last_is_ambiguous() {
        let error = parse(
            r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01",
                             "last":{"count":1,"unit":"month"}}}"#,
        )
        .expect_err("both shapes at once is ambiguous");
        assert!(matches!(error, MalformedQuestion::Range(_)), "{error:?}");
    }

    #[test]
    fn a_relative_range_resolves_against_a_fixed_clock() {
        use sutura_domain::calendar::Date;

        struct FixedClock(Date);
        impl sutura_runtime::relative_range::WallClock for FixedClock {
            fn today(&self) -> Result<Date, sutura_runtime::relative_range::ClockUnavailable> {
                Ok(self.0)
            }
        }

        let clock = FixedClock(Date::parse("2026-09-16").expect("a real date"));
        let excluding_today = body(r#"{"metric":"revenue","grain":"month","range":{"last":{"count":1,"unit":"month"}}}"#);
        let query = super::query_of(excluding_today, &clock).expect("a fixed clock resolves a relative range");
        assert_eq!(query.range().start().to_iso(), "2026-08-01");
        assert_eq!(query.range().end().to_iso(), "2026-09-01");

        let including_today = body(
            r#"{"metric":"revenue","grain":"month",
                "range":{"last":{"count":1,"unit":"month","include_current":true}}}"#,
        );
        let query = super::query_of(including_today, &clock).expect("include_current changes the resolved range");
        assert_eq!(query.range().start().to_iso(), "2026-09-01");
        assert_eq!(query.range().end().to_iso(), "2026-09-17");
    }

    /// The refusal proof for an unreadable clock - a predicate exercised with no test of its
    /// refusal is the standard hole this repository watches for.
    #[test]
    fn an_unreadable_clock_is_an_internal_failure_not_a_malformed_question() {
        use sutura_domain::calendar::Date;

        struct BrokenClock;
        impl sutura_runtime::relative_range::WallClock for BrokenClock {
            fn today(&self) -> Result<Date, sutura_runtime::relative_range::ClockUnavailable> {
                Err(sutura_runtime::relative_range::ClockUnavailable::NotADate {
                    cause: Date::from_days_since_epoch(i32::MAX).expect_err("i32::MAX is not a date"),
                })
            }
        }

        let question = body(r#"{"metric":"revenue","grain":"month","range":{"last":{"count":1,"unit":"month"}}}"#);
        let error = super::query_of(question, &BrokenClock).expect_err("the clock never answers");
        assert!(
            matches!(
                error,
                MalformedQuestion::Range(sutura_runtime::relative_range::RangeResolutionError::Clock(_))
            ),
            "{error:?}"
        );
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
    fn a_cross_posture_answer_discloses_both_postures_and_no_acknowledgement_prose() {
        // **`docs/adr/0040`'s one behavioural break, on the wire.** A shared leg beside an
        // impersonating one used to leave here as `409 legs_decide_identity_differently`; it is a
        // `200` whose `executed_as` carries one entry per source with two DIFFERENT posture values.
        // No schema edit and no golden re-record - the field was already a list. **And this body is
        // why the disclosure is not a control:** it serializes `executed_as` beside `rows`, so a
        // caller who reads which leg ran as whom already holds them. The control is the boot-time
        // acknowledgement on each source's own entry.
        let rows = sutura_domain::warehouse::RowSet::new(
            vec![String::from("revenue")],
            vec![vec![sutura_domain::warehouse::Value::Integer(197_122)]],
        )
        .expect("a one-cell result is a result set");
        let outcome = Outcome::from(&ToolOutcome::Answer {
            // Derived from the mono fixture, so this cell and the fixtures cannot disagree.
            provenance: crate::testing::bundle().provenance(
                crate::testing::ran_shared()
                    .and(
                        sutura_domain::model::SourceName::parse("warehouse").expect("a fixture source is a source"),
                        sutura_domain::source::SourcePosture::ImpersonationAtSource,
                    )
                    .expect("two distinct sources are two legs"),
            ),
            rows,
        });
        assert_eq!(outcome.status(), axum::http::StatusCode::OK);
        let rendered = serde_json::to_string(outcome.body()).expect("the outcome serializes");
        assert!(
            rendered.contains(
                r#""executed_as":[{"source":"local","posture":"shared-service-user"},{"source":"warehouse","posture":"impersonation-at-source"}]"#
            ),
            "{rendered}"
        );
        // And the LABELS only. `SourcePosture`'s shared variant carries the operator's own
        // acknowledgement and derives `Serialize`, so a posture VALUE in this body would publish
        // that sentence to every caller - which a mixed answer makes more reachable, not less.
        assert!(!rendered.contains("transport-layer fake"), "{rendered}");
    }

    #[test]
    fn the_catalog_view_lists_grains_coarsest_first_and_carries_the_digest() {
        let bundle = crate::testing::bundle();
        let body = CatalogBody::of(&ScopedView::everything(&bundle), sutura_config::CatalogProse::Quoted);
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
        // And the default setting really carries the prose, so the omission test below is a
        // difference rather than a fixture with nothing in it.
        assert!(rendered.contains("Revenue, in minor units."), "{rendered}");
        assert!(rendered.contains(r#""catalog_prose":"quoted""#), "{rendered}");
    }

    /// `prompt.catalog_prose: omitted` is honoured on the HTTP surface too.
    ///
    /// `#128` asked for `catalog_prose_omitted_omits_it_from_every_surface` and got it on the agent
    /// tool alone: this body carried every description whatever the operator had written down, so a
    /// deployment that had dropped its catalog prose from the prompt and from the MCP tool still
    /// served it here. The setting is not about the delimiter - `serde` owns the boundary on this
    /// surface and no cell or description can cross it - it is about whether a catalog author's
    /// words reach an agent at all, which is a property of the DEPLOYMENT and cannot be true of one
    /// transport and false of another.
    ///
    /// What must survive the omission is everything a caller needs in order to ask a valid question:
    /// the names, the grains, the dimensions and the allowlist. A caller that cannot form a question
    /// gets refusals instead of prose, which is a worse answer to the same worry.
    #[test]
    fn catalog_prose_omitted_omits_it_from_the_http_body() {
        let bundle = crate::testing::bundle();
        let omitted = serde_json::to_value(CatalogBody::of(
            &ScopedView::everything(&bundle),
            sutura_config::CatalogProse::Omitted,
        ))
        .expect("the catalog serializes");
        let rendered = omitted.to_string();
        // No description reaches the caller, on the metric or on the dimension.
        assert!(!rendered.contains("Revenue, in minor units."), "{rendered}");
        assert!(!rendered.contains("Sales region"), "{rendered}");
        assert!(!rendered.contains("description"), "{rendered}");
        // The omission is stated rather than left to be inferred from an absent field, so a client
        // can tell "this deployment ships no prose" from "this catalog has none".
        assert_eq!(omitted["catalog_prose"], "omitted");
        // And the structure a valid question needs is untouched.
        assert_eq!(omitted["metrics"][0]["name"], "revenue");
        assert_eq!(omitted["metrics"][0]["grains"][0], "month");
        assert_eq!(omitted["metrics"][0]["dimensions"][0]["name"], "region");
        assert_eq!(omitted["metrics"][0]["dimensions"][0]["allowed_values"][0], "north");
        assert_eq!(omitted["provenance"]["definition_version"], "test-1");
    }

    /// Every hostile description in the shared corpus stays one opaque JSON string here.
    ///
    /// The structured half of `#128`'s item 2, and the twin of
    /// `the_injection_corpus_cells_stay_single_opaque_json_strings_on_http`: the same corpus
    /// `sutura-app` and `sutura-mcp` walk, asserting the property THIS encoder provides rather than
    /// the one a text surface has to build. A description containing a heading, a fence or a fake
    /// `definitions:` trailer survives as exactly itself inside one field, and the document still
    /// parses - so nothing here needs escaping and nothing here does any.
    ///
    /// The corpus goes through a real `Description`, so what is proved is about prose a catalog can
    /// actually hold; and it is run under BOTH settings, because the interesting failure is a
    /// hostile description that the omission was believed to have dropped.
    #[test]
    fn the_injection_corpus_prose_stays_a_single_opaque_json_string_on_http() {
        for prose in sutura_app::untrusted::PROSE {
            let bundle = crate::testing::described_bundle(prose);
            let quoted = serde_json::to_value(CatalogBody::of(
                &ScopedView::everything(&bundle),
                sutura_config::CatalogProse::Quoted,
            ))
            .expect("the catalog serializes");
            let seen = quoted["metrics"][0]["description"]
                .as_str()
                .expect("the description survives as one string field");
            assert_eq!(seen, *prose, "a corpus description did not survive the field boundary");
            let omitted = serde_json::to_value(CatalogBody::of(
                &ScopedView::everything(&bundle),
                sutura_config::CatalogProse::Omitted,
            ))
            .expect("the catalog serializes");
            assert!(
                omitted["metrics"][0]["description"].is_null(),
                "a corpus description survived an omission: {omitted}"
            );
        }
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
