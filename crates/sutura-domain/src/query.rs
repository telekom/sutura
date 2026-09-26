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
use crate::model::{Aggregate, DimensionName, Grain, MetricName, ModelName, RelationshipName, SourceName, TableName};
use crate::nonempty::NonEmpty;
use crate::pinned::Provenance;
use crate::warehouse::RowSet;

pub mod filter;
pub mod limits;
pub mod top;

pub use filter::Filter;
pub use limits::{InvalidResponseByteLimit, ResponseByteLimit, ResultBound};
pub use top::{InvalidTopN, Top, TopBy, TopDirection, TopN};

/// One or more certified metric names a question asks about together - see
/// `github.com/telekom/sutura#968` for the shape and [`crate::nonempty::NonEmpty`] for why the
/// invariant is the type rather than a check.
pub type MetricNames = NonEmpty<MetricName>;

/// The most dimensions one question may group by.
///
/// A bound for the same reason the time range is bounded: a group-by over every column is a table
/// scan with a plausible name, and the cost lands on a shared data system. Four covers the questions
/// a person asks and refuses the ones a loop generates.
pub const MAX_DIMENSIONS: usize = 4;
/// The most metrics one question may name together.
///
/// A bound for the same reason the time range and the dimension count are bounded: each
/// additional metric is one more certified column the deployment computes for every row it
/// already reads, so the cost multiplies the groups rather than the scan. Eight covers the
/// questions a person asks - a handful of KPIs over the same break-down - and refuses a loop
/// that enumerates a catalog. A set larger than this is the same refusal a caller narrows by
/// asking about fewer metrics.
pub const MAX_METRICS: usize = 8;

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

// `Filter` and its non-empty value set moved to `filter` - see that module for why a caller's
// value is parsed by the type a catalog author's value is parsed by, and for what a rejected
// value's refusal does not carry. A caller's value is checked against the metric's allowlist
// first, so the parameterisation `crate::warehouse::ParamValue` performs is the second line of
// defence rather than the only one.

/// A modelled question.
///
/// `deny_unknown_fields` is load-bearing rather than strict-for-its-own-sake. Without it a question
/// carrying `sql:` or `table:` deserializes cleanly with the extra field dropped on the floor, and a
/// caller who believes they sent SQL gets an answer to a different question. With it, the attempt is
/// an error naming the field.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    metrics: MetricNames,
    grain: Grain,
    range: TimeRange,
    #[serde(default)]
    dimensions: Vec<DimensionName>,
    #[serde(default)]
    filters: Vec<Filter>,
    /// An order and a row limit the caller chose, bounding a wide group-by instead of asking for
    /// every group. `github.com/telekom/sutura#777`.
    #[serde(default)]
    top: Option<Top>,
}

impl Query {
    pub const fn new(
        metrics: MetricNames,
        grain: Grain,
        range: TimeRange,
        dimensions: Vec<DimensionName>,
        filters: Vec<Filter>,
    ) -> Self {
        Self {
            metrics,
            grain,
            range,
            dimensions,
            filters,
            top: None,
        }
    }

    /// One metric, the shape every question asked before `github.com/telekom/sutura#968`. A
    /// convenience over [`Self::new`] for the overwhelmingly common case, so a caller asking about
    /// one metric writes one name rather than building a one-element [`MetricNames`].
    #[inline]
    #[must_use]
    pub const fn single(
        metric: MetricName,
        grain: Grain,
        range: TimeRange,
        dimensions: Vec<DimensionName>,
        filters: Vec<Filter>,
    ) -> Self {
        Self::new(MetricNames::one(metric), grain, range, dimensions, filters)
    }

    /// Attaches a `top` clause. A builder rather than a sixth constructor argument, so every
    /// existing caller of [`Self::new`] - a question without `top` - keeps its argument list; a
    /// question built this way is byte-for-byte one built without it, minus this one field.
    #[inline]
    #[must_use]
    pub const fn with_top(mut self, top: Top) -> Self {
        self.top = Some(top);
        self
    }

    /// Every metric this question asks about, in the order the caller listed them. At least one:
    /// see [`MetricNames`].
    #[inline]
    pub const fn metrics(&self) -> &MetricNames {
        &self.metrics
    }

    /// The first metric named - the whole of [`Self::metrics`] for a single-metric question, and
    /// what a caller who has not yet widened for multiple metrics reads.
    #[inline]
    pub const fn metric(&self) -> &MetricName {
        self.metrics.first()
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

    #[inline]
    pub const fn top(&self) -> Option<Top> {
        self.top
    }

    /// The literal text this question carries, for the assertion that none of it reaches the SQL.
    ///
    /// It exists so the no-injection golden can be written as "no value from the question appears in
    /// the statement" rather than as a list of places to look, which is the form that goes stale the
    /// first time a field is added.
    pub fn literals(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::from([self.range.start().to_iso(), self.range.end().to_iso()]);
        out.extend(
            self.filters
                .iter()
                .flat_map(Filter::values)
                .map(|value| String::from(value.as_str())),
        );
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
    /// A question named more than one metric, and two of them do not declare the same model or
    /// the same time column.
    ///
    /// **A mixed-model set is unrepresentable as one grouped statement.** Two metrics over two
    /// physical tables have no shared `FROM`, and two metrics on one table but two different time
    /// columns would group by an ambiguous bucket - `sutura_semantic::resolve` compares every
    /// metric named against the first, so the pair reported is always the first metric and the
    /// first one that disagreed with it, never a third-party guess at which is "wrong".
    MetricsSpanDifferentModels { first: MetricName, other: MetricName },
    /// More metrics than [`MAX_METRICS`] one question may name together.
    TooManyMetrics { requested: usize, limit: usize },
    /// The same metric named twice in one question. Refused rather than de-duplicated, the same way
    /// a duplicated dimension is: a caller who sent it twice believes something about the result
    /// that is not true.
    DuplicateMetricName { metric: MetricName },
    /// A question named more than one metric and also reached a remote dimension.
    ///
    /// **A multi-metric question never federates.** The combiner links exactly two legs on one
    /// column and produces one measure column; there is no shape here for several certified
    /// columns arriving through a join. Refused by name, naming every metric asked, rather than
    /// silently answered as though only the first metric had been named.
    MultiMetricFederationNotExecutable { metrics: Vec<MetricName> },
    /// A question named more than one metric and also asked for `top`.
    ///
    /// **`top` names no metric to rank by**, and the two rendering paths disagree about which
    /// measure column it means once there is more than one: this workspace's own wiring, not a
    /// caller-narrowable question. Refused by name, naming every metric asked, rather than ranking
    /// by whichever measure a given adapter happens to read.
    MultiMetricTopNotExecutable { metrics: Vec<MetricName> },
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
    /// **A fact about the two adapters this answer would run on, read per LEG.** `Warehouses<W>`
    /// holds one adapter TYPE, but that type can itself be a closed enum over every kind a build
    /// linked (`telekom/sutura#112`) - so this reads `Warehouse::executes_legs` on each leg's own
    /// INSTANCE rather than one constant for the whole build. A build linking only the in-process
    /// engine still answers every two-source question it can plan, because both legs share that
    /// one instance's declaration; a build whose registry mixes a leg-capable kind with one that
    /// takes the port's default now refuses only the leg naming the declining kind, never the whole
    /// build's worth of questions. An adapter that takes the default everywhere - `BigQuery`, or a
    /// fake - still arrives here for every question it is asked.
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
    /// The relationship crossing into the remote data system declares more than one join key.
    ///
    /// **Distinct from [`FederationLinkAmbiguous`](Self::FederationLinkAmbiguous), which names two
    /// relationships crossing at once.** This is one relationship, correctly declared - a compound
    /// key is exactly what `telekom/sutura#967` exists to allow inside one data system - but the
    /// combiner links two legs on a single column, and a compound key would need one per column,
    /// which the lookup leg's shape does not carry. Named for what is actually true rather than
    /// reused from the ambiguity case, so a caller is not told two relationships exist when one does.
    FederationLinkCompound {
        source: SourceName,
        relationship: RelationshipName,
    },
    /// The question's measure cannot be decomposed into one leg per source.
    ///
    /// A measure federates only when its aggregate can be recomputed above the legs. A distinct count
    /// cannot: two exact distinct counts added together over-count every key the two legs share, and
    /// no re-aggregating function repairs it. The honest answer is to refuse rather than to pull the
    /// rows up through a combiner that would have to guess.
    ///
    /// Carries the metric and the aggregate that cannot descend, so a caller sees why.
    MeasureDoesNotFederate { metric: MetricName, aggregate: Aggregate },
    /// The combiner could not compute the answer as asked, deterministically.
    ///
    /// **D19 + A4: this used to have no refusal at all.** A non-finite ratio and a link value
    /// mapping to more than one lookup row left as `ServiceError::Federated` and reached a transport
    /// as an HTTP `503` - "worth retrying", the status a data system that might come back
    /// produces. Neither is: the same plan against the same rows fails again, so retrying spends a
    /// caller's own budget on an answer that was never going to change.
    /// [`crate::plan::FederationCombiner::answer_not_well_formed`] is the predicate that decides
    /// which of a combiner's own failures land here rather than staying a wiring-defect
    /// `ServiceError`.
    ///
    /// Carries the classification and no cell: see [`FederatedAnswerRefusal`](crate::plan::FederatedAnswerRefusal)'s
    /// own note on why a join key or a float value never reaches this far.
    FederatedAnswerNotWellFormed { federated: crate::plan::FederatedAnswerRefusal },
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
    /// **D7: carries the identifier and neither of the two paths, and no transport renders the
    /// identifier either.** It used to be argued that the bare identifier was safe to show because
    /// it names the join to avoid; it still reaches an agent's own context exactly as a schema name
    /// in the generated prompt would, which is the rule `sutura_app::prompt` states for that
    /// surface, so every transport's message is generic and the identifier stays a typed field for
    /// logs and tests.
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
    /// The data system refused the executed statement because the identity it ran it as may not ask
    /// it.
    ///
    /// **The refusal a data system hands back at the identity/authorization level, as opposed to the
    /// two bounds [`ResourcesExhausted`](RefusalReason::ResourcesExhausted) and
    /// [`ResultTooLarge`](RefusalReason::ResultTooLarge) classify.** Those are the domain's own
    /// numbers - a reservation this deployment refused, a page it caps. This one is the DATA
    /// SYSTEM saying no about WHO asked: the role it runs the question under lacks the permission,
    /// a statement's `SELECT` names a table or column that identity may not read, row-level security
    /// denies it. It is permanent in the same sense [`CredentialUnavailable`](RefusalReason::CredentialUnavailable)
    /// is - the same question as the same identity is refused again - but it is not a missing
    /// credentials decision: the identity that ran the query WAS presented, and the source refuses
    /// it.
    ///
    /// **A refusal rather than an error, and the distinction is the whole variant.** It used to
    /// arrive as [`crate::warehouse::Warehouse::execute`]'s `Err` and leave the transport as the
    /// `503` a dead data system produces - so a caller was told to retry an authorization decision
    /// that will refuse again at the same place. It is an answer, and the answer does not change by
    /// asking again.
    ///
    /// **It is the query-time sibling of the boot-time
    /// [`Warehouse::preflight_was_refused`](crate::warehouse::Warehouse::preflight_was_refused)
    /// split.** That predicate tells a pre-flight's *could not verify* apart from *this identity may
    /// not ask*; this variant is the same split when the asking happens in `execute`. An adapter
    /// answers [`Warehouse::source_refused`](crate::warehouse::Warehouse::source_refused), and this
    /// is what the domain names the `true` half.
    ///
    /// Carries the source and nothing about which permission or which identity: both are the data
    /// system's to say, and echoing them would publish a foreign authorization decision into a log,
    /// a UI and an agent's context.
    SourceRefused { source: SourceName },
    /// This answer ran out of the time it was given, at the data system or before it was ever
    /// asked.
    ///
    /// **A refusal rather than a failure, and `docs/adr/0029` argues both directions once rather
    /// than asserting the choice.** For a failure: time is load-dependent in a way memory is not, so
    /// *repeating this without modification will fail the same way* is likely here rather than
    /// certain. For a refusal, which wins: the deployment decided the bound and the data system
    /// enforced it, so something WAS judged; the failure this replaces used to leave as a retryable
    /// `503`, which invites an automatic retry that spends the whole budget at the data system
    /// again, and on a networked adapter billed for the call, bills again; and an `Err` writes no
    /// audit record, so a question a configured bound stopped would leave no outcome anywhere.
    ///
    /// Mapped from [`crate::warehouse::Warehouse::deadline_exceeded`] after
    /// [`ResourcesExhausted`](RefusalReason::ResourcesExhausted) and
    /// [`ResultTooLarge`](RefusalReason::ResultTooLarge), before
    /// [`SourceRefused`](RefusalReason::SourceRefused); also raised directly, with no adapter
    /// involved at all, when the budget is already spent before `dry_run`, before `execute`, or
    /// before the next leg of a federated answer starts.
    ///
    /// Carries the configured budget in seconds - the same reason
    /// [`ResourcesExhausted`](RefusalReason::ResourcesExhausted) carries its ceiling: a number an
    /// operator configured, the same for every caller, safe in a log. Not how long the question
    /// would have taken, which nobody knows, and not which leg spent it, which would tell a caller
    /// how a deployment's sources compare.
    DeadlineExceeded { budget_seconds: u64 },
    /// The asking subject has spent more than this replica's configured byte ceiling inside the
    /// current window.
    ///
    /// **The first refusal in this enum that self-heals, and `docs/adr/0030` is the record.** Every
    /// other 4xx row here is permanent in the sense that matters to a client: the same question
    /// refused now is refused again on an identical retry, because nothing about the refusal
    /// changes with time. This one is not - the same question asked again after the window this
    /// replica tracks rolls over is a different answer, because nothing about the question changed,
    /// only that time passed. That is what `429` states and what `422`
    /// ([`ResourcesExhausted`](RefusalReason::ResourcesExhausted), whose row says *narrowing helps
    /// and repeating does not* - here narrowing does not help and repeating does) and `403`
    /// ([`CredentialUnavailable`](RefusalReason::CredentialUnavailable),
    /// [`SourceRefused`](RefusalReason::SourceRefused) - permanent grants) do not.
    ///
    /// **It amends `docs/adr/0005`**, whose Context section says "the two \[statuses retried by
    /// convention, `429` and `408`\] and no refusal maps to either" - false from this variant on.
    ///
    /// **Keyed on the subject `PrincipalChain::attribution()` names, never the whole chain** - an
    /// agent acting for a subject spends that subject's own budget, not one of its own, so a
    /// caller's own retrying tool can exhaust the allowance their own next direct question needed.
    /// The actor chain does not disappear: it travels on the audit record beside this refusal, which
    /// is always written under the full chain regardless of outcome, so *whose* retry spent the
    /// budget is answerable after the fact even though the counter did not key on it in advance.
    ///
    /// **The counter is per-replica, in-process, and this variant does not say otherwise.** A
    /// deployment with N replicas gives N times the configured ceiling before every replica has
    /// independently refused, and the counter resets on a restart in addition to its own window.
    /// `governance.per_replica_spend_ceiling` is the settings key, named to say so.
    ///
    /// **What is summed to trigger it, and what is not.** The estimate is
    /// [`crate::warehouse::PreFlight::Accepted`]'s own `estimated_bytes`, read after a dry run and
    /// before `execute`; a federated answer sums both legs' `Some` estimates and checks the total
    /// against the ceiling before either leg executes, so a two-source answer is refused
    /// all-or-nothing rather than after one leg has already spent. An adapter whose dry run answers
    /// `None` - every adapter but `BigQuery` today - counts nothing toward this ceiling, which is
    /// stated as "not counted" rather than implied as "free": the ceiling governs only the spend
    /// this deployment could price, not the spend that happened.
    ///
    /// Carries the seconds until this replica's window resets, the same reason
    /// [`DeadlineExceeded`](RefusalReason::DeadlineExceeded) carries its budget: a number this
    /// replica computed, the same meaning for every caller, safe in a log - and enough for a
    /// transport to answer `Retry-After` with a fact rather than a guess.
    BudgetExhausted { reset_after_seconds: u64 },
    /// A federated `top` was ranked over a combined set the row ceiling had already cut, so the
    /// ranking is over an arbitrary slice rather than over the dimension.
    ///
    /// **`github.com/telekom/sutura#777`'s case 2.** A federated `top` ranks above the combine,
    /// and the combined set, before that rank is applied, already hit [`crate::plan::RowCeiling`].
    /// The top ten of an arbitrary ten thousand wears the shape of a right answer and is not one,
    /// which is the failure this repository refuses everywhere else it can be reached.
    ///
    /// **Carries the ceiling that fired, so the message can name it and point at the deployment's
    /// operator - the one who can raise [`crate::plan::RowCeiling`], which is a configured value and
    /// not [`crate::plan::MAX_ROWS`] the compiled constant. Naming a compiled constant here would be
    /// advice nobody addressed could act on.**
    TopOverUncertifiedRows { ceiling: u32 },
    /// A ratio term names a model other than the metric's own, and this workspace does not yet
    /// build the second fact leg such a term needs.
    ///
    /// **`telekom/sutura#780`'s vocabulary, and the plan half of its first slice.** The catalog
    /// admits the definition - `Definitions::assemble` proves the named model is declared and the
    /// term's column exists on it - so a metric with a cross-model ratio loads and is addressable
    /// by name, PROVIDED it declares no anchor: an anchor is executed at boot, and one on such a
    /// metric reaches this refusal through `NotValidated::AnchorNotExecuted` instead, which takes
    /// the whole bundle down rather than only that metric - fails closed, and stated here rather
    /// than left for a reviewer to find by asking. Asking a plain question about it is refused
    /// rather than mis-planned against the metric's own table: the splitter has one plan shape per
    /// data system today, [`QueryPlan`](crate::plan::QueryPlan) and
    /// [`FederatedPlan`](crate::plan::FederatedPlan), and neither reads a second FACT model's rows,
    /// aggregated on its own and joined above - which is what a certified answer over two facts
    /// needs, per the issue's own decision record.
    ///
    /// **Distinct from [`MeasureDoesNotFederate`](Self::MeasureDoesNotFederate) and
    /// [`FederationNotExecutable`](Self::FederationNotExecutable) on purpose.** Neither reason
    /// applies here: the aggregate is additive (a `sum` or a `count_distinct` federates fine when
    /// the second leg is a lookup), and a build's adapter capability is not what is missing - the
    /// plan SHAPE for two aggregated fact legs does not exist yet, on any adapter. Reusing either
    /// variant would misreport why the question is refused.
    ///
    /// A [`RefusalReason`] rather than a wiring defect, for
    /// [`FederationNotExecutable`](Self::FederationNotExecutable)'s own reason: this variant
    /// means the capability and nothing else. It is NOT narrowable the way
    /// most of this enum's questions are - the cross-model term is part of the metric's
    /// definition, `Query` has no field that reaches it, and every caller-facing text this
    /// variant produces says so ("nothing you can change in the question"). What changes the
    /// answer is this workspace building the second fact leg, not a different question.
    CrossModelRatioNotExecutable { metric: MetricName, model: ModelName },
}

impl RefusalReason {
    /// The machine-readable `code` a client or an agent branches on, shared by every transport.
    ///
    /// **The one place this is decided.** The HTTP and agent surfaces used to spell their own
    /// tables and nothing compared them, so a code could drift until the two transports disagreed
    /// about what a refusal was. Both now read [`RefusalReason::code`] and neither writes its own
    /// list, so there is one spelling for the whole surface.
    ///
    /// Being exhaustive with no wildcard arm, a variant added here either gets its code in the same
    /// edit or does not compile. The derivation is fixed by
    /// `the_code_is_the_variant_name_in_snake_case`: each code is the `snake_case` spelling
    /// of the variant's own name, read off this type's own `Serialize` rather than a list typed
    /// beside it - so a hand-written code that drifted from the variant fails that test, and the two
    /// transports, both reading this one method, cannot drift from each other without first drifting
    /// from the variant.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::MetricUnknown { .. } => "metric_unknown",
            Self::MetricsSpanDifferentModels { .. } => "metrics_span_different_models",
            Self::TooManyMetrics { .. } => "too_many_metrics",
            Self::DuplicateMetricName { .. } => "duplicate_metric_name",
            Self::MultiMetricFederationNotExecutable { .. } => "multi_metric_federation_not_executable",
            Self::MultiMetricTopNotExecutable { .. } => "multi_metric_top_not_executable",
            Self::GrainNotSupported { .. } => "grain_not_supported",
            Self::DimensionNotPermitted { .. } => "dimension_not_permitted",
            Self::DimensionNotFilterable { .. } => "dimension_not_filterable",
            Self::DimensionValueNotAllowed { .. } => "dimension_value_not_allowed",
            Self::DuplicateDimension { .. } => "duplicate_dimension",
            Self::TooManyDimensions { .. } => "too_many_dimensions",
            Self::ResultTooLarge { .. } => "result_too_large",
            Self::TimeRangeTooLong { .. } => "time_range_too_long",
            Self::PlanSpansTooManySources { .. } => "plan_spans_too_many_sources",
            Self::FederationNotExecutable => "federation_not_executable",
            Self::FederationLinkAmbiguous { .. } => "federation_link_ambiguous",
            Self::FederationLinkCompound { .. } => "federation_link_compound",
            Self::MeasureDoesNotFederate { .. } => "measure_does_not_federate",
            Self::FederatedAnswerNotWellFormed { .. } => "federated_answer_not_well_formed",
            Self::PlanTablesShareAnIdentifier { .. } => "plan_tables_share_an_identifier",
            Self::SourceUnavailable { .. } => "source_unavailable",
            Self::ResourcesExhausted { .. } => "resources_exhausted",
            Self::CredentialUnavailable { .. } => "credential_unavailable",
            Self::SourceRefused { .. } => "source_refused",
            Self::DeadlineExceeded { .. } => "deadline_exceeded",
            Self::BudgetExhausted { .. } => "budget_exhausted",
            Self::TopOverUncertifiedRows { .. } => "top_over_uncertified_rows",
            Self::CrossModelRatioNotExecutable { .. } => "cross_model_ratio_not_executable",
        }
    }
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
    use super::{Filter, Query, RefusalReason, ResultBound, ToolOutcome};
    use crate::calendar::{Date, TimeRange};
    use crate::catalog::DimensionValue;
    use crate::model::{DimensionName, Grain, MetricName, RelationshipName};

    fn june() -> TimeRange {
        TimeRange::new(
            Date::parse("2026-06-01").expect("a test date is a date"),
            Date::parse("2026-07-01").expect("a test date is a date"),
        )
        .expect("June is a range")
    }

    fn query_with_filter(value: &str) -> Query {
        Query::single(
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
        let query = query_with_filter("north");
        let values = query.filters()[0].values();
        assert_eq!(
            values,
            vec![&DimensionValue::parse("north").expect("a test value is a value")]
        );
    }

    #[test]
    fn an_in_filter_s_values_all_reach_literals() {
        // `In`/`NotIn` widen the shape of a filter, not the source of a value - every value in the
        // set is still a literal the no-injection golden must be able to see.
        let query = Query::single(
            MetricName::parse("revenue").expect("a test metric is a metric"),
            Grain::Month,
            june(),
            Vec::new(),
            vec![Filter::in_set(
                DimensionName::parse("region").expect("a test dimension is a dimension"),
                crate::nonempty::NonEmpty::parse(vec![
                    DimensionValue::parse("north").expect("a test value is a value"),
                    DimensionValue::parse("south").expect("a test value is a value"),
                ])
                .expect("two values is a set"),
            )],
        );
        let literals = query.literals();
        assert!(literals.contains("north"), "{literals:?}");
        assert!(literals.contains("south"), "{literals:?}");
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

    /// Every variant, so the cross-transport contract is checked over the whole enum and not over
    /// whichever ones somebody remembered.
    fn every_reason() -> Vec<RefusalReason> {
        use crate::model::{Aggregate, ModelName, SourceName, TableName};
        vec![
            RefusalReason::MetricUnknown {
                metric: MetricName::parse("revenue").expect("a test metric"),
            },
            RefusalReason::MetricsSpanDifferentModels {
                first: MetricName::parse("revenue").expect("a test metric"),
                other: MetricName::parse("margin").expect("a test metric"),
            },
            RefusalReason::TooManyMetrics { requested: 9, limit: 8 },
            RefusalReason::DuplicateMetricName {
                metric: MetricName::parse("revenue").expect("a test metric"),
            },
            RefusalReason::MultiMetricFederationNotExecutable {
                metrics: vec![
                    MetricName::parse("revenue").expect("a test metric"),
                    MetricName::parse("margin").expect("a test metric"),
                ],
            },
            RefusalReason::MultiMetricTopNotExecutable {
                metrics: vec![
                    MetricName::parse("revenue").expect("a test metric"),
                    MetricName::parse("margin").expect("a test metric"),
                ],
            },
            RefusalReason::GrainNotSupported {
                metric: MetricName::parse("revenue").expect("a test metric"),
                grain: Grain::Week,
            },
            RefusalReason::DimensionNotPermitted {
                metric: MetricName::parse("revenue").expect("a test metric"),
                dimension: DimensionName::parse("region").expect("a test dimension"),
            },
            RefusalReason::DimensionNotFilterable {
                metric: MetricName::parse("revenue").expect("a test metric"),
                dimension: DimensionName::parse("region").expect("a test dimension"),
            },
            RefusalReason::DimensionValueNotAllowed {
                metric: MetricName::parse("revenue").expect("a test metric"),
                dimension: DimensionName::parse("region").expect("a test dimension"),
            },
            RefusalReason::DuplicateDimension {
                dimension: DimensionName::parse("region").expect("a test dimension"),
            },
            RefusalReason::TooManyDimensions { requested: 5, limit: 4 },
            RefusalReason::ResultTooLarge {
                bound: ResultBound::Rows { limit: 10_000 },
            },
            RefusalReason::TimeRangeTooLong { days: 9000, limit: 3653 },
            RefusalReason::PlanSpansTooManySources { sources: 3, limit: 2 },
            RefusalReason::FederationNotExecutable,
            RefusalReason::FederationLinkAmbiguous {
                source: SourceName::parse("warehouse").expect("a test source"),
            },
            RefusalReason::FederationLinkCompound {
                source: SourceName::parse("warehouse").expect("a test source"),
                relationship: RelationshipName::parse("usage_subscription").expect("a test relationship"),
            },
            RefusalReason::MeasureDoesNotFederate {
                metric: MetricName::parse("active_subscriptions").expect("a test metric"),
                aggregate: Aggregate::CountDistinct,
            },
            RefusalReason::FederatedAnswerNotWellFormed {
                federated: crate::plan::FederatedAnswerRefusal::AmbiguousLink,
            },
            RefusalReason::PlanTablesShareAnIdentifier {
                table: TableName::parse("orders").expect("a test table"),
            },
            RefusalReason::SourceUnavailable {
                source: SourceName::parse("local").expect("a test source"),
            },
            RefusalReason::SourceRefused {
                source: SourceName::parse("local").expect("a test source"),
            },
            RefusalReason::ResourcesExhausted {
                ceiling_bytes: 1024 * 1024 * 1024,
            },
            RefusalReason::CredentialUnavailable {
                source: SourceName::parse("warehouse").expect("a test source"),
            },
            RefusalReason::DeadlineExceeded { budget_seconds: 29 },
            RefusalReason::BudgetExhausted { reset_after_seconds: 41 },
            RefusalReason::TopOverUncertifiedRows { ceiling: 10_000 },
            RefusalReason::CrossModelRatioNotExecutable {
                metric: MetricName::parse("revenue_per_customer").expect("a test metric"),
                model: ModelName::parse("customers").expect("a test model"),
            },
        ]
    }

    /// The derivation that keeps the two transports' vocabularies equal without either being able
    /// to see the other - in the crate both read.
    ///
    /// The transport surfaces each read [`RefusalReason::code`], so a drift in that single method
    /// is a drift in BOTH transports at once, and this is the test that would see it: the code is
    /// held to the `snake_case` spelling of the variant's own name. The variant name is read out of
    /// this type's own `Serialize` - externally tagged, so the one key of the serialized object IS
    /// the variant name - rather than from a list typed here, which would be the same hand-written
    /// table the transports used to carry.
    #[test]
    fn the_code_is_the_variant_name_in_snake_case() {
        for reason in every_reason() {
            let value = serde_json::to_value(&reason).expect("a refusal serializes");
            let variant = match value {
                serde_json::Value::Object(map) => map.keys().next().cloned().expect("an externally tagged enum has one key"),
                serde_json::Value::String(name) => name,
                _ => panic!("a refusal serializes to an object or a unit string"),
            };
            assert_eq!(reason.code(), &snake_case(&variant), "{variant}");
        }
    }

    fn snake_case(name: &str) -> String {
        let mut out = String::with_capacity(name.len().saturating_add(4));
        for (index, character) in name.char_indices() {
            if character.is_ascii_uppercase() {
                if index != 0 {
                    out.push('_');
                }
                out.push(character.to_ascii_lowercase());
            } else {
                out.push(character);
            }
        }
        out
    }

    #[test]
    fn a_response_byte_limit_of_zero_is_refused() {
        use super::{InvalidResponseByteLimit, ResponseByteLimit};

        assert_eq!(ResponseByteLimit::parse(0).unwrap_err(), InvalidResponseByteLimit::Zero);
        assert_eq!(ResponseByteLimit::parse(1).expect("one byte is a limit").bytes(), 1);
    }

    #[test]
    fn the_default_response_byte_limit_is_eight_mebibytes() {
        use super::ResponseByteLimit;

        assert_eq!(ResponseByteLimit::DEFAULT.bytes(), 8 * 1024 * 1024);
    }
}
