//! The tool surface: what a caller may ask, and what comes back.
//!
//! **This module is the governance boundary, and it is defined by what it does not contain.**
//! [`Query`] has no field for SQL, a table name, a filter expression or a list of row ids. An
//! uncertified question is therefore unrepresentable rather than refused, which is a stronger
//! property than it sounds: a refusal can be retried until something succeeds, and an absent field
//! cannot.
//!
//! Widening this is a governance change. `AGENTS.md` says which mechanism has to still hold.

use std::collections::BTreeSet;

use crate::calendar::TimeRange;
use crate::catalog::DimensionValue;
use crate::model::{Aggregate, DimensionName, Grain, MetricName, SourceName, TableName};
use crate::pinned::Provenance;
use crate::warehouse::RowSet;

/// The most dimensions one question may group by.
///
/// A bound for the same reason the time range is bounded: a group-by over every column is a table
/// scan with a plausible name, and the cost lands on a shared data system. Four covers the questions
/// a person asks and refuses the ones a loop generates.
pub const MAX_DIMENSIONS: usize = 4;

/// The longest span of history one question may ask about, in days.
///
/// **This is the bound the [`TimeRange`] newtype does not provide.** That type refuses an *absent*
/// endpoint; it accepts `[0001-01-01, 9999-12-31)`, which is over three and a half million days and, on
/// both execution paths, a full scan. `plan::MAX_ROWS` does not help: it bounds the rows *returned*
/// after the aggregate - refusing a result that exceeds them - so a question that scans everything and
/// groups it into one bucket is inside it. The span is what rows-read is a function of, so the span is
/// where the cap goes.
///
/// **3653 days is ten calendar years, counted at its longest.** Ten consecutive Gregorian years hold
/// 3652 or 3653 days depending on where the leap days fall, so this number is the one that lets
/// *any* ten-year window through rather than most of them. Ten years is chosen because it covers the
/// reporting a person actually does - a decade of annual figures, five years of quarters, three years
/// of months - and the longest range anywhere in this repository's example corpus, in its questions
/// and in its anchors alike, is 181 days - so nothing authored today is anywhere near it.
///
/// It also stays under `plan::MAX_ROWS`, and that is not a coincidence worth losing: at `day` grain
/// the time axis of a permitted question is at most 3653 buckets, so the row cap can only ever be
/// reached by dimension cardinality and never by the range alone. Raising this past the row cap would
/// make [`RefusalReason::ResultTooLarge`] the normal outcome of a wide range - a refusal nobody could
/// act on, because narrowing the range would not be what got them there.
///
/// A *span*, not a bucket count, and the difference matters. A bucket count would let `year` grain
/// through with a thousand years of scanning for a thousand rows, which is precisely the request this
/// exists to refuse; the span bounds the scan at every grain and bounds the buckets as a consequence.
///
/// **What it does not bound, said plainly rather than left for someone to discover.** It bounds ONE
/// question: three permitted ten-year questions cover thirty years, and nothing here correlates two
/// requests, because a per-caller budget needs a clock, a subject and somewhere to keep a counter and
/// this crate has none of the three. And inside a permitted span the *groups* are still the span times
/// the cardinality of up to [`MAX_DIMENSIONS`] dimensions - a dimension declared without a value list
/// has whatever cardinality the column has - so `plan::MAX_ROWS` refuses that result rather than
/// bounding the work that produced it: the groups are built, and then the answer is declined. A
/// refusal is not a budget. A day count is also only a proxy for rows: ten years of a small table and
/// ten years of a large one are the same number here. A real budget is expressed in rows or bytes
/// scanned, which needs something from the data system that no port asks for yet.
pub const MAX_RANGE_DAYS: i32 = 3653;

/// One equality filter: a dimension, and a value the pinned bundle declares.
///
/// The value is a [`DimensionValue`] here and a bind parameter by the time it reaches a statement. It
/// is checked against the metric's allowlist first, so the parameterisation is the second line of
/// defence rather than the only one.
///
/// # Why a caller's value is parsed by the type a catalog author's value is parsed by
///
/// It was a `String`, and the review that gave `DimensionValue` to the catalog side asked whether the
/// request side wanted it too. It does, for four reasons, and the last one is the decisive one:
///
/// * **It refuses nothing a request could have been answered.** The two are compared for equality
///   against the metric's allowlist, and every entry in that allowlist is a `DimensionValue`. Text
///   that cannot be one cannot be in there, so parsing here turns a `DimensionValueNotAllowed`
///   refusal into a `400` naming the field and loses no answerable question.
/// * **The precedent is already here and is older than this type.** A caller's `metric` and
///   `dimension` arrive as text and are parsed by [`MetricName`] and [`DimensionName`] - the same
///   types the catalog loader uses, at the same boundary, by the same constructor. A value being the
///   one field held to a laxer rule was the asymmetry, not the fix.
/// * **It bounds what a request may carry before anything allocates it.** A ten-megabyte filter value
///   used to be compared against the allowlist and refused, having been read, cloned into
///   [`Query::literals`] and rendered into whatever an audit sink keeps.
/// * **A second character rule is a rule nothing compares against the first.** [`crate::text`] exists
///   because one such rule was written down twice and the copies drifted. A request-side value type
///   with its own idea of what a value may hold would be that mistake, deliberately, in a place where
///   one side of the comparison is content and the other is a caller.
///
/// **What does NOT follow is that a refusal may name the text.** `sutura_http::wire` parses the value
/// and reports `filters[i].value` without the parse error underneath it, because
/// [`InvalidDimensionValue`](crate::catalog::InvalidDimensionValue) carries the offending input and
/// [`RefusalReason`]'s own rule is that caller-supplied text is never reflected into a message that
/// reaches a log, a UI and an agent's context.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    dimension: DimensionName,
    value: DimensionValue,
}

impl Filter {
    pub const fn new(dimension: DimensionName, value: DimensionValue) -> Self {
        Self { dimension, value }
    }

    #[inline]
    pub const fn dimension(&self) -> &DimensionName {
        &self.dimension
    }

    #[inline]
    pub const fn value(&self) -> &DimensionValue {
        &self.value
    }
}

/// A modelled question.
///
/// `deny_unknown_fields` is load-bearing rather than strict-for-its-own-sake. Without it a question
/// carrying `sql:` or `table:` deserializes cleanly with the extra field dropped on the floor, and a
/// caller who believes they sent SQL gets an answer to a different question. With it, the attempt is
/// an error naming the field.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    metric: MetricName,
    grain: Grain,
    range: TimeRange,
    #[serde(default)]
    dimensions: Vec<DimensionName>,
    #[serde(default)]
    filters: Vec<Filter>,
}

impl Query {
    pub const fn new(
        metric: MetricName,
        grain: Grain,
        range: TimeRange,
        dimensions: Vec<DimensionName>,
        filters: Vec<Filter>,
    ) -> Self {
        Self {
            metric,
            grain,
            range,
            dimensions,
            filters,
        }
    }

    #[inline]
    pub const fn metric(&self) -> &MetricName {
        &self.metric
    }

    #[inline]
    pub const fn grain(&self) -> Grain {
        self.grain
    }

    #[inline]
    pub const fn range(&self) -> TimeRange {
        self.range
    }

    #[inline]
    pub fn dimensions(&self) -> &[DimensionName] {
        &self.dimensions
    }

    #[inline]
    pub fn filters(&self) -> &[Filter] {
        &self.filters
    }

    /// The literal text this question carries, for the assertion that none of it reaches the SQL.
    ///
    /// It exists so the no-injection golden can be written as "no value from the question appears in
    /// the statement" rather than as a list of places to look, which is the form that goes stale the
    /// first time a field is added.
    pub fn literals(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::from([self.range.start().to_iso(), self.range.end().to_iso()]);
        out.extend(self.filters.iter().map(|f| String::from(f.value().as_str())));
        out
    }
}

/// Why a question was not answered.
///
/// Typed rather than prose, because the variant is the contract and the message is not. Every
/// variant has a test that provokes it: a refusal nobody has seen happen is a refusal nobody knows
/// works.
///
/// **There is no `TimeRangeUnbounded` variant, deliberately.** [`TimeRange`] has no unbounded form,
/// so such a refusal could never be provoked, and a variant with no test that can reach it looks
/// like coverage while being dead code. The type does that job instead.
///
/// [`TimeRangeTooLong`](RefusalReason::TimeRangeTooLong) is the variant that exists for the half the
/// type does *not* do, and the pair is worth reading together: an absent bound is unrepresentable, a
/// bound that is present and enormous is refused. The second has to be a refusal rather than a parse
/// error because the same [`TimeRange`] is also a catalog author's anchor range, and a maximum on the
/// type would govern authorship in order to govern requests.
///
/// Note what these variants do *not* carry: a rejected filter value is never echoed back.
/// `DimensionValueNotAllowed` names the dimension and stops there. Reflecting caller-supplied text
/// into a message that reaches a log, a UI and an agent's context is how a rejected value becomes
/// somebody else's input.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum RefusalReason {
    /// No metric of that name is in the pinned bundle.
    MetricUnknown { metric: MetricName },
    /// The metric exists and does not declare that grain. Not a narrower question: a grain the
    /// author did not render is a number nobody certified.
    GrainNotSupported { metric: MetricName, grain: Grain },
    /// The metric does not declare that dimension. A dimension a metric did not declare is a name
    /// that does not resolve, not a filter to apply anyway.
    DimensionNotPermitted { metric: MetricName, dimension: DimensionName },
    /// The dimension exists but declares no value allowlist, so it can be grouped by and not
    /// filtered.
    DimensionNotFilterable { metric: MetricName, dimension: DimensionName },
    /// The dimension is filterable and the value is not one the bundle declares.
    DimensionValueNotAllowed { metric: MetricName, dimension: DimensionName },
    /// The same dimension appears twice in one question. Refused rather than deduplicated: a
    /// caller who sent it twice believes something we do not.
    DuplicateDimension { dimension: DimensionName },
    /// More group-by keys than [`MAX_DIMENSIONS`].
    TooManyDimensions { requested: usize, limit: usize },
    /// The result was too much data to certify, and [`ResultBound`] says which bound said so.
    ///
    /// **The refusal that replaced a silent truncation, and it was a wrong-number bug.** The cap
    /// used to be the `LIMIT` on the statement and nothing compared the rows that came back against
    /// it, so a question wide enough to exceed it was answered with the first `MAX_ROWS` groups by
    /// group key - with provenance attached and no indication it was partial. Summing them gives a
    /// wrong number under a certified name, and nothing downstream can tell.
    ///
    /// A governance outcome rather than an error, for the same reason
    /// [`TimeRangeTooLong`](RefusalReason::TimeRangeTooLong) is: the question is well formed, the
    /// metric permits it, and the answer is still no. Refused rather than narrowed, because a total
    /// over part of the groups is not a smaller answer to the question asked - it is a different
    /// number wearing the same name.
    ///
    /// **One variant for two bounds, and the payload is what keeps that from being a lie.** A row cap
    /// this deployment configured and a data system declining to hand a result back in one piece are
    /// the same answer to a caller - *too much data, ask a narrower question* - so they share a
    /// variant, a code and a status rather than teaching an authorization server, a dashboard and an
    /// agent a second vocabulary for one remedy. What they do not share is a number, which is why the
    /// field is [`ResultBound`] rather than a `limit` that would have to be filled in with something
    /// for the case that has none.
    ResultTooLarge { bound: ResultBound },
    /// A span of history longer than [`MAX_RANGE_DAYS`].
    ///
    /// The availability boundary, and a governance outcome rather than a malformed question: the
    /// range parsed, both endpoints are real dates, and the answer is still no. Refused rather than
    /// silently narrowed to the last permitted day, because an answer about a different period than
    /// the one asked about is a wrong number nothing downstream can detect.
    ///
    /// Carries the two day counts and nothing from the caller's text, which is what makes it safe to
    /// log: a day count is derived from parsed dates, so there is no caller-controlled string to
    /// reflect into a message that reaches a log, a UI and an agent's context.
    TimeRangeTooLong { days: i32, limit: i32 },
    /// The plan would need to read from more than the deployment serves.
    ///
    /// Refused rather than run in parts, because each data system is a separate identity to satisfy,
    /// and a plan that runs partly as somebody else is the failure this design exists to prevent.
    ///
    /// **Exactly two is served** - a question that spans two sources is split into two legs and
    /// combined above them (the splitter reaches `sutura_semantic::Plan::Federated`, and `answer`
    /// executes it where the registered adapter can run a leg and otherwise refuses it as
    /// [`FederationNotExecutable`](RefusalReason::FederationNotExecutable)). So this is the bound on
    /// an unbounded fan-out: the "too many" is a named count against the limit the deployment serves.
    PlanSpansTooManySources { sources: usize, limit: usize },
    /// The question asked is served by two sources, but this build has no adapter that can execute
    /// a leg.
    ///
    /// **A fact about the BUILD, not about the question or the sources.** `Warehouses<W>` holds one
    /// adapter type, so this reads that type's `Warehouse::EXECUTES_LEGS` once: a build whose adapter
    /// declares it answers every two-source question it can plan, and a build whose adapter takes the
    /// default refuses all of them. The in-process engine declares it and is non-optional in both
    /// published binaries, so a release answers; an adapter that takes the default - `BigQuery`, or a
    /// fake - still arrives here.
    ///
    /// `answer` refuses here rather than surface the adapter's own refusal as a retryable 503: this
    /// is not a data system being down, and a caller must not retry it.
    ///
    /// **One producer, and that is `telekom/sutura#338`.** The compile stage used to raise this too,
    /// for a two-source plan it had built and could not then assemble - a defect in this workspace
    /// wearing a governance refusal's clothes. It mattered most while every published build refused
    /// here anyway, because the two were then the same answer to a caller; it still matters now that
    /// a release executes legs, because the question a caller has to be able to ask is *is this
    /// build unable to run a leg, or did sutura fail to assemble a plan it had already decided on*.
    /// That failure leaves as `sutura_semantic::CompileFailure` now, so this variant means the
    /// capability and nothing else.
    FederationNotExecutable,
    /// The question's remote dimensions join the metric's own through more than one relationship.
    ///
    /// The combiner links the two legs on a single column; two relationships on one remote data
    /// system would need two link columns, which the lookup leg does not carry. Refused rather than
    /// guess a link, and named as a link ambiguity rather than a source count: it is not that too
    /// many sources are involved.
    FederationLinkAmbiguous { source: SourceName },
    /// The question's measure cannot be decomposed into one leg per source.
    ///
    /// A measure federates only when its aggregate can be recomputed above the legs. A distinct count
    /// cannot: two exact distinct counts added together over-count every key the two legs share, and
    /// no re-aggregating function repairs it. The honest answer is to refuse rather than to pull the
    /// rows up through a combiner that would have to guess.
    ///
    /// Carries the metric and the aggregate that cannot descend, so a caller sees why.
    MeasureDoesNotFederate { metric: MetricName, aggregate: Aggregate },
    /// Two tables the plan would read answer to one identifier inside one statement.
    ///
    /// **A reproduced wrong-answer report, not a hypothetical.** A fact table at
    /// `analytics-prod.sales.orders` joined to a dimension table at `reference-data.crm.orders` gave a
    /// statement whose `ON` clause compared `orders.customer_id` with `orders.id` - one table with
    /// itself - because a column is qualified by the LAST part of a path and both paths end the same
    /// way. A real `DuckDB` answers that with `Binder Error: Ambiguous reference to table "orders"`; a
    /// target that binds it to one side instead returns a number under a certified metric name.
    ///
    /// **A refusal here rather than a load-time refusal, deliberately**, and the asymmetry with
    /// [`LabelShadowsTable`](crate::catalog::InconsistentDefinitions::LabelShadowsTable) is the
    /// reason: a colliding LABEL costs its author a rename, while a colliding TABLE is a physical
    /// name nobody here can change - and same-name tables across datasets are the normal shape of the
    /// estate that qualified paths exist for. So the metric stays authorable and only a question that
    /// actually puts both tables in one statement is declined; asking for a dimension that needs no
    /// join is still answered. `sutura_domain::plan::tables` holds the guard and the argument for why
    /// distinct explicit aliases are not the fix today.
    ///
    /// Carries the identifier the two collapsed to and neither of the two paths. The identifier is
    /// the thing a person can act on - it names the join to avoid - and a path carries the project
    /// and dataset a deployment reads, which is the operator's business rather than the asker's. The
    /// operator-facing detail is on the domain error the plan stage refused with.
    PlanTablesShareAnIdentifier { table: TableName },
    /// The plan named a data system this process did not open.
    ///
    /// **What raises it today is a name comparison, not an identity check**, and the doc comment
    /// used to claim otherwise. `sutura_app::answer` compares the plan's source against the
    /// adapter's own and refuses when they differ, which catches a bundle pointed at one data
    /// system being answered from another - a real hole, and the reason the check exists.
    ///
    /// It is deliberately the variant an identity failure will also use, because both are the same
    /// answer to a caller: this question cannot be answered here, and it will not be answered
    /// somewhere else instead. A refusal rather than a fallback - running as the service's own
    /// identity would turn "you may not see these rows" into "here are the rows" - but nothing in
    /// this workspace can yet run as any identity, so that half is a design target and not a
    /// control. `AGENTS.md` records which is which.
    SourceUnavailable { source: SourceName },
    /// An engine operator asked its memory pool for more than the deployment's working-set ceiling.
    ///
    /// **The variant that exists because process death was the alternative.** Shipped profiles
    /// compile `panic = "abort"`, so an unbounded allocation is not an error for the caller who
    /// asked - it is the process ending for every caller in flight. A bounded pool turns that into a
    /// reservation that fails, and this is what a failed reservation is on the way out.
    ///
    /// **A refusal rather than an error, and the distinction is the whole point of the variant.**
    /// Exhaustion used to reach a caller as `503 unavailable`, which is what a data system that is
    /// down looks like - so a caller was told to retry against a bound that will fire again at the
    /// same place. It is a governance outcome: the question is well formed, the metric permits it,
    /// and this deployment will not spend more than a configured number of bytes certifying it.
    ///
    /// Carries the ceiling and not what was asked for. The ceiling is a configured number, so it is
    /// the same for every caller and safe in a log; the *demand* is a measurement of the shape of
    /// somebody's data, and reporting it would tell a caller how much of the pool their question
    /// needed - a number arrived at by asking rather than by being permitted to know it.
    ///
    /// **What this does NOT cover, stated with the claim.** The pool counts what the engine's own
    /// operators reserve - a hash-join build side, aggregate state, a sort - and nothing else. Not
    /// what a driver buffers, not `collect()` materialising every batch, not the row set built while
    /// results are converted. So a question large enough to end the process on one of those paths
    /// still ends it, and this refusal is not the control that reaches them.
    ResourcesExhausted { ceiling_bytes: u64 },
    /// The asking subject has no credential at that data system.
    ///
    /// **Understood, and refused.** Asking differently does not help: what is missing is a grant at
    /// the data system, or a different subject. The plan is fine, the metric permits the question,
    /// and this deployment will not answer it as somebody else - which is the whole of what the
    /// credential port bought, because the alternative was a leg that ran as the process and came
    /// back with rows the asker may not see, under a certified metric name and valid provenance.
    ///
    /// **It is the one refusal `docs/adr/0008` adds, and the record deletes the other one it
    /// proposed.** A `SourceCannotImpersonate` was on that list at `409`, and part 6 walks every
    /// configuration that was supposed to reach it: each turns out to be a boot refusal, an `Err` for
    /// a wiring defect between the broker and the source declaration, this variant, or the decided
    /// permitted behaviour - a shared source in a multi-user deployment answers and records the
    /// posture it ran under. A variant no test can provoke is one this enum refuses to carry.
    ///
    /// **It amends `docs/adr/0005`**, which says the `403`s "are not a statement about a credential"
    /// because at the time no token widened anything. This one is, so a transport's detail for it must
    /// not send a caller looking for a better deployment token: the deployment's own credential is
    /// not what is missing.
    ///
    /// Carries the source and nothing about the credential. Which grant a subject lacks is the data
    /// system's to say, and guessing it here would be this deployment holding a second opinion about
    /// somebody else's authorization.
    CredentialUnavailable { source: SourceName },
    /// The legs of one answer would not all decide identity the same way.
    ///
    /// **Not a source count, and that distinction is the whole variant.**
    /// [`PlanSpansTooManySources`](RefusalReason::PlanSpansTooManySources) bounds a fan-out and says
    /// *sources*; this says what a combined number would be made of. Two sources under one posture
    /// are answered - that is the shape that ships - and two sources deciding identity two different
    /// ways are refused, because adding rows one identity was permitted to see to rows another
    /// identity was permitted to see produces a total no identity is entitled to, under a certified
    /// metric name and with valid provenance attached.
    ///
    /// Refused rather than disclosed, and *disclosed* is not the third option it reads as: an answer
    /// carries `executed_as` and `rows` in one body on both transports, with no streaming and no
    /// second message, so the only outcome that reaches a caller before the rows is a refusal. The
    /// per-leg record still ships and is still worth having - it documents a disclosure that
    /// happened, which is a different job from preventing one.
    ///
    /// **Carries the posture LABELS and never a `SourcePosture`.** That type's shared variant holds
    /// the operator's own acknowledgement text and both are `Serialize`, so a value here would
    /// publish operator prose to every caller, log and agent context - the same rule
    /// [`PlanTablesShareAnIdentifier`](RefusalReason::PlanTablesShareAnIdentifier) follows when it
    /// carries the identifier and neither path. The labels come from a closed set of two.
    ///
    /// **What it cannot decide, stated with the claim.** *Same posture* is decidable and *same
    /// asker* is not: nothing in this workspace names WHICH shared identity a source is read as, so
    /// two `shared-service-user` legs may be two different deployment-held identities and this
    /// passes them.
    LegsDecideIdentityDifferently { postures: BTreeSet<&'static str> },
}

/// Which bound a result was too large for.
///
/// **The vocabulary exists so one refusal can be honest about two causes.** The answer a caller gets
/// is one sentence - *too much data, ask a narrower question* - and
/// [`RefusalReason::ResultTooLarge`] is that one answer. This is what the deployment knows about why,
/// and the two arms differ in who measured it: the row cap is a number an operator configured here,
/// and the volume bound belongs to the data system and is not one this process was told.
///
/// **Closed, and read by exhaustive matches with no wildcard arm in both transports.** A third bound
/// is a compile error in each of them rather than a case one renders as another - which is what stops
/// a bound with no number being described using somebody else's number. The agent-facing prompt is
/// deliberately NOT one of those matches: `sutura_app::prompt::guide_for` keys on the
/// [`RefusalReason`] variant and never the payload, because what it must tell an agent - *too much
/// data, ask a narrower question* - is the same for both bounds and a guide per bound would give an
/// agent two paragraphs saying one thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ResultBound {
    /// The plan's row cap, in rows.
    ///
    /// Carries the limit and **not** how many rows there would have been, because nobody knows: the
    /// plan asks for one row more than the cap and stops there, so what is known is "more than
    /// this". That is the difference from
    /// [`TooManyDimensions`](RefusalReason::TooManyDimensions), which can name what was requested
    /// because the caller sent it.
    Rows { limit: u32 },
    /// The data system would not hand this result back in one piece.
    ///
    /// Raised through `Warehouse::result_did_not_fit`, a predicate for the reason that port method's
    /// own documentation gives. It is *not* the row cap: the statement carries `MAX_ROWS + 1` as its
    /// `LIMIT`, so a result reaching this arm was inside the cap and was still more data than the
    /// data system would deliver at once - a wide result rather than a tall one.
    ///
    /// **Carries no number, and that is a decision rather than a field somebody forgot.** The bound
    /// belongs to the data system and is not stated to a client: the endpoint this arm was built for
    /// caps a reply by size and reports neither that cap nor the reply's size, so the only figures
    /// in scope are how much was scanned and how much would be billed - neither of which is the
    /// bound that fired. An `Option<u64>` here would make every reader decide what an absence
    /// permits, and a figure filled in from one of those would be a certified-looking number for a
    /// bound that is not the one that refused. A fabricated limit is worse than an absent one.
    ///
    /// **The limit, stated with the claim:** a caller is told to narrow the question and is not told
    /// by how much. That is the whole of what this deployment honestly knows.
    Volume,
}

/// What a tool call produced.
///
/// A refusal is a *variant of the result*, not an `Err`. A caller cannot mistake it for a transport
/// hiccup and retry until something works, which is what an error would invite.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum ToolOutcome {
    Answer { provenance: Provenance, rows: RowSet },
    Refusal { reason: RefusalReason },
}

impl ToolOutcome {
    #[inline]
    pub const fn is_refusal(&self) -> bool {
        matches!(*self, Self::Refusal { .. })
    }

    /// The refusal reason, if this is one. Convenience for tests and for an audit sink.
    #[inline]
    pub const fn refusal(&self) -> Option<&RefusalReason> {
        match *self {
            Self::Refusal { ref reason } => Some(reason),
            Self::Answer { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Filter, Query, RefusalReason, ToolOutcome};
    use crate::calendar::{Date, TimeRange};
    use crate::catalog::DimensionValue;
    use crate::model::{DimensionName, Grain, MetricName};

    fn june() -> TimeRange {
        TimeRange::new(
            Date::parse("2026-06-01").expect("a test date is a date"),
            Date::parse("2026-07-01").expect("a test date is a date"),
        )
        .expect("June is a range")
    }

    fn query_with_filter(value: &str) -> Query {
        Query::new(
            MetricName::parse("revenue").expect("a test metric is a metric"),
            Grain::Month,
            june(),
            vec![DimensionName::parse("region").expect("a test dimension is a dimension")],
            vec![Filter::new(
                DimensionName::parse("region").expect("a test dimension is a dimension"),
                DimensionValue::parse(value).expect("a test value is a value"),
            )],
        )
    }

    #[test]
    fn a_question_reports_every_literal_it_carries() {
        // The no-injection golden is written against this, so a field added to `Query` without
        // being listed here would silently stop being checked. That is why it is a method on the
        // type rather than a list in the test.
        let literals = query_with_filter("north").literals();
        assert!(literals.contains("2026-06-01"), "{literals:?}");
        assert!(literals.contains("2026-07-01"), "{literals:?}");
        assert!(literals.contains("north"), "{literals:?}");
        assert_eq!(literals.len(), 3);
    }

    // The two governance properties of this type that need a real format parser to provoke -
    // `deny_unknown_fields` refusing a `sql:` field, and a range with no `end` failing to
    // deserialize at all - are asserted in `sutura-catalog-local`, against the YAML. They are
    // asserted there rather than here because that is the format a catalog is actually written in,
    // so the assertion covers the read path a typo arrives through; asserting them over a second
    // format would restate serde rather than the catalog. Not for want of a parser here:
    // `serde_json` is on `ALLOWED_IN_DOMAIN` in `xtask/src/boundaries.rs`, because the definition
    // digest is taken over the serialized form and has to be computed by code the domain trusts.

    #[test]
    fn a_value_a_catalog_could_not_declare_never_becomes_a_filter() {
        // The request side is held to the same character rule and the same length as the catalog
        // side, so text that could not be in an allowlist never reaches the comparison against one.
        // A caller sending either of these gets a `400` naming `filters[0].value` - raised by
        // `sutura_http::wire` WITHOUT the parse error underneath it, because that error carries the
        // caller's own text and this module's rule is that nothing reflects it back.
        drop(DimensionValue::parse("nor\u{200B}th").unwrap_err());
        drop(DimensionValue::parse("x".repeat(10_000)).unwrap_err());
        // And the value that would have been answered still is.
        assert_eq!(query_with_filter("north").filters()[0].value().as_str(), "north");
    }

    #[test]
    fn a_refusal_is_a_result_and_not_an_error() {
        // The invariant in one assertion: a refusal is reachable by matching on the outcome, so a
        // caller cannot treat it as a transport failure and retry until it succeeds.
        let outcome = ToolOutcome::Refusal {
            reason: RefusalReason::MetricUnknown {
                metric: MetricName::parse("clv").expect("a test metric is a metric"),
            },
        };
        assert!(outcome.is_refusal());
        assert!(matches!(outcome.refusal(), Some(&RefusalReason::MetricUnknown { .. })));
    }

    #[test]
    fn a_mixed_posture_refusal_carries_the_labels_and_no_acknowledgement_text() {
        // **The disclosure rule for OPERATOR text, which this enum's own note states for CALLER
        // text.** `SourcePosture::SharedServiceUser` carries a `SharedIdentityDeclared` ->
        // `AcknowledgementReason`, and both derive `Serialize` - so a posture VALUE in this variant
        // would publish the sentence an operator wrote on a source's entry to every caller, every
        // log and every agent's context. The variant carries labels off a closed set of two instead.
        //
        // Asserted on the serialized body as well as on `Debug`, because the body is what a
        // transport hands out and `Debug` is what reaches a log by accident.
        let reason = RefusalReason::LegsDecideIdentityDifferently {
            postures: crate::source::SourcePosture::NAMES.iter().copied().collect(),
        };
        let serialized = serde_json::to_string(&reason).expect("a refusal serializes");
        for rendered in [format!("{reason:?}"), serialized] {
            assert!(rendered.contains("shared-service-user"), "{rendered}");
            assert!(rendered.contains("impersonation-at-source"), "{rendered}");
            // No route to operator prose: the type the labels came from has none in it.
            assert!(!rendered.contains("acknowledg"), "{rendered}");
            assert!(!rendered.contains("declared"), "{rendered}");
        }
    }

    #[test]
    fn a_rejected_filter_value_is_not_echoed_back() {
        // Deliberate: a refusal message reaches a log, a UI and an agent's context. Reflecting the
        // caller's text into all three turns a rejected value into somebody else's input, so the
        // variant names the dimension and stops.
        let rejected = query_with_filter("north");
        let reason = RefusalReason::DimensionValueNotAllowed {
            metric: rejected.metric().clone(),
            dimension: DimensionName::parse("region").expect("a test dimension is a dimension"),
        };
        // `Debug` is the rendering that reaches a log by accident, so it is the one to assert on.
        let rendered = format!("{reason:?}");
        assert!(!rendered.contains("north"), "{rendered}");
        assert!(rendered.contains("region"), "{rendered}");
    }
}
