//! The tool surface: what a caller may ask, and what comes back.
//!
//! **This module is the governance boundary, and it is defined by what it does not contain.**
//! [`Query`] has no field for SQL, a table name, a filter expression or a list of row ids. An
//! uncertified question is therefore unrepresentable rather than refused, which is a stronger
//! property than it sounds: a refusal can be retried until something succeeds, and an absent field
//! cannot.
//!
//! Widening this is a governance change. `AGENTS.md` says which mechanism has to still hold.

use crate::calendar::TimeRange;
use crate::model::{DimensionName, Grain, MetricName, ModelName, RelationshipName, SourceName, TableName};
use crate::nonempty::NonEmpty;
use crate::pinned::Provenance;
use crate::warehouse::RowSet;

#[cfg(feature = "test-fakes")]
mod exemplars;
pub mod filter;
pub mod limits;
mod methods;
pub mod top;

#[cfg(feature = "test-fakes")]
pub use exemplars::every_refusal;
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
/// The most lookup legs any deployment may configure for one metric's plan, and the most
/// [`LookupLegs`](crate::plan::LookupLegs) holds.
///
/// The configured bound is [`LookupLegCeiling`](crate::plan::LookupLegCeiling), three by default;
/// this is the one a plan type and the combiner's table names are sized to. Eight is a round number
/// stated as one, not a measured one: no plan over more than three lookup legs has been measured,
/// and the working-set ceiling, not this, bounds their bytes. Changing it amends `docs/adr/0007`.
pub const MAX_LOOKUP_LEGS: usize = 8;
/// The most metrics one question may name together.
///
/// A bound for the same reason the time range and the dimension count are bounded: each
/// additional metric is one more certified column the deployment computes for every row it
/// already reads, so the cost multiplies the groups rather than the scan. Eight covers the
/// questions a person asks - a handful of KPIs over the same break-down - and refuses a loop
/// that enumerates a catalog. A set larger than this is the same refusal a caller narrows by
/// asking about fewer metrics.
pub const MAX_METRICS: usize = 8;

/// The most filters one question may ask the catalog to resolve.
pub const MAX_FILTERS: usize = 16;

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
/// 3651, 3652 or 3653 days depending on where the leap days fall, so this number is the one that lets
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
    /// More metrics than [`MAX_METRICS`] one question may name together.
    TooManyMetrics { requested: usize, limit: usize },
    /// A question named several metrics, and a dimension it groups or filters by does not end on
    /// the same model and column for `first` and `other`.
    ///
    /// **A dimension only joins the metrics' answers when it ends on the same model and column for
    /// every metric.** Reading the first metric's column for all of them would label another
    /// metric's numbers with a grouping it never computed. The check is structural: two columns
    /// that mean the same thing but are two copies are refused too. Refused naming the dimension
    /// and the first pair that disagree, never a guess at which of them is wrong.
    DimensionNotConformed {
        dimension: DimensionName,
        first: MetricName,
        other: MetricName,
    },
    /// A question needs more legs than one answer may run, counting every leg of every metric.
    ///
    /// Each leg is held whole before the answers are merged, so the leg count bounds held memory
    /// the way [`TooManyMetrics`](RefusalReason::TooManyMetrics) bounds the statement. The "too
    /// many" is a named count against the deployment's [`LegCeiling`](crate::plan::LegCeiling).
    QuestionNeedsTooManyLegs { legs: usize, limit: usize },
    /// The same metric named twice in one question. Refused rather than de-duplicated, the same way
    /// a duplicated dimension is: a caller who sent it twice believes something about the result
    /// that is not true.
    DuplicateMetricName { metric: MetricName },
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
    /// More filters than [`MAX_FILTERS`], refused before any filter is parsed.
    TooManyFilters { requested: usize, limit: usize },
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
    /// The plan would read more distinct data systems than one answer may.
    ///
    /// Refused rather than run in parts, because each data system is a separate identity to satisfy,
    /// and a plan that runs partly as somebody else is the failure this design exists to prevent. The
    /// count is of distinct sources, the set the broker mints credentials for once per answer, so it
    /// bounds one metric and several metrics alike. A lookup leg that reads a source another leg
    /// already reads does not add to it. Narrowable: ask for fewer dimensions that live in other
    /// data systems, or for fewer metrics at a time. The "too many" is a named count against the
    /// deployment's [`SourceCeiling`](crate::plan::SourceCeiling).
    PlanSpansTooManySources { sources: usize, limit: usize },
    /// The plan would need more lookup legs than one answer may carry.
    ///
    /// Refused rather than run in parts, because each leg's data system is a separate identity to
    /// satisfy, and a plan that runs partly as somebody else is the failure this design exists to
    /// prevent. A lookup leg is one crossing relationship, so two dimensions that reach one remote
    /// model through two relationships count twice, and a chain that crosses twice spends two. The
    /// "too many" is a named count against the deployment's
    /// [`LookupLegCeiling`](crate::plan::LookupLegCeiling). Narrowable: ask for fewer remote
    /// dimensions.
    PlanNeedsTooManyLookupLegs { legs: usize, limit: usize },
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
    /// The two columns the crossing relationship joins are declared as kinds of value that do not
    /// join: different kinds, or a pair the relationship's declared comparison does not join.
    ///
    /// Decided from the catalog before either leg runs, and only when BOTH types are declared and
    /// BOTH are known: an integer against text can never match, so every row would miss. A type
    /// this does not know, or one a catalog leaves undeclared, defers to the combiner's own check
    /// over the legs' real schemas (`FederatedAnswerNotWellFormed`). The types are quoted and
    /// escaped, never raw: a source's dictionary supplied them. Not narrowable by the caller: the
    /// declared types are the catalog's.
    FederationLinkTypeMismatch {
        relationship: RelationshipName,
        fact_type: crate::plan::QuotedColumnType,
        lookup_type: crate::plan::QuotedColumnType,
    },
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
    /// The address the caller's identity provider gives them is outside the domains that
    /// `domain_delegation` source declares in `address_domains`, so the question is not run there as
    /// that address, nor as the deployment.
    ///
    /// Decided by the broker before any credential is minted or exchanged for any source. Carries the
    /// source and never the address, which names a person.
    AddressOutsideDomains { source: SourceName },
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
    /// **A refusal rather than an error, and the distinction is the whole variant**: not the `503` a
    /// dead data system produces, because the answer does not change by asking again.
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
    /// The request named a payer this source does not bill, so nothing ran.
    BillToNotAllowed { source: SourceName },
    /// No payer could be decided for this source's job - no mapped group, more than one project read, none named.
    NoBillingProject { source: SourceName },
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
    /// A cross-model ratio whose two fact models share no conformed calendar
    /// (`telekom/sutura#780`). The metric's terms name two fact models, and the metric declares no
    /// `shared_calendar` - so both facts cannot be bucketed through the same time dimension, and
    /// the combiner cannot join them on the link AND the bucket. The ratio is refused rather than
    /// bucketed on the first fact's time column, which would misalign the two facts.
    ///
    /// **Retires, and replaces, `CrossModelRatioNotExecutable`** - the reason a cross-model ratio
    /// used before this variant existed, which refused EVERY such ratio under one name regardless
    /// of what a catalog author could do about it. This one names the missing declaration
    /// specifically: a metric that DOES declare a reachable `shared_calendar` no longer reaches
    /// this variant at all (`docs/adr/0002`'s second amendment). The retired variant is not kept
    /// unreachable - `xtask/src/refusals/registry.rs` records the straight substitution, the same
    /// mechanism `TopNotFederated` → `TopOverUncertifiedRows` already used.
    ///
    /// Narrowable in principle: a catalog author who declares the shared calendar makes the ratio
    /// executable. Only a local catalog can declare one (its `shared_calendar:` key); no other
    /// catalog adapter reads the field yet. Not narrowable by the caller: the `shared_calendar` is
    /// part of the metric's definition, not of the question.
    CrossModelRatioWithoutSharedCalendar { metric: MetricName, model: ModelName },
    /// A cross-model ratio asked by no dimension both fact models link to (`telekom/sutura#780`).
    ///
    /// The two facts are aggregated in two statements and joined above on ONE link into a remote
    /// dimension's model, so the question groups and filters by that model's dimensions and nothing
    /// else, and the second fact `model` declares its own relationship into it on the column the
    /// first fact links through, truncated to the same grain or not at all. Narrowable: group by a
    /// dimension both facts link to.
    CrossModelRatioWithoutSharedDimension { metric: MetricName, model: ModelName },
    /// A cross-model ratio whose second fact `model` does not name which of its relationships serves
    /// a lookup leg (`github.com/telekom/sutura#1493`).
    ///
    /// Each lookup leg a ratio groups by is joined to both facts on its own link, so the second fact
    /// needs exactly one relationship into that leg's model on the column the leg links through. Two
    /// relationships that qualify (a bill-to and a ship-to), or one relationship that would serve two
    /// legs, leave the choice to a guess, and a wrong guess is a wrong ratio. Which relationship
    /// serves which leg is the catalog's to say; a caller can only leave dimensions out, one of two
    /// that reach the same model or all that do.
    CrossModelRatioLinkAmbiguous { metric: MetricName, model: ModelName },
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
        use crate::model::{ModelName, SourceName, TableName};
        vec![
            RefusalReason::MetricUnknown {
                metric: MetricName::parse("revenue").expect("a test metric"),
            },
            RefusalReason::TooManyMetrics { requested: 9, limit: 8 },
            RefusalReason::DimensionNotConformed {
                dimension: DimensionName::parse("region").expect("a test dimension"),
                first: MetricName::parse("revenue").expect("a test metric"),
                other: MetricName::parse("margin").expect("a test metric"),
            },
            RefusalReason::QuestionNeedsTooManyLegs { legs: 8, limit: 6 },
            RefusalReason::DuplicateMetricName {
                metric: MetricName::parse("revenue").expect("a test metric"),
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
            RefusalReason::TooManyFilters {
                requested: 17,
                limit: 16,
            },
            RefusalReason::ResultTooLarge {
                bound: ResultBound::Rows { limit: 10_000 },
            },
            RefusalReason::TimeRangeTooLong { days: 9000, limit: 3653 },
            RefusalReason::PlanSpansTooManySources { sources: 5, limit: 4 },
            RefusalReason::PlanNeedsTooManyLookupLegs { legs: 4, limit: 3 },
            RefusalReason::FederationNotExecutable,
            RefusalReason::FederationLinkTypeMismatch {
                relationship: RelationshipName::parse("usage_subscription").expect("a test relationship"),
                fact_type: crate::plan::QuotedColumnType::from(
                    &crate::catalog::ColumnType::parse("BIGINT").expect("a test type"),
                ),
                lookup_type: crate::plan::QuotedColumnType::from(
                    &crate::catalog::ColumnType::parse("VARCHAR").expect("a test type"),
                ),
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
            RefusalReason::AddressOutsideDomains {
                source: SourceName::parse("warehouse").expect("a test source"),
            },
            RefusalReason::DeadlineExceeded { budget_seconds: 29 },
            RefusalReason::BudgetExhausted { reset_after_seconds: 41 },
            RefusalReason::TopOverUncertifiedRows { ceiling: 10_000 },
            RefusalReason::CrossModelRatioWithoutSharedCalendar {
                metric: MetricName::parse("revenue_per_customer").expect("a test metric"),
                model: ModelName::parse("customers").expect("a test model"),
            },
            RefusalReason::CrossModelRatioWithoutSharedDimension {
                metric: MetricName::parse("revenue_per_customer").expect("a test metric"),
                model: ModelName::parse("customers").expect("a test model"),
            },
            RefusalReason::CrossModelRatioLinkAmbiguous {
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
