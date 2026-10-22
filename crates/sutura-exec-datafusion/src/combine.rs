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
//! | a link pair its relationship declares a cast for | `cast`: the link column cast with `safe: false`, so a value that does not cast refuses the answer |
//! | a link value with more than one lookup row | a `DataFusion` plan over the lookup leg: group, count, keep past one |
//! | a non-finite measure | one pass over the answer's measure column, and only when it is a float |
//! | a leaf column mixing integer and real cells | **not representable**: an Arrow column has one type |
//! | a leaf total past `i64::MAX` | **answered exactly** instead of refused - see `leaf_sum` below |

use std::sync::Arc;

use datafusion::arrow::array::{Array as _, Float64Array, RecordBatch};
use datafusion::arrow::datatypes::{DataType, SchemaRef};
use datafusion::common::{Column, TableReference};
use datafusion::datasource::MemTable;
use datafusion::functions_aggregate::expr_fn::count;
use datafusion::logical_expr::{Expr, LogicalPlan, LogicalPlanBuilder, lit};
use datafusion::prelude::{SessionConfig, SessionContext};
// `StreamExt::next`, so the combined answer is charged batch by batch instead of collected first.
use futures_util::StreamExt as _;
use sutura_domain::model::Aggregate;
use sutura_domain::plan::{FederatedAnswerRefusal, FederatedPlan, FederationCombiner, InternalLabel, LegAt, Legs};
use sutura_domain::query::MAX_LOOKUP_LEGS;
use sutura_domain::warehouse::{Accumulating, ResultBatches, UnannouncedBatch};

use crate::pool::{self, WorkingSet};

/// The one logical plan above the legs, and the labels and leaves it reads.
mod arm;
/// The cast a relationship's declared key comparison asks of a link column.
mod cast;
/// The drill-across merge: several arms joined above their legs, in this one session.
mod merge;
/// The provider a source and a subject give the plan cutter.
mod provider;
/// The combine's own schema reading, which decides every type-level refusal before a plan is built.
mod schema;
pub use provider::SourceProvider;

use arm::{answer_labels, combine_plan, leaf_labels, qualified, scan};
use cast::{Cast, LinkCasts};
use schema::{FACT, LegSchema, agreeing_link};

/// The name the fact leg is registered under, so a column reference is QUALIFIED.
///
/// **Both legs project the link column under one internal label, so the join's output carries two
/// columns of that name** - and an unqualified reference to it is ambiguous. Registering each leg as
/// a named table is what makes `fact.0_link` and `lookup.0_link` two different expressions, and it
/// is also what lets a fact key and a lookup key share a label without colliding.
const FACT_TABLE: &str = "fact";

/// The names the lookup legs are registered under, by position, and the side word each one's
/// refusals carry. See [`FACT_TABLE`]. One per leg [`MAX_LOOKUP_LEGS`] allows, so a plan cannot
/// carry a lookup leg with no table to register it under.
const LOOKUP_TABLES: [&str; MAX_LOOKUP_LEGS] = ["lookup", "lookup_1", "lookup_2"];

/// The table the leg at `at` is registered under, or `label` missing from a lookup leg the plan
/// does not carry - which `FederatedPlan::new` does not build.
fn table_at(at: LegAt, label: &str) -> Result<&'static str, CombineError> {
    match at {
        LegAt::Fact => Ok(FACT_TABLE),
        LegAt::Lookup(position) => LOOKUP_TABLES.get(position).copied().ok_or_else(|| missing_lookup(label)),
    }
}

/// `label` read from a lookup leg the plan does not carry.
fn missing_lookup(label: &str) -> CombineError {
    CombineError::MissingColumn {
        side: "lookup",
        label: String::from(label),
    }
}

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

/// How wide an exact leaf is accumulated. See `arm::leaf_sum`.
const EXACT_SUM_PRECISION: u8 = 76;

/// Why a combine could not be assembled.
///
/// Split the way the port's two predicates read it: the caller-facing arms are deterministic
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
    /// Unreachable through `Carried`, whose `combine` is a `Sum`, a `Min`, a `Max` or a
    /// `CountDistinct` over a pulled-up key. Kept rather than assumed away: the classification is the
    /// only thing closing it, and a second producer of plans would not be.
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
    /// Two arms bucket by columns of different Arrow types, so no period of one could equal a period of
    /// the other.
    #[error("the first arm's bucket column is {first} and another arm's is {other}, so no period can match")]
    BucketKindMismatch { first: String, other: String },
    /// Two arms group by columns of different Arrow types for one answer key, which no equality
    /// compares without a cast.
    ///
    /// Carries the label, which names a dimension, and the two Arrow types: never a key value.
    #[error("the first arm's key `{label}` is {first} and another arm's is {other}, so no key can match without a cast")]
    KeyKindMismatch { label: String, first: String, other: String },
    /// The leg results handed to a merge are not the plan's arms: another count, or a joined arm's
    /// legs where a local arm's one result belongs.
    #[error("the leg results of arm {arm} do not match the plan's arm")]
    ArmsMiswired { arm: usize },
    /// The relationship declares how its keys compare, and these two legs' link columns are not a
    /// pair that declaration joins.
    ///
    /// Not [`Self::LinkTypeMismatch`], which says no row can match: two text legs under a declared
    /// integer can, so it is the declaration that no longer fits the data.
    #[error(
        "the relationship declares its keys as {declared}, and a fact leg of {fact} with a lookup leg of {lookup} is not a pair that joins"
    )]
    LinkKindsNotJoined {
        declared: &'static str,
        fact: &'static str,
        lookup: &'static str,
    },
    /// A link value could not be read as the kind the relationship declares for it.
    ///
    /// Carries the two kinds and never the value: Arrow's own message names it, and a join key is
    /// exactly the caller data this workspace keeps out of a message. The whole answer is refused
    /// rather than the row dropped, because a dropped row is a number that looks right.
    #[error("a {side} leg's link value could not be read as {to}, which the relationship declares; the column holds {from}")]
    LinkValueNotCast {
        side: &'static str,
        from: &'static str,
        to: &'static str,
    },
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
    /// **What it is worth TODAY, stated next to the claim rather than after it.** The crate is a
    /// dependency of this one, and [`SourceProvider`] is the provider a cut would use. But no code
    /// runs the crate's optimizer rule, and the provider has no optimizer of its own
    /// (`docs/adr/0039`, fourth amendment). So nothing in this process compares two contexts yet.
    /// What the method buys now is that the seam is the one a provider plugs into, and that the
    /// value reaching it is a digest rather than a subject: the context is interpolated into plan
    /// text by that crate, so a raw subject there would be a person's identifier in `EXPLAIN`
    /// output. The type is what makes that unrepresentable - `ComputeContext::of` takes a `Subject`
    /// and there is no other constructor.
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
        let context = bounded_session(working_set)?;
        let logical = arm_plan(plan, &legs, &context, ceiling_bytes, working_set).await?;
        executed(&context, logical, answer_labels(plan), 1, ceiling_bytes, working_set).await
    }
}

/// `logical` run in `context`, its columns checked against `labels` and its last `measures` columns
/// checked finite: the tail every combine shares, whether it joined two legs or several arms.
async fn executed(
    context: &SessionContext,
    logical: LogicalPlan,
    labels: Vec<String>,
    measures: usize,
    ceiling_bytes: u64,
    working_set: WorkingSet,
) -> Result<ResultBatches, CombineError> {
    let frame = context
        .execute_logical_plan(logical)
        .await
        .map_err(|cause| CombineError::Analyze { cause })?;
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
    refuse_non_finite(&answered, measures)?;
    Ok(answered)
}

/// One joined arm's logical plan over `context`: its legs checked, registered under the fixed leg
/// names and probed for an ambiguous link, then cut.
///
/// The plan owns the tables it reads, so a later arm may register its legs under the same names in the
/// same session without disturbing it.
async fn arm_plan(
    plan: &FederatedPlan,
    legs: &Legs<'_>,
    context: &SessionContext,
    ceiling_bytes: u64,
    working_set: WorkingSet,
) -> Result<LogicalPlan, CombineError> {
    let fact = LegSchema::of(FACT, legs.fact().schema())?;
    let lookups = legs
        .lookups()
        .iter()
        .zip(LOOKUP_TABLES)
        .map(|(&(leg, batches), table)| Ok((leg, table, batches, LegSchema::of(table, batches.schema())?)))
        .collect::<Result<Vec<_>, CombineError>>()?;
    // Each lookup leg's link kind agrees with the leg it joins onto, on its own link label.
    for (position, &(leg, _, _, ref schema)) in lookups.iter().enumerate() {
        let link = InternalLabel::Link(position).label();
        let onto = match leg.onto() {
            LegAt::Fact => &fact,
            LegAt::Lookup(earlier) => &lookups.get(earlier).ok_or_else(|| missing_lookup(&link))?.3,
        };
        // A relationship's declaration is the plan's one, and it names the first link.
        let declared = plan.link_comparison().filter(|_| position == 0);
        agreeing_link(declared, onto.link_kind(&link)?, schema.link_kind(&link)?)?;
    }
    let link = InternalLabel::Link(0).label();
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
        agreeing_link(None, fact.link_kind(&link)?, second.link_kind(&link)?)?;
    }
    let casts = match lookups.first() {
        Some((_, _, _, lookup)) => LinkCasts::of(plan.link_comparison(), &link, &fact, lookup, second_fact.as_ref())?,
        None => LinkCasts::default(),
    };
    let leaves = leaf_labels(plan.federation(), &fact, second_fact.as_ref())?;
    fact.projects(plan.bucket_label())?;
    if let Some(second) = &second_fact {
        second.projects(plan.bucket_label())?;
    }
    for key in plan.keys() {
        match key.side() {
            LegAt::Fact => fact.projects(key.label())?,
            LegAt::Lookup(position) => lookups
                .get(position)
                .ok_or_else(|| missing_lookup(key.label()))?
                .3
                .projects(key.label())?,
        }
    }

    register_link(context, FACT_TABLE, FACT, legs.fact(), &link, casts.fact.as_ref())?;
    for (position, &(_, table, batches, _)) in lookups.iter().enumerate() {
        let own = InternalLabel::Link(position).label();
        register_link(
            context,
            table,
            table,
            batches,
            &own,
            casts.lookup.as_ref().filter(|_| position == 0),
        )?;
    }
    if let (Some(_), Some(batches)) = (&second_fact, legs.second_fact()) {
        register_link(
            context,
            SECOND_FACT_TABLE,
            SECOND_FACT,
            batches,
            &link,
            casts.second_fact.as_ref(),
        )?;
    }

    // Each lookup leg on its own link, a leg that joins onto an earlier lookup leg among them.
    for (position, &(_, table, _, _)) in lookups.iter().enumerate() {
        let own = InternalLabel::Link(position).label();
        refuse_ambiguous_link(context, table, &[&own], ceiling_bytes, working_set).await?;
    }
    // The second fact is joined on the link AND the bucket, so a second row for one pair fans
    // every first-fact row under it out exactly as a second lookup row would.
    if second_fact.is_some() {
        let on = [link.as_str(), plan.bucket_label()];
        refuse_ambiguous_link(context, SECOND_FACT_TABLE, &on, ceiling_bytes, working_set).await?;
        // A cast can make two first-fact groups one key, and each would meet the second fact's row.
        if casts.fact.is_some() {
            refuse_ambiguous_link(context, FACT_TABLE, &on, ceiling_bytes, working_set).await?;
        }
    }

    let second_table = second_fact.as_ref().map(|_| SECOND_FACT_TABLE);
    combine_plan(plan, context, &link, &leaves, second_table).await
}

/// A session whose operators reserve against `working_set`: the one pool a question's combine has.
fn bounded_session(working_set: WorkingSet) -> Result<SessionContext, CombineError> {
    let environment = pool::environment(working_set).map_err(|cause| CombineError::Environment { cause })?;
    Ok(SessionContext::new_with_config_rt(
        SessionConfig::new(),
        environment.into_runtime(),
    ))
}

/// The pool a ceiling of `bytes` sizes.
///
/// A ceiling of zero, or one wider than this target's pointers, is not a pool this engine can
/// install. Clamped to one byte rather than defaulted to unbounded: an unbounded pool under
/// `panic = "abort"` makes a wide join process death for every caller in flight, and a one-byte pool
/// refuses the first reservation - which is a refusal the caller can read.
fn working_set_of(bytes: u64) -> WorkingSet {
    let clamped = usize::try_from(bytes).unwrap_or(usize::MAX);
    WorkingSet::of_bytes(core::num::NonZeroUsize::new(clamped).unwrap_or(core::num::NonZeroUsize::MIN))
}

impl FederationCombiner for DataFusionCombiner {
    type Error = CombineError;

    fn combine(&self, plan: &FederatedPlan, legs: Legs<'_>, working_set_bytes: u64) -> Result<ResultBatches, Self::Error> {
        let working_set = working_set_of(working_set_bytes);
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
            CombineError::BucketKindMismatch { .. } => Some(FederatedAnswerRefusal::BucketKindMismatch),
            CombineError::KeyKindMismatch { .. } => Some(FederatedAnswerRefusal::KeyKindMismatch),
            CombineError::LinkKindsNotJoined { .. } => Some(FederatedAnswerRefusal::LinkKindsNotJoined),
            CombineError::LinkValueNotCast { .. } => Some(FederatedAnswerRefusal::LinkValueNotCast),
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
            | CombineError::ArmsMiswired { .. }
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
    register_parts(context, table, side, batches.schema(), batches.batches().to_vec())
}

/// [`register`], with the leg's link column `label` cast first when its relationship declares one.
fn register_link(
    context: &SessionContext,
    table: &str,
    side: &'static str,
    batches: &ResultBatches,
    label: &str,
    cast: Option<&Cast>,
) -> Result<(), CombineError> {
    match cast {
        None => register(context, table, side, batches),
        Some(cast) => {
            let (schema, cast) = cast.apply(side, batches, label)?;
            register_parts(context, table, side, &schema, cast)
        }
    }
}

/// `batches` under `schema`, registered as `table`.
fn register_parts(
    context: &SessionContext,
    table: &str,
    side: &'static str,
    schema: &SchemaRef,
    batches: Vec<RecordBatch>,
) -> Result<(), CombineError> {
    let memory = MemTable::try_new(Arc::clone(schema), vec![batches]).map_err(|cause| CombineError::Register { side, cause })?;
    // A merge registers each arm's legs under the same names in turn; the plan an earlier arm built
    // holds its own table, so the name is free again. Nothing is registered under it on the two-leg
    // path, where this finds nothing to remove.
    context
        .deregister_table(TableReference::bare(table))
        .map_err(|cause| CombineError::Register { side, cause })?;
    context
        .register_table(TableReference::bare(table), Arc::new(memory))
        .map(|_replaced| ())
        .map_err(|cause| CombineError::Register { side, cause })
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
/// **One pass over the measure columns, and only the ones that are floats.** The measures are the
/// answer's last `measures` columns by construction - [`answer_labels`] states the order, and a
/// drill-across answer carries one per arm - and only a `Float64` measure can be non-finite: an
/// exact decimal cannot represent one.
///
/// **Why the combiner refuses this rather than letting the presentation edge do it.**
/// `ResultBatches::to_rows` already refuses a non-finite double, so the answer would be refused
/// either way - but as `sutura_app::ServiceError::Unreadable`, which reaches a caller as a
/// data-system failure. It is neither: a definition that declared `fails` for a zero denominator
/// asked for exactly this, and a caller told to retry it retries forever. So the refusal is taken
/// here, where it can be classified as
/// [`FederatedAnswerRefusal::NonFinite`](sutura_domain::plan::FederatedAnswerRefusal::NonFinite).
fn refuse_non_finite(answered: &ResultBatches, measures: usize) -> Result<(), CombineError> {
    let width = answered.schema().fields().len();
    for measure in width.saturating_sub(measures)..width {
        let Some(field) = answered.schema().fields().get(measure) else {
            continue;
        };
        if *field.data_type() != DataType::Float64 {
            continue;
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
    }
    Ok(())
}

#[cfg(test)]
mod tests;
