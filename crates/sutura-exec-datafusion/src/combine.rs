//! The combiner: two legs' Arrow batches joined, re-aggregated and divided by a `DataFusion` plan.
//!
//! # What this is, and which record decided it
//!
//! `sutura_domain::plan::FederationCombiner`'s one implementor. `docs/adr/0007` designed that port
//! and recorded that it was not built; `docs/adr/0039` step 3 is the owner instruction that builds
//! it - *the combiner should be `DataFusion`*, so that a later multi-node move adopts a mechanism
//! rather than redeveloping one, and so that no path in this workspace walks a result one cell at a
//! time.
//!
//! **Nothing here is a hand join.** The whole combine is ONE logical plan - two in-memory tables,
//! one join, one aggregate, one projection, one sort - built out of the same
//! [`crate::translate`] and [`crate::collect`] functions the whole-plan and leg paths build theirs
//! out of. What this module adds is the three things only a combine needs: which column joins to
//! which, how a carried leaf re-aggregates, and the divide tree above every leg.
//!
//! # The bound, and exactly what it reaches
//!
//! `working_set_bytes` sizes a [`crate::pool`] `GreedyMemoryPool` for **this call's own session**,
//! which is why a combiner holds no session and builds one per combine: a pool is a property of a
//! `RuntimeEnv`, and the ceiling is a per-question argument. So the join build side, the aggregate
//! state and the sort - which is where the hand-written combine's own byte budget actually spent -
//! are operator reservations against that ceiling, and a reservation over it fails immediately with
//! nowhere to spill.
//!
//! **What it does NOT count, stated here rather than left to be discovered.** `crate::pool`'s own
//! header is the full list; the two that mattered for a combine were that the in-memory tables the
//! legs are registered as are NOT a reservation (they are batches the caller already holds, and
//! registering them copies no buffer), and that `collect()` materialising the answer was not one
//! either. So this is a bound on the combine's OPERATORS and on nothing else. The hand-written
//! combine counted the answer's own cells and not the operators; the two bounds cover different
//! things, and the honest summary is that the reach moved rather than widened. What still bounds
//! the answer's own size is `sutura_domain::plan::MAX_ROWS` and the response bound, both applied
//! by `sutura_app::federated` over the combined result.
//!
//! **One of those two gaps is now closed, and by a different mechanism rather than by the pool
//! widening.** `docs/adr/0009`'s fourth amendment builds the byte budget at the execution boundary,
//! and this module spends it: `collected` streams the combined answer into
//! `sutura_domain::warehouse::Accumulating` under `WorkingSet::result_budget`, charging both what
//! the batches cost to hold and what `to_rows` will cost to build. So *`collect()` materialising the
//! answer is not counted* is spent - nothing here calls `collect()` any more, and an over-budget
//! answer leaves as [`CombineError::Exhausted`] so the caller is told the configured number refused
//! it.
//!
//! **The other gap is unchanged, and so is the shape of the claim.** The `MemTable`s the legs are
//! registered as are still not a reservation and are still not charged here: they are the CALLER's
//! batches, already accumulated under the budget of whichever adapter produced them, and this module
//! never allocates a second copy of them. Neither bound is a superset of the other - the pool sees
//! operators and not results, this budget sees the result and not operators - and the two are sized
//! from the same configured number, so a combine's worst case is twice it rather than once.
//!
//! # Every refusal the hand-written combine made, and where each one went
//!
//! | Refused | Now |
//! | --- | --- |
//! | a missing column, a duplicate label | this module's own `schema` half, from the leg's own Arrow schema |
//! | a floating-point link key | ditto - from the TYPE, so an empty leg is judged too |
//! | two legs whose link columns can never match | ditto, and it no longer depends on which cell happened to be first |
//! | a non-numeric leaf | ditto |
//! | a link value with more than one lookup row | a `DataFusion` plan over the lookup leg: group, count, keep past one |
//! | a non-finite measure | one pass over the answer's measure column, and only when it is a float |
//! | a leaf column mixing integer and real cells | **not representable**: an Arrow column has one type |
//! | a leaf total past `i64::MAX` | **answered exactly** instead of refused - see `leaf_sum` below |

use std::sync::Arc;

use datafusion::arrow::array::{Array as _, Float64Array};
use datafusion::arrow::datatypes::DataType;
use datafusion::common::{Column, JoinType, TableReference};
use datafusion::datasource::MemTable;
use datafusion::functions::expr_fn::nullif;
use datafusion::functions_aggregate::expr_fn::{count, max, min, sum};
use datafusion::logical_expr::{Expr, LogicalPlan, LogicalPlanBuilder, cast, lit};
use datafusion::prelude::{SessionConfig, SessionContext};
// `StreamExt::next`, so the combined answer is charged batch by batch instead of collected first.
use futures_util::StreamExt as _;
use sutura_domain::federation::{Above, Carried};
use sutura_domain::measure::ZeroDenominator;
use sutura_domain::model::Aggregate;
use sutura_domain::plan::{
    AnswerKey, FederatedAnswerRefusal, FederatedPlan, FederationCombiner, InternalLabel, LegSide, Legs, labels,
};
use sutura_domain::warehouse::{Accumulating, ResultBatches, UnannouncedBatch};

use crate::collect::outputs;
use crate::pool::{self, WorkingSet};

/// The combine's own schema reading, which decides every type-level refusal before a plan is built.
mod schema;

use schema::{FACT, LOOKUP, LeafKind, LegSchema, agreeing_link};

/// The name the fact leg is registered under, so a column reference is QUALIFIED.
///
/// **Both legs project the link column under one internal label, so the join's output carries two
/// columns of that name** - and an unqualified reference to it is ambiguous. Registering each leg as
/// a named table is what makes `fact.0_link` and `lookup.0_link` two different expressions, and it
/// is also what lets a fact key and a lookup key share a label without colliding.
const FACT_TABLE: &str = "fact";

/// The name the lookup leg is registered under. See [`FACT_TABLE`].
const LOOKUP_TABLE: &str = "lookup";

/// The second fact leg's side word, for the refusals it carries.
///
/// A third leg arrives only for a cross-model ratio (`telekom/sutura#780`): a second fact over a
/// different model, joined above on the link and the time bucket. Its leaves are the ones whose
/// `Carried::model` is `Some`; the first fact leg carries the rest.
const SECOND_FACT: &str = "second_fact";

/// The name the second fact leg is registered under. See [`FACT_TABLE`].
const SECOND_FACT_TABLE: &str = "second_fact";

/// The label the ambiguity probe counts under.
///
/// In the reserved namespace, taken from the type rather than spelled, so it cannot collide with a
/// column either leg projects: a question cannot name a label beginning with a digit.
fn probe_label() -> String {
    InternalLabel::Leaf(usize::MAX).label()
}

/// How wide an exact leaf is accumulated. See [`leaf_sum`].
const EXACT_SUM_PRECISION: u8 = 76;

/// Why a combine could not be assembled.
///
/// Split the way the port's two predicates read it: the four caller-facing arms are deterministic
/// refusals about the DATA the legs returned, and the rest are this workspace's own wiring or the
/// engine's. Every arm carries an Arrow type or a label where it carries anything at all - a
/// driver's metadata and a label the splitter assigned - and never a cell.
#[derive(Debug, thiserror::Error)]
pub enum CombineError {
    /// A tokio runtime could not be built for this combiner.
    #[error("could not build a runtime for the combiner")]
    Runtime {
        #[source]
        cause: std::io::Error,
    },
    /// The bounded execution environment could not be built.
    #[error("could not build a bounded execution environment for the combine")]
    Environment {
        #[source]
        cause: crate::DataFusionError,
    },
    /// A leg's batches could not be registered as an in-memory table.
    #[error("could not register the {side} leg's result as a table")]
    Register {
        side: &'static str,
        #[source]
        cause: datafusion::error::DataFusionError,
    },
    /// The combine's logical plan could not be built.
    #[error("could not build the combine's logical plan")]
    Build {
        #[source]
        cause: datafusion::error::DataFusionError,
    },
    /// The combine's plan could not be resolved against the registered tables.
    #[error("could not resolve the combine's logical plan")]
    Analyze {
        #[source]
        cause: datafusion::error::DataFusionError,
    },
    /// The combine's plan failed while executing, for a reason that is not the ceiling.
    #[error("the combine did not execute")]
    Execute {
        #[source]
        cause: datafusion::error::DataFusionError,
    },
    /// The ceiling refused a reservation.
    ///
    /// **Carries the ceiling, because nothing else can.** `Warehouse::working_set_exhausted` reads
    /// the bound off the adapter, which holds one for its whole life; a combiner's ceiling is a
    /// per-question argument, so the only place that knows which number fired is the call that
    /// passed it.
    #[error("the combine exceeded the {ceiling_bytes}-byte working-set ceiling")]
    Exhausted { ceiling_bytes: u64 },
    /// A batch the engine produced did not agree with the schema it announced.
    #[error("the combine produced a batch its own announced schema does not describe")]
    Unannounced {
        #[source]
        cause: UnannouncedBatch,
    },
    /// The answer came back under labels the plan does not project.
    #[error("the combine projected {actual:?} where the plan asks for {expected:?}")]
    SchemaMismatch { expected: Vec<String>, actual: Vec<String> },
    /// A column the plan named is absent from a leg's result.
    #[error("the {side} leg's result has no column `{label}`")]
    MissingColumn { side: &'static str, label: String },
    /// A leg's result labels two columns the same, so no reference to that label is unambiguous.
    #[error("the {side} leg's result labels two columns `{label}`")]
    DuplicateLabels { side: &'static str, label: String },
    /// A carried leaf names an aggregate no re-aggregating expression exists for.
    ///
    /// Unreachable through `FederatedPlan::new`, which refuses such a leaf before a plan exists.
    /// Kept rather than assumed away: the constructor is the only thing closing it, and a second
    /// producer of plans would not be.
    #[error("the combine has no re-aggregating expression for `{aggregate}`")]
    UnsupportedAggregate { aggregate: Aggregate },
    /// A link column carries an Arrow type this domain maps no cell of, so a question naming it
    /// could not have been read even if it joined.
    #[error("the {side} leg's link column `{label}` came back as {arrow_type}, which this domain does not map")]
    LinkTypeNotMapped {
        side: &'static str,
        label: String,
        arrow_type: String,
    },
    /// A link column carries a floating-point type, which the float-key rule forbids.
    #[error("the {side} leg's link column came back as {arrow_type}, and a float is not an exact join key")]
    FloatLinkKey { side: &'static str, arrow_type: String },
    /// The two legs' link columns carry kinds that can never match.
    #[error("the fact leg's link column is {fact} and the lookup leg's is {lookup}, so no row can match")]
    LinkTypeMismatch { fact: &'static str, lookup: &'static str },
    /// A link value maps to more than one lookup row, or to more than one second-fact row in one
    /// bucket, which would double every measure under it.
    ///
    /// Carries no key: a join key is exactly the caller data this workspace keeps out of a message.
    #[error("a link value maps to more than one row of a leg joined on it")]
    AmbiguousLink,
    /// A carried leaf's column is not a type an exact total or comparison can be taken over.
    #[error("the carried leaf `{label}` came back as {arrow_type}, which no exact re-aggregation reads")]
    NonNumericLeaf { label: String, arrow_type: String },
    /// The measure is not a finite number.
    #[error("the combined answer's measure is not a finite number")]
    NonFinite,
}

/// The combiner: a tokio runtime, and a bounded session built per combine.
///
/// **It holds no session, and that is the whole reason the ceiling is a real bound.** A
/// `GreedyMemoryPool` is installed on a `RuntimeEnv` and a `RuntimeEnv` is installed on a
/// `SessionContext`, so a combiner that kept one session would have to fix the ceiling at
/// construction - and the ceiling is what a deployment configures per question. One session per
/// combine also means a combine's registered tables cannot outlive it, which is what keeps one
/// caller's leg results out of another's session.
///
/// **What it does NOT hold is an identity, and that is not an omission.** `crate::pool`'s process
/// is one operating-system identity, so a combine runs as the deployment. What the combiner carries
/// per subject instead is a [`ComputeContext`](sutura_domain::identity::ComputeContext) - see
/// [`Self::for_subject`].
pub struct DataFusionCombiner {
    /// An `Option` so [`Drop`] can take it out, which is `DataFusionWarehouse`'s own reason: a
    /// runtime dropped inside an async context aborts.
    runtime: Option<tokio::runtime::Runtime>,
    /// The per-subject discriminator this combine's session is keyed on, when a subject is in hand.
    context: Option<sutura_domain::identity::ComputeContext>,
}

impl core::fmt::Debug for DataFusionCombiner {
    /// Names the type and whether a subject context is held, and nothing else.
    ///
    /// **Never the context's own value.** `ComputeContext`'s digest is opaque by construction, but a
    /// `Debug` that printed it would put a stable per-subject discriminator into any log that
    /// formats a service - which is the disclosure the digest exists to bound, one layer out.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DataFusionCombiner")
            .field("keyed_per_subject", &self.context.is_some())
            .finish_non_exhaustive()
    }
}

impl Drop for DataFusionCombiner {
    /// Takes the runtime out and drops it on a thread of its own, for
    /// `DataFusionWarehouse::drop`'s measured reason: dropping a tokio runtime from inside an async
    /// context panics, and shipped profiles compile `panic = "abort"`.
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            drop(std::thread::spawn(move || drop(runtime)));
        }
    }
}

impl DataFusionCombiner {
    /// A combiner with no subject in hand: what a composition root builds once and shares.
    ///
    /// # Errors
    ///
    /// [`CombineError::Runtime`] if a current-thread runtime cannot be built.
    pub fn new() -> Result<Self, CombineError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .map_err(|cause| CombineError::Runtime { cause })?;
        Ok(Self {
            runtime: Some(runtime),
            context: None,
        })
    }

    /// The same combiner, keyed to one subject asking of one source.
    ///
    /// **This is the wiring `docs/adr/0039` step 5 held `ComputeContext` back for, and the property
    /// it carries is a negative one.** `datafusion-federation`'s provider equality is
    /// `name() == name() && compute_context() == compute_context()`, so two providers for one source
    /// on behalf of two subjects that compare EQUAL are fused by the optimizer into one federated
    /// node executed through one of them - one caller's scan on the other caller's credential, with
    /// no bug on either caller's own path. A per-subject digest makes them unequal, which is the
    /// optimizer's *ambiguous* path: each single-source sub-plan federated separately.
    ///
    /// **What it is worth TODAY, stated next to the claim rather than after it.** This build depends
    /// on no federation provider - `docs/adr/0039` step 4 is blocked ahead of one, and
    /// `cargo xtask unused-deps` would fail a declared dependency no crate names - so nothing in
    /// this process compares two contexts yet. What the method buys now is that the seam is the one
    /// a provider plugs into, and that the value reaching it is a digest rather than a subject: the
    /// context is interpolated into plan text by that crate, so a raw subject there would be a
    /// person's identifier in `EXPLAIN` output. The type is what makes that unrepresentable -
    /// `ComputeContext::of` takes a `Subject` and there is no other constructor.
    ///
    /// It is deliberately NOT part of [`FederationCombiner`]: a port method taking a subject would
    /// say the combine executes as one, and `crate::pool`'s process does not.
    #[must_use]
    pub fn for_subject(mut self, context: sutura_domain::identity::ComputeContext) -> Self {
        self.context = Some(context);
        self
    }

    /// The per-subject discriminator this combiner is keyed on, if a root handed one over.
    ///
    /// The digest as text, which is what a federation provider's `compute_context` returns. `None`
    /// is a combiner a root built without a subject, which is every path in this build today.
    #[must_use]
    pub fn compute_context(&self) -> Option<&str> {
        self.context.as_ref().map(sutura_domain::identity::ComputeContext::as_str)
    }

    /// The tokio runtime this combine executes on.
    ///
    /// Answered as an error rather than unwrapped, for `DataFusionWarehouse::runtime`'s reason: the
    /// only path to `None` is this type's own [`Drop`], which nothing reaches.
    fn runtime(&self) -> Result<&tokio::runtime::Runtime, CombineError> {
        self.runtime.as_ref().ok_or_else(|| CombineError::Runtime {
            cause: std::io::Error::other("the runtime was already taken out by the combiner's drop"),
        })
    }

    /// One combine, in a session bounded to this call's own ceiling.
    async fn assembled(
        plan: &FederatedPlan,
        legs: Legs<'_>,
        working_set: WorkingSet,
        ceiling_bytes: u64,
    ) -> Result<ResultBatches, CombineError> {
        let fact = LegSchema::of(FACT, legs.fact().schema())?;
        let lookup = LegSchema::of(LOOKUP, legs.lookup().schema())?;
        let link = InternalLabel::Link.label();
        agreeing_link(fact.link_kind(&link)?, lookup.link_kind(&link)?)?;
        // A cross-model ratio (`telekom/sutura#780`) carries a second fact leg; the two facts are
        // joined above on the link and the bucket, so its link kind must agree with the first.
        let second_fact = match (plan.second_fact(), legs.second_fact()) {
            (Some(_), Some(batches)) => Some(LegSchema::of(SECOND_FACT, batches.schema())?),
            (Some(_), None) => {
                return Err(CombineError::MissingColumn {
                    side: SECOND_FACT,
                    label: link.clone(),
                });
            }
            (None, _) => None,
        };
        if let Some(second) = &second_fact {
            agreeing_link(fact.link_kind(&link)?, second.link_kind(&link)?)?;
        }
        let leaves = leaf_labels(plan, &fact, second_fact.as_ref())?;
        fact.projects(plan.bucket_label())?;
        if let Some(second) = &second_fact {
            second.projects(plan.bucket_label())?;
        }
        for key in plan.keys() {
            match key.side() {
                LegSide::Fact => fact.projects(key.label())?,
                LegSide::Lookup => lookup.projects(key.label())?,
            }
        }

        let environment = pool::environment(working_set).map_err(|cause| CombineError::Environment { cause })?;
        let context = SessionContext::new_with_config_rt(SessionConfig::new(), environment.into_runtime());
        register(&context, FACT_TABLE, FACT, legs.fact())?;
        register(&context, LOOKUP_TABLE, LOOKUP, legs.lookup())?;
        if let (Some(_), Some(batches)) = (&second_fact, legs.second_fact()) {
            register(&context, SECOND_FACT_TABLE, SECOND_FACT, batches)?;
        }

        refuse_ambiguous_link(&context, LOOKUP_TABLE, &[&link], ceiling_bytes, working_set).await?;
        // The second fact is joined on the link AND the bucket, so a second row for one pair fans
        // every first-fact row under it out exactly as a second lookup row would.
        if second_fact.is_some() {
            let on = [link.as_str(), plan.bucket_label()];
            refuse_ambiguous_link(&context, SECOND_FACT_TABLE, &on, ceiling_bytes, working_set).await?;
        }

        let second_table = second_fact.as_ref().map(|_| SECOND_FACT_TABLE);
        let logical = combine_plan(plan, &context, &link, &leaves, second_table).await?;
        let frame = context
            .execute_logical_plan(logical)
            .await
            .map_err(|cause| CombineError::Analyze { cause })?;
        let labels = answer_labels(plan);
        let actual: Vec<String> = frame
            .schema()
            .fields()
            .iter()
            .map(|field| String::from(field.name().as_str()))
            .collect();
        if actual != labels {
            return Err(CombineError::SchemaMismatch {
                expected: labels,
                actual,
            });
        }
        let answered = collected(frame, ceiling_bytes, working_set).await?;
        refuse_non_finite(&answered)?;
        Ok(answered)
    }
}

impl FederationCombiner for DataFusionCombiner {
    type Error = CombineError;

    fn combine(&self, plan: &FederatedPlan, legs: Legs<'_>, working_set_bytes: u64) -> Result<ResultBatches, Self::Error> {
        // A ceiling of zero, or one wider than this target's pointers, is not a pool this engine can
        // install. Clamped to one byte rather than defaulted to unbounded: an unbounded pool under
        // `panic = "abort"` makes a wide join process death for every caller in flight, and a
        // one-byte pool refuses the first reservation - which is a refusal the caller can read.
        let clamped = usize::try_from(working_set_bytes).unwrap_or(usize::MAX);
        let bytes = core::num::NonZeroUsize::new(clamped).unwrap_or(core::num::NonZeroUsize::MIN);
        let working_set = WorkingSet::of_bytes(bytes);
        self.runtime()?
            .block_on(Self::assembled(plan, legs, working_set, working_set_bytes))
    }

    fn working_set_exhausted(&self, error: &Self::Error) -> Option<u64> {
        match *error {
            CombineError::Exhausted { ceiling_bytes } => Some(ceiling_bytes),
            _ => None,
        }
    }

    /// **Exhaustive with no wildcard arm**, so a variant added to [`CombineError`] has to say which
    /// side of the split it is on before this compiles. The direction the mistake would fall in is
    /// the one that tells a caller to retry a refusal that returns the same answer.
    fn answer_not_well_formed(&self, error: &Self::Error) -> Option<FederatedAnswerRefusal> {
        match *error {
            CombineError::NonFinite => Some(FederatedAnswerRefusal::NonFinite),
            CombineError::FloatLinkKey { .. } => Some(FederatedAnswerRefusal::FloatLinkKey),
            CombineError::AmbiguousLink => Some(FederatedAnswerRefusal::AmbiguousLink),
            CombineError::LinkTypeMismatch { .. } => Some(FederatedAnswerRefusal::LinkTypeMismatch),
            CombineError::NonNumericLeaf { .. } => Some(FederatedAnswerRefusal::NonNumericLeaf),
            // Ours, the engine's, or the deployment's - never the question's. `Exhausted` is
            // deliberately here: it has its own refusal, which `working_set_exhausted` answers and
            // `sutura_app` asks first.
            CombineError::Runtime { .. }
            | CombineError::Environment { .. }
            | CombineError::Register { .. }
            | CombineError::Build { .. }
            | CombineError::Analyze { .. }
            | CombineError::Execute { .. }
            | CombineError::Exhausted { .. }
            | CombineError::Unannounced { .. }
            | CombineError::SchemaMismatch { .. }
            | CombineError::MissingColumn { .. }
            | CombineError::DuplicateLabels { .. }
            | CombineError::UnsupportedAggregate { .. }
            | CombineError::LinkTypeNotMapped { .. } => None,
        }
    }
}

/// One leg's batches, registered under `table` for the length of one combine.
///
/// `MemTable` over the batches the caller already holds: an Arrow batch is reference-counted
/// buffers, so this copies no cell and reserves nothing against the pool. `crate::pool`'s header is
/// where that limit on the ceiling's reach is stated.
fn register(context: &SessionContext, table: &str, side: &'static str, batches: &ResultBatches) -> Result<(), CombineError> {
    let memory = MemTable::try_new(Arc::clone(batches.schema()), vec![batches.batches().to_vec()])
        .map_err(|cause| CombineError::Register { side, cause })?;
    context
        .register_table(TableReference::bare(table), Arc::new(memory))
        .map(|_replaced| ())
        .map_err(|cause| CombineError::Register { side, cause })
}

/// The answer's column labels, in the order the answer projects them.
///
/// Keys in question order, then the fact leg's time bucket, then the measure under the metric's own
/// certified name. **The same order the mono path emits**, which is what makes a federated answer
/// and a single-source one comparable - and what lets `FederatedPlan::rank` find the measure by
/// position rather than by label.
fn answer_labels(plan: &FederatedPlan) -> Vec<String> {
    let mut labels: Vec<String> = plan.keys().iter().map(|key| String::from(key.label())).collect();
    labels.push(String::from(plan.bucket_label()));
    labels.push(String::from(plan.measure_label()));
    labels
}

/// Every carried leaf, in `labels` order.
type Leaves = Vec<Leaf>;

/// One carried leaf: the label the splitter projected it under, how its column re-aggregates, and
/// the registered table it is read from.
///
/// A named alias because the spelled-out form is past `clippy.toml`'s type-complexity threshold,
/// and naming it says which part of the triple is the label.
type Leaf = (String, LeafKind, &'static str);

/// The label of every carried leaf, in carried order, with its type judged.
///
/// `labels(plan.federation())` is the SAME function the splitter names the fact leg's terms with, so
/// the column a leaf is read from and the label it was projected under cannot disagree - there is no
/// second copy of the naming rule. A two-fact plan (`telekom/sutura#780`) reads a leaf whose
/// `Carried::model` is `Some` off the second fact leg; every other leaf, and every leaf of a plan
/// with no second fact, reads the first. Decided here once, with the table it names, so the
/// expression built from a leaf cannot read a table this check did not.
fn leaf_labels(plan: &FederatedPlan, fact: &LegSchema, second_fact: Option<&LegSchema>) -> Result<Leaves, CombineError> {
    let carried = plan.federation().carried();
    labels(plan.federation())
        .into_iter()
        .zip(carried.iter())
        .map(|(internal, carried)| {
            let label = internal.label();
            let (schema, table) = match (carried.model(), second_fact) {
                (Some(_), Some(second)) => (second, SECOND_FACT_TABLE),
                _ => (fact, FACT_TABLE),
            };
            let kind = schema.leaf_kind(&label)?;
            Ok((label, kind, table))
        })
        .collect()
}

/// A qualified reference to one leg's column, built rather than parsed.
///
/// `Column::new` with a bare `TableReference`, never `col("fact.0_link")`: the parsing form
/// lowercases every part of a dotted name, which is the case-folding trap `crate::translate::column`
/// exists to avoid - and an internal label begins with a digit, which a parser would read as the
/// start of a number.
fn qualified(table: &str, label: &str) -> Expr {
    Expr::Column(Column::new(Some(TableReference::bare(table)), label))
}

/// The table one answer key is read from.
const fn table_of(key: &AnswerKey) -> &'static str {
    match key.side() {
        LegSide::Fact => FACT_TABLE,
        LegSide::Lookup => LOOKUP_TABLE,
    }
}

/// The whole combine, as one logical plan.
async fn combine_plan(
    plan: &FederatedPlan,
    context: &SessionContext,
    link: &str,
    leaves: &[Leaf],
    second_table: Option<&str>,
) -> Result<LogicalPlan, CombineError> {
    let fact = scan(context, FACT_TABLE).await?;
    let lookup = scan(context, LOOKUP_TABLE).await?;
    // A second fact leg (`telekom/sutura#780`) is joined INNER on the link AND the bucket: both
    // legs are grouped by both, so each first-fact row meets at most the one second-fact row of its
    // own period. On the link alone a second fact with two periods fans every first-fact row out
    // across both and multiplies the numerator. A row present in one fact and absent in the other is
    // dropped rather than null-padded; the lookup join below is the splitter's decision, apart.
    let mut builder = LogicalPlanBuilder::from(fact);
    if let Some(table) = second_table {
        let second = scan(context, table).await?;
        let bucket = plan.bucket_label();
        let on = [
            qualified(FACT_TABLE, link).eq(qualified(table, link)),
            qualified(FACT_TABLE, bucket).eq(qualified(table, bucket)),
        ];
        builder = builder
            .join_on(second, JoinType::Inner, on)
            .map_err(|cause| CombineError::Build { cause })?;
    }
    // **The join kind is the splitter's decision, carried on the plan rather than guessed.** LEFT
    // keeps a fact row whose link value found no lookup row, with null remote keys; INNER drops it.
    // A NULL link value is decided identically under both, and by the join rather than by an arm of
    // ours: `NULL = NULL` is not true, so such a row matches nothing.
    let kind = if plan.include_unmatched() {
        JoinType::Left
    } else {
        JoinType::Inner
    };
    let on = qualified(FACT_TABLE, link).eq(qualified(LOOKUP_TABLE, link));
    let mut builder = builder
        .join_on(lookup, kind, [on])
        .map_err(|cause| CombineError::Build { cause })?;

    // Keys in question order, then the bucket - the order `answer_labels` states, so the aggregate's
    // output fields line up with the labels position for position.
    let mut grouping: Vec<Expr> = plan.keys().iter().map(|key| qualified(table_of(key), key.label())).collect();
    grouping.push(qualified(FACT_TABLE, plan.bucket_label()));
    let group_count = grouping.len();

    let measure = above_expression(plan.federation().above(), leaves, &mut 0)?;
    builder = builder
        .aggregate(grouping, vec![measure])
        .map_err(|cause| CombineError::Build { cause })?;

    let labels = answer_labels(plan);
    let (projection, ordering) = outputs(builder.schema(), &labels, group_count).map_err(schema_mismatch)?;
    // `sort_by` is ascending, nulls last - the ordered-result contract
    // `sutura_sql::generate::ordered_nulls_last` states for the rendered path and the hand-written
    // combine's own comparator restated. A null means there was nothing to group under, and that
    // sorts after every value.
    builder
        .project(projection)
        .and_then(|projected| projected.sort_by(ordering))
        .and_then(LogicalPlanBuilder::build)
        .map_err(|cause| CombineError::Build { cause })
}

/// One registered leg, as a plan to build on.
async fn scan(context: &SessionContext, table: &str) -> Result<LogicalPlan, CombineError> {
    let frame = context
        .table(TableReference::bare(table))
        .await
        .map_err(|cause| CombineError::Analyze { cause })?;
    Ok(frame.into_unoptimized_plan())
}

/// [`crate::collect::outputs`]'s own refusal, as this module's.
fn schema_mismatch(cause: crate::DataFusionError) -> CombineError {
    match cause {
        crate::DataFusionError::SchemaMismatch { expected, actual } => CombineError::SchemaMismatch { expected, actual },
        other => CombineError::Build {
            cause: datafusion::error::DataFusionError::External(Box::new(other)),
        },
    }
}

/// The divide tree above the legs, as one expression over the aggregate's own inputs.
///
/// `cursor` walks the tree in the same order [`labels`] collects the federation's leaves in, which
/// is what pairs each [`Above::Total`] node with the column its own leaf was projected under. The
/// one caller starts it at zero, so there is no call site that can point it elsewhere.
///
/// **The division happens HERE and cannot happen in a leg**, which is `sutura_domain::federation`'s
/// own shape: a `Carried` has no quotient variant, so a per-leg guard is unrepresentable and the
/// [`ZeroDenominator`] the definition asked for is applied once, above every leg's rows.
fn above_expression(above: &Above, leaves: &[Leaf], cursor: &mut usize) -> Result<Expr, CombineError> {
    match *above {
        Above::Total(ref carried) => {
            let &(ref label, kind, table) = leaves.get(*cursor).ok_or_else(|| CombineError::MissingColumn {
                side: FACT,
                label: InternalLabel::Leaf(*cursor).label(),
            })?;
            *cursor = cursor.saturating_add(1);
            leaf_expression(carried, qualified(table, label), kind)
        }
        Above::Quotient {
            ref numerator,
            ref denominator,
            zero_denominator,
        } => {
            // The same arithmetic `crate::translate::measure_expression` emits for the mono path, so
            // a ratio cannot mean one thing for one source and another for two: the numerator casts
            // to a float first (integer division truncates, which would silently answer a whole
            // number), and the zero handling is the definition's.
            let top = cast(above_expression(numerator, leaves, cursor)?, DataType::Float64);
            let bottom = cast(above_expression(denominator, leaves, cursor)?, DataType::Float64);
            let bottom = match zero_denominator {
                ZeroDenominator::Null => nullif(bottom, lit(0.0_f64)),
                // No guard, deliberately: the division answers a non-finite value and
                // `refuse_non_finite` refuses the answer. That is the same mechanism the engine's own
                // mono path has for this case - the alternative would be a second definition of what
                // `fails` means.
                ZeroDenominator::Fail => bottom,
            };
            Ok(top / bottom)
        }
    }
}

/// One carried leaf, re-aggregated by its own [`Carried::combine`].
///
/// Only three aggregates can arrive: `FederatedPlan::new` refuses a leaf whose `combine` has no
/// re-aggregating function, so the fourth arm is a refusal for a plan this workspace's own
/// constructor could not have built.
fn leaf_expression(carried: &Carried, column: Expr, kind: LeafKind) -> Result<Expr, CombineError> {
    let aggregate = carried.combine();
    match aggregate {
        Aggregate::Sum => Ok(leaf_sum(column, kind)),
        // No cast: a minimum and a maximum are exact in the column's own type, and casting one would
        // be the widening `sutura_domain::warehouse::arrow` refuses for a 32-bit float.
        Aggregate::Min => Ok(min(column)),
        Aggregate::Max => Ok(max(column)),
        Aggregate::Count | Aggregate::Avg | Aggregate::CountDistinct => Err(CombineError::UnsupportedAggregate { aggregate }),
    }
}

/// A leaf's total, accumulated wide enough that no total this workspace can produce wraps.
///
/// **`DataFusion`'s sum accumulator adds with WRAPPING arithmetic**, measured in the pinned 55.1.0
/// source: both the scalar and the grouped accumulator use `add_wrapping`, and `arrow`'s own
/// `sum` kernel documents that an overflow wraps rather than erroring. So the accumulator's WIDTH is
/// the bound, and the hand-written combine's `checked_add`-and-refuse is not available.
///
/// An exact leaf is therefore cast to a 256-bit decimal at the column's own scale before summing,
/// which is a better answer than the refusal it replaces: `ResultBatches::to_rows` widens a
/// zero-scale value that fits an `i64` back to an integer and renders one that does not as its
/// exact text, so a total past `i64::MAX` comes back **exact** where it used to be refused.
///
/// **The limit, next to the claim.** A 256-bit accumulator holds about `10^76`; a leaf value a
/// `Decimal128` column can carry is at most about `10^38`, so wrapping needs on the order of
/// `10^38` rows. No row ceiling in this workspace permits that, and it is a width rather than a
/// guard - a combiner that narrowed the accumulator would lose the property with nothing to say so.
///
/// A float leaf is summed as a float, which is the same addition the mono path's own `SUM` performs;
/// a total that leaves the finite range is refused by [`refuse_non_finite`].
fn leaf_sum(column: Expr, kind: LeafKind) -> Expr {
    match kind {
        LeafKind::Exact { scale } => sum(cast(column, DataType::Decimal256(EXACT_SUM_PRECISION, scale))),
        LeafKind::Float => sum(column),
    }
}

/// Refuses a joined leg (`table`) that maps one value of its join columns (`on`) to more than one row.
///
/// **A `DataFusion` plan rather than a walk**, and it reads no cell: group the leg by its join
/// columns, count each group, keep the groups past one, and stop at the first. What the combine
/// learns is whether that result is EMPTY.
///
/// More than one lookup row for one link value doubles every measure joined to it, so it is refused
/// rather than certified. Null join values are excluded first, because a null never joins at all -
/// two null-linked lookup rows duplicate nothing.
///
/// **The cost, stated where it is paid:** each checked leg is scanned twice, once here and once in the
/// join. The alternative is a pre-aggregation joined into the same plan, which would have to read
/// the count back out of the answer to refuse - so this is the shape that keeps the refusal
/// separable from the number.
async fn refuse_ambiguous_link(
    context: &SessionContext,
    table: &str,
    on: &[&str],
    ceiling_bytes: u64,
    working_set: WorkingSet,
) -> Result<(), CombineError> {
    let probe = probe_label();
    let scanned = scan(context, table).await?;
    let columns: Vec<Expr> = on.iter().map(|label| qualified(table, label)).collect();
    let joinable = columns
        .iter()
        .fold(lit(true), |all, column| all.and(column.clone().is_not_null()));
    let logical = LogicalPlanBuilder::from(scanned)
        .filter(joinable)
        .and_then(|filtered| filtered.aggregate(columns, vec![count(lit(1_i64)).alias(probe.as_str())]))
        .and_then(|grouped| {
            let counted = Expr::Column(Column::new_unqualified(probe.as_str()));
            grouped.filter(counted.gt(lit(1_i64)))
        })
        .and_then(|past_one| past_one.limit(0, Some(1)))
        .and_then(LogicalPlanBuilder::build)
        .map_err(|cause| CombineError::Build { cause })?;
    let frame = context
        .execute_logical_plan(logical)
        .await
        .map_err(|cause| CombineError::Analyze { cause })?;
    let found = collected(frame, ceiling_bytes, working_set).await?;
    if found.rows() > 0 {
        return Err(CombineError::AmbiguousLink);
    }
    Ok(())
}

/// A frame's batches, streamed into the guard, checked against the frame's own announced schema and
/// charged against the byte budget as they arrive.
///
/// The row ceiling is `usize::MAX` for `crate::collected`'s own reason - `docs/adr/0009` retired the
/// row cap because a row count is not a memory bound - and the byte budget is what replaced it.
/// Nothing calls `DataFrame::collect` here any more: a combine that retained every batch before
/// charging anything would refuse after the memory was spent.
///
/// **An over-budget combined answer leaves as [`CombineError::Exhausted`] and not as
/// [`CombineError::Unannounced`]**, which is a classification decision rather than a convenience.
/// The budget is the working-set ceiling (`WorkingSet::result_budget`), so what the caller needs to
/// be told is that this question wanted more memory than the configured number - the same thing a
/// refused operator reservation means, from the same number, and `working_set_exhausted` is the one
/// predicate the combiner port has to say it with. Routing it through `Unannounced` instead would
/// reach the caller as a data-system failure inviting a retry.
async fn collected(
    frame: datafusion::prelude::DataFrame,
    ceiling_bytes: u64,
    working_set: WorkingSet,
) -> Result<ResultBatches, CombineError> {
    let budget = working_set.result_budget();
    let announced = Arc::clone(frame.schema().inner());
    // `pool::exhausted` uses `find_root`, because a reservation is refused deep inside an operator
    // and the engine wraps the error on the way out. Matching the outermost variant would report
    // almost every real exhaustion as a transport failure, which is the direction that tells a
    // caller to retry a bound that fires again in the same place.
    let exhaustion = |cause: datafusion::error::DataFusionError| {
        if pool::exhausted(&cause) {
            CombineError::Exhausted { ceiling_bytes }
        } else {
            CombineError::Execute { cause }
        }
    };
    let mut stream = frame.execute_stream().await.map_err(exhaustion)?;
    let mut accumulating = Accumulating::announcing(announced, usize::MAX, budget);
    while let Some(batch) = stream.next().await {
        accumulating.push(batch.map_err(exhaustion)?).map_err(|cause| match cause {
            UnannouncedBatch::OverBudget { .. } => CombineError::Exhausted { ceiling_bytes },
            UnannouncedBatch::OverBound { .. } | UnannouncedBatch::Width { .. } | UnannouncedBatch::Mislabelled { .. } => {
                CombineError::Unannounced { cause }
            }
        })?;
    }
    Ok(accumulating.finish())
}

/// Refuses an answer whose measure is not a finite number.
///
/// **One pass over ONE column, and only when that column is a float.** The measure is the answer's
/// last column by construction - [`answer_labels`] states the order - and only a `Float64` measure
/// can be non-finite: an exact decimal cannot represent one.
///
/// **Why the combiner refuses this rather than letting the presentation edge do it.**
/// `ResultBatches::to_rows` already refuses a non-finite double, so the answer would be refused
/// either way - but as `sutura_app::ServiceError::Unreadable`, which reaches a caller as a
/// data-system failure. It is neither: a definition that declared `fails` for a zero denominator
/// asked for exactly this, and a caller told to retry it retries forever. So the refusal is taken
/// here, where it can be classified as
/// [`FederatedAnswerRefusal::NonFinite`](sutura_domain::plan::FederatedAnswerRefusal::NonFinite).
fn refuse_non_finite(answered: &ResultBatches) -> Result<(), CombineError> {
    let measure = answered.schema().fields().len().saturating_sub(1);
    let Some(field) = answered.schema().fields().get(measure) else {
        return Ok(());
    };
    if *field.data_type() != DataType::Float64 {
        return Ok(());
    }
    for batch in answered.batches() {
        let Some(column) = batch.columns().get(measure) else {
            continue;
        };
        let Some(floats) = column.as_any().downcast_ref::<Float64Array>() else {
            continue;
        };
        if floats.iter().flatten().any(|value| !value.is_finite()) {
            return Err(CombineError::NonFinite);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
