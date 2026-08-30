//! A [`Warehouse`] adapter that executes a plan in process, generating no SQL at all.
//!
//! `DataFusion` is the reason the port takes a [`QueryPlan`] rather than a rendered statement. It
//! has no dialect: a plan becomes a `LogicalPlan` over Arrow, the engine type-checks it against the
//! registered tables and runs it in this process. Every bug class that lived in rendering is
//! therefore absent here rather than fixed - there is no identifier to quote, no alias to emit, no
//! `GROUP BY` to hand the wrong expression to, and no placeholder whose position could disagree
//! with the parameter list.
//!
//! Values are handed over as typed literals, which is the strongest form of parameterisation
//! available and not a weakening of one. See the `literal` function below.
//!
//! Three things this adapter deliberately does not offer:
//!
//! **No arbitrary SQL entry point.** Not merely absent: the `sql` feature is off, so this engine's
//! parser is not compiled into the binary. There is nothing here that turns text into a plan, which
//! is a stronger statement than not calling it.
//!
//! **No entry point that takes a statement.** The only way in is a [`QueryPlan`], which carries its
//! parameters as a typed list. A development affordance that ran something somebody typed would be
//! the shortest path around every check upstream of here.
//!
//! **No result caching.** Under row-level security a query-keyed cache is a cross-user leak, and
//! although an in-process engine over a local file has no row-level security to leak through,
//! adding a cache here would be the place the habit started.
//!
//! What it does offer is two narrow, typed attach affordances, [`DataFusionWarehouse::attach_csv`]
//! and [`DataFusionWarehouse::attach_parquet`], which is what a golden fixture needs.
//!
//! **The engine's memory is bounded, and the bound is not this process's memory.** Every session here
//! is built with a `RuntimeEnv` carrying a fixed-size pool, because the alternative is the engine's
//! unbounded one - and under `panic = "abort"` a large enough hash join is then process death for
//! every concurrent caller rather than an error for the one who asked. A refused reservation leaves as
//! `RefusalReason::ResourcesExhausted`. What the pool counts is operator reservations and **nothing
//! else**: not what a driver buffers, not `collect()` materialising every batch, not the row set built
//! in the conversion loop below. See [`pool`], which states the gap rather than implying it is closed.
//!
//! # Four files, along three seams
//!
//! `translate.rs` turns a plan into expressions and never reads a result; `collect.rs` turns a result
//! into domain rows and never reads a plan except for its labels; `pool.rs` is the working-set ceiling
//! and reads neither. What is left here is what none of them is about: the session, the runtime,
//! attaching a file, and executing.

use std::path::Path;
use std::sync::Arc;

use datafusion::common::JoinType as EngineJoin;
use datafusion::execution::memory_pool::MemoryPool;
use datafusion::logical_expr::{Expr, LogicalPlan, LogicalPlanBuilder};
use datafusion::prelude::{CsvReadOptions, ParquetReadOptions, SessionConfig, SessionContext};
use sutura_domain::identity::{Presented, PresentedDisagreesWithPosture};
use sutura_domain::model::{JoinType, SourceName, TableName};
use sutura_domain::plan::{AnchorPlan, Executable, QueryPlan};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::{AnchorRows, MalformedRowSet, RowSet, Value, Warehouse};

/// Why this data system could not answer.
///
/// One variant per failure mode rather than one wrapper around the engine's error, because "the
/// plan would not build", "the plan would not resolve" and "a column came back as a type we do not
/// map" send a reader to three different places.
#[derive(Debug, thiserror::Error)]
pub enum DataFusionError {
    /// The runtime this adapter executes on could not be built.
    ///
    /// One runtime is built per adapter and kept, so this happens once at construction or not at
    /// all. The engine is async all the way down and `futures::executor::block_on` panics at
    /// collect time with "no reactor running", so there is no runtime-free path to fall back to.
    #[error("the runtime this adapter executes on could not be built")]
    Runtime {
        #[source]
        cause: std::io::Error,
    },
    /// The execution environment - the bounded memory pool, and nowhere to spill - could not be
    /// built.
    ///
    /// Separate from [`Self::Runtime`], which is the *tokio* runtime: one is a thread pool and one is
    /// the memory bound, and a deployment that cannot start needs to know which. Like `Runtime` it
    /// happens once at construction or not at all.
    #[error("the bounded execution environment this adapter reserves against could not be built")]
    Environment {
        #[source]
        cause: datafusion::error::DataFusionError,
    },
    #[error("could not register {path} as table {table}")]
    Attach {
        table: String,
        path: String,
        #[source]
        cause: datafusion::error::DataFusionError,
    },
    /// A logical plan could not be assembled from the query plan.
    ///
    /// A bug here or upstream rather than a refusal: a caller cannot ask anything that causes one.
    #[error("the plan could not be assembled as a logical plan")]
    Build {
        #[source]
        cause: datafusion::error::DataFusionError,
    },
    /// The engine refused the plan: an unknown table, an unknown column, a type mismatch.
    #[error("the engine did not accept the plan")]
    Analyze {
        #[source]
        cause: datafusion::error::DataFusionError,
    },
    #[error("the plan failed while running")]
    Execute {
        #[source]
        cause: datafusion::error::DataFusionError,
    },
    /// A column came back as a type this adapter does not map.
    ///
    /// An error rather than a stringified fallback, for the reason the `DuckDB` adapter gives: a
    /// nested type rendered with `Debug` would flow into an answer looking like data, and an anchor
    /// comparison against it would pass or fail for reasons nobody could read.
    #[error("column {column} came back as {arrow_type}, which this adapter does not map")]
    UnsupportedType { column: String, arrow_type: String },
    /// The schema said one Arrow type and the array was another.
    ///
    /// Unreachable through the engine, and reported rather than skipped anyway: the alternative is
    /// substituting a null for a value that exists.
    #[error("column {column} did not downcast to {arrow_type}, though its schema says that is its type")]
    Downcast { column: String, arrow_type: &'static str },
    /// A floating-point column came back as a value that is not a number.
    ///
    /// **What `zero_denominator: fails` actually produces.** A ratio measure choosing that word is
    /// translated as an unguarded division with the numerator cast to `Float64`, so the division is
    /// IEEE float division: dividing by zero answers `inf` here rather than failing, and zero divided
    /// by zero answers `NaN`. `Real` refuses all three, which is what makes the word `fails` true of
    /// the metric that chose it instead of the string `inf` arriving under a certified name.
    ///
    /// The cause names which of the three it was; this variant names the column.
    #[error("column {column} came back as a value that is not a finite number")]
    NotFinite {
        column: String,
        #[source]
        cause: sutura_domain::warehouse::NotFinite,
    },
    /// A day number came back that is not a date this build can represent.
    #[error("column {column} came back as a day number that is not a date")]
    NotADate {
        column: String,
        #[source]
        cause: sutura_domain::calendar::InvalidDate,
    },
    #[error("the result set was not rectangular")]
    Shape {
        #[source]
        cause: MalformedRowSet,
    },
    /// The result schema is not the one the plan's labels describe.
    ///
    /// Checked rather than papered over. `QueryPlan::result_labels` is the one definition both
    /// adapters build from, so a disagreement here means the projection is not what we think it is,
    /// and answering from it would return a number from a column nobody chose.
    #[error("the result columns are {actual:?}, and the plan's labels are {expected:?}")]
    SchemaMismatch { expected: Vec<String>, actual: Vec<String> },
    /// A predicate named a parameter index the plan does not have.
    ///
    /// Predicates are resolved by their recorded index rather than by position, so this is what a
    /// plan built with a stale index looks like instead of a silently wrong comparison.
    #[error("a predicate binds parameter {index}, and the plan carries {count}")]
    MissingParam { index: usize, count: usize },
    /// A plan with no predicate at all.
    ///
    /// Unreachable: a plan always carries the two bounds of its `TimeRange`, which cannot be
    /// unbounded. Written as a branch rather than an assertion because the SQL path refuses the
    /// same shape, and an adapter that quietly ran it unfiltered would disagree with the other one
    /// about an unbounded scan.
    #[error("a plan must carry the two bounds of its range, and this one carries no predicate")]
    NoPredicate,
    /// One leg of a federated answer, which this adapter has nothing to assemble above.
    ///
    /// **Not a refusal and not a default body.** [`Warehouse::execute`] takes an
    /// [`Executable`](sutura_domain::plan::Executable), so this adapter's match over what it can be
    /// handed is exhaustive - which is the mechanism, and this variant is what it costs today.
    /// Nothing constructs a [`LegPlan`](sutura_domain::plan::LegPlan) outside a test: there is no
    /// splitter and no combiner, so no code path reaches here. When the combiner arrives this arm is
    /// where the engine's leg path lands, and until then an error naming the leg is more honest than
    /// a silently non-federating default.
    ///
    /// It carries the table rather than a sentence, because that is the one thing a reader chasing
    /// this needs and the message may be reworded.
    #[error("this adapter executes a whole plan, and the leg against {table} needs a combiner above it")]
    LegWithoutCombiner { table: String },
    /// The credential broker handed this adapter subject material it has nowhere to put.
    ///
    /// **An `Err` and never a refusal, and the direction is the point.** Nothing about the question
    /// was wrong: it is a wiring defect between the broker and the source declaration, and offering
    /// it as a refusal would invite a client to retry a deployment bug until something works.
    /// `docs/adr/0008` part 4 states both directions and says which one is silent - an adapter that
    /// quietly *accepted* material it cannot use would report a leg as impersonated that ran shared.
    ///
    /// This adapter is one process reading local files under one operating-system identity, which is
    /// what [`Warehouse::IMPERSONATION`] declares, so the only shape it can be handed is the
    /// deployment's own identity for that source. A configuration that asked for anything else does
    /// not boot - `SourcePosture::deliverable_by` refuses it in the composition root - so reaching
    /// this arm in production means the broker ignored the declaration it reads.
    #[error(
        "source `{at}` was handed {presented}, and this adapter has nowhere for a subject's own \
         credential to arrive: it is one process reading local files under one identity. This is a \
         wiring defect between the credential broker and the source declaration"
    )]
    NoPlaceForASubject { at: String, presented: &'static str },
    /// The broker presented a leg that does not agree with how this source was DECLARED.
    ///
    /// **The check above answers a different question, and a review found the gap.**
    /// `NoPlaceForASubject` compares what arrived against what this CODE can carry - the
    /// [`Warehouse::IMPERSONATION`] constant - and says nothing about the posture the composition
    /// root handed this adapter. So a shared leg carrying a *different* operator acknowledgement
    /// matched the variant and was accepted, and provenance - which is read off `posture` - then
    /// reported this adapter's own declaration rather than the acknowledgement the broker actually
    /// presented.
    ///
    /// The comparison is a real one rather than a value against itself: the broker reads the settings
    /// tree and this adapter holds what the root handed it. An `Err` for the reason the variant above
    /// is one - a wiring defect between the broker and the source declaration, which no caller may
    /// retry into an answer. The typed cause carries which disagreement it was.
    #[error("the credential broker presented a leg that disagrees with how this source is declared")]
    PresentedDisagreesWithPosture {
        #[source]
        cause: PresentedDisagreesWithPosture,
    },
}

/// A plan becomes expressions here. The half of this adapter that never reads a result.
mod translate;

/// A result becomes domain rows here. The mirror half, which never reads a plan except for its
/// labels.
mod collect;

/// The working-set ceiling.
///
/// The pool, the never-spill policy, and how a refused reservation is recognised. Its own file
/// because it is a third seam, and because `lib.rs` is at the length gate.
pub mod pool;

pub use crate::pool::WorkingSet;

use crate::collect::{cell, outputs};
use crate::translate::{bucket_expression, column, measure_expression, predicate, table_reference};

/// An in-process engine, behind the [`Warehouse`] port.
pub struct DataFusionWarehouse {
    source: SourceName,
    /// Which identity a query reaches this source as, as the deployment declared it.
    ///
    /// **Handed over at construction and never derived here.** The adapter declares a *capability* -
    /// see [`Warehouse::IMPERSONATION`] below - and the deployment declares the *posture*; an adapter
    /// that chose its own posture would be an adapter deciding what a caller gets. It is kept so
    /// provenance can be read off the thing that executed rather than off a settings tree.
    posture: SourcePosture,
    context: SessionContext,
    /// **One runtime, built once and kept.** The engine is async from the first table lookup to the
    /// last batch collected, and a runtime built per query is a reactor created and torn down for
    /// every question.
    ///
    /// How WIDE it is depends on which constructor built it, and the difference is measured on
    /// [`DataFusionWarehouse::with_worker_threads`]. One thread is right for the command-line tool,
    /// which answers one question and exits; it is a hard ceiling on a server, because every caller
    /// `block_on`s this same runtime and a single-threaded one runs their work one at a time.
    ///
    /// Still no `enable_all` either way, so still no timer and no I/O driver - neither of which a
    /// plan over a local file needs.
    ///
    /// *Not bounded here: this runtime has its own blocking pool, at `tokio`'s default of 512
    /// threads. Nothing in this crate sizes it, and the transport's admission bound does not reach
    /// it.*
    runtime: tokio::runtime::Runtime,
    /// The pool every operator in this session reserves against, kept rather than derived.
    ///
    /// Retained for two reasons. It is what an operator watching a deployment reads - reserved bytes
    /// against the ceiling, which `docs/adr/0015` specifies and deliberately does not ship until this
    /// field exists - and reaching it back out of the session context would be a second path to the
    /// same value.
    ///
    /// See [`crate::pool`] for what it counts, which is narrower than "this process's memory".
    pool: Arc<dyn MemoryPool>,
    /// The ceiling the pool was built with.
    ///
    /// Kept alongside the pool rather than read off it, which is what `docs/adr/0015` decides and for
    /// a stated reason: `MemoryPool::memory_limit` defaults to `Unknown`, so a pool implementation
    /// that does not override it reports no ceiling and the ratio an operator wants is unavailable.
    /// The configured number is always knowable.
    working_set: WorkingSet,
}

impl core::fmt::Debug for DataFusionWarehouse {
    /// Hand-written because neither the session context nor the runtime is `Debug`, and because a
    /// session context's own `Debug` would be the sort of thing that prints every registered path
    /// into a log for no benefit.
    ///
    /// The pool is not printed either, and `finish_non_exhaustive` is what says so. `GreedyMemoryPool`
    /// is `Debug`, so it could be - but a `Debug` of a warehouse is a thing that reaches a log by
    /// accident, and what it would carry is a live reservation figure: a number about the shape of
    /// whatever question is in flight. [`Self::working_set`] is the accessor for the configured
    /// ceiling, which is the half that is a configuration fact rather than an observation.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DataFusionWarehouse")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl DataFusionWarehouse {
    /// Builds an adapter with nothing registered.
    ///
    /// There is no file to open, which is the difference from the `DuckDB` adapter: the engine is
    /// this process, and a table exists once it has been attached.
    ///
    /// **It takes a ceiling, and that is not optional.** `SessionContext::new()` installs the
    /// engine's unbounded pool, which under `panic = "abort"` makes a large enough join process death
    /// rather than a refusal - so a constructor that let a caller skip the bound would be the one
    /// place the whole control could be forgotten. `sutura_config::WorkingSetCeiling::DEFAULT_BYTES`
    /// is what a caller with no settings to read uses.
    /// **It also takes the posture, and that is not optional either**, for the reason the ceiling is
    /// not: a defaulted posture would be a claim about who a query runs as that nobody made.
    pub fn new(source: SourceName, posture: SourcePosture, working_set: WorkingSet) -> Result<Self, DataFusionError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .map_err(|cause| DataFusionError::Runtime { cause })?;
        let (environment, pool) = pool::environment(working_set)?;
        Ok(Self {
            source,
            posture,
            // `new_with_config_rt` rather than `new`, which is the whole of the bound: `new` installs
            // an `UnboundedMemoryPool`. The config half is the engine's own default here, because a
            // current-thread runtime has no width to pin - see `with_worker_threads` for the site
            // where it does.
            context: SessionContext::new_with_config_rt(SessionConfig::new(), environment),
            runtime,
            pool,
            working_set,
        })
    }

    /// The same adapter, `workers` threads wide.
    ///
    /// **What [`Self::new`] costs when several threads call it at once, measured rather than
    /// assumed.** Twenty questions per caller over a million rows, in the `dev` profile - where
    /// every dependency is already at `opt-level = 3`, so the aggregation kernels are the shipped
    /// ones - on a sixteen-way host. Throughput, normalised to one caller on the current-thread
    /// runtime:
    ///
    /// | callers | `new` | `with_worker_threads(callers)` | one `new` per caller |
    /// | --- | --- | --- | --- |
    /// | 1 | 1.00x | 1.07x | 0.98x |
    /// | 2 | 1.02x | 2.08x | 1.82x |
    /// | 4 | 1.03x | 3.94x | 3.14x |
    /// | 8 | 1.01x | 6.06x | 4.87x |
    ///
    /// The first column is the finding: it is *flat*. A shared current-thread runtime does not scale
    /// with callers at all on this workload, because every `block_on` drives the same single-threaded
    /// core and the work is inside it. The third column is the other candidate - a runtime per
    /// calling thread - and it is consistently worse than one wide runtime while also needing
    /// per-thread state, so it was not taken.
    ///
    /// **The partition count follows the width**, which a bare `worker_threads` call would not do.
    /// `DataFusion` defaults `target_partitions` to `available_parallelism` - the number this key
    /// exists to override, since a CPU quota does not change it - so a two-worker runtime would
    /// otherwise build sixteen-way plans and execute them two at a time. Pinning it is why the small
    /// widths above are *ahead* of the baseline rather than level with it.
    ///
    /// [`Self::new`] is deliberately left alone: the command-line tool answers one question and
    /// exits, and it is also the caller with no settings to read a width from.
    pub fn with_worker_threads(
        source: SourceName,
        posture: SourcePosture,
        workers: core::num::NonZeroUsize,
        working_set: WorkingSet,
    ) -> Result<Self, DataFusionError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(workers.get())
            .thread_name("sutura-engine")
            .build()
            .map_err(|cause| DataFusionError::Runtime { cause })?;
        let (environment, pool) = pool::environment(working_set)?;
        Ok(Self {
            source,
            posture,
            // `new_with_config_rt` and NOT `new_with_config`: the second takes the default
            // environment, which carries the engine's unbounded pool. **`with_target_partitions` is
            // untouched** - `width_tests.rs` asserts both halves of this line, and a partition count
            // that stops following the width silently builds sixteen-way plans on a two-worker
            // runtime.
            context: SessionContext::new_with_config_rt(SessionConfig::new().with_target_partitions(workers.get()), environment),
            runtime,
            pool,
            working_set,
        })
    }

    /// The pool every operator in this session reserves against.
    ///
    /// **An accessor because the fields are private and stay private**, and because
    /// `docs/adr/0015` needs `MemoryPool::reserved` for a gauge whose absence it currently specifies:
    /// a gauge reading zero while no pool exists is a lie an operator builds an alert on.
    ///
    /// **State the limit with the reading.** What comes back counts operator reservations - a
    /// hash-join build side, aggregate state, a sort - and nothing else. It is not this process's
    /// memory, and it must not be alerted on as though it were: `collect()` materialising every batch
    /// and the row set built during conversion are both outside it, on the same request path.
    #[inline]
    #[must_use]
    pub fn memory_pool(&self) -> &Arc<dyn MemoryPool> {
        &self.pool
    }

    /// The ceiling this adapter's pool was built with.
    ///
    /// From the configured value rather than from `MemoryPool::memory_limit`, which defaults to
    /// `Unknown`: a pool that does not override it reports no ceiling, and then the reserved-against-
    /// ceiling ratio an operator actually wants cannot be computed.
    #[inline]
    #[must_use]
    pub const fn working_set(&self) -> WorkingSet {
        self.working_set
    }

    /// Exposes a CSV file as a table.
    ///
    /// A narrow, typed affordance instead of a general "run this" method, which is what a local
    /// adapter usually grows and what would make every check upstream of here optional. Nothing is
    /// escaped and nothing is quoted, because nothing is rendered: the name and the path are
    /// arguments to a registration call, so a path with a quote in it is a path. `has_header` is the
    /// read options' default.
    pub fn attach_csv(&self, table: &TableName, path: &Path) -> Result<(), DataFusionError> {
        let located = path.display().to_string();
        self.runtime
            .block_on(
                self.context
                    .register_csv(table_reference(table), located.as_str(), CsvReadOptions::new()),
            )
            .map_err(|cause| DataFusionError::Attach {
                table: String::from(table.as_str()),
                path: located,
                cause,
            })
    }

    /// Exposes a Parquet file as a table.
    ///
    /// The CSV affordance's twin, and why the `parquet` feature is on. Neither `compression` nor
    /// `avro` is: each reintroduces a licence the supply-chain gate does not allow, and the
    /// manifest's comment records which.
    pub fn attach_parquet(&self, table: &TableName, path: &Path) -> Result<(), DataFusionError> {
        let located = path.display().to_string();
        self.runtime
            .block_on(
                self.context
                    .register_parquet(table_reference(table), located.as_str(), ParquetReadOptions::default()),
            )
            .map_err(|cause| DataFusionError::Attach {
                table: String::from(table.as_str()),
                path: located,
                cause,
            })
    }

    /// The plan, as a logical plan.
    ///
    /// The shape the SQL path renders, built as nodes instead: scan, joins, filter, aggregate,
    /// projection, sort, limit. Nothing here is text, so nothing here can be a quoting or a
    /// placeholder bug.
    async fn logical_plan(&self, plan: &QueryPlan) -> Result<LogicalPlan, DataFusionError> {
        let scan = self.scan(plan.table()).await?;
        let mut builder = LogicalPlanBuilder::from(scan);

        for join in plan.joins() {
            let right = self.scan(join.table()).await?;
            let on = column(join.origin()).eq(column(join.target()));
            // A LEFT join, always, matching the SQL path - and there it was a bug before it was a
            // decision. An INNER join drops every fact row whose dimension row is missing, so a
            // grouped answer totals less than the ungrouped one with nothing raising an error. The
            // catalog`s duplication check cannot see it: that guard is about fan-out, not about
            // elimination. `a_dimension_join_does_not_change_the_measure` asserts the reconciliation
            // over real data, and it fails on an inner join.
            //
            // `OneToMany` never reaches here: a join that can duplicate the metric's rows is refused
            // when the definitions are assembled. Matched exhaustively anyway, so a fourth
            // cardinality is a compile error rather than a silently wrong plan.
            builder = match join.join_type() {
                JoinType::OneToOne | JoinType::ManyToOne | JoinType::OneToMany => builder
                    .join_on(right, EngineJoin::Left, [on])
                    .map_err(|cause| DataFusionError::Build { cause })?,
            };
        }

        let mut conjuncts = Vec::with_capacity(plan.filters().len());
        for filter in plan.filters() {
            conjuncts.push(predicate(plan, filter.predicate())?);
        }
        let mut remaining = conjuncts.into_iter();
        let Some(first) = remaining.next() else {
            return Err(DataFusionError::NoPredicate);
        };
        builder = builder
            .filter(remaining.fold(first, Expr::and))
            .map_err(|cause| DataFusionError::Build { cause })?;

        // Keys then the bucket, which is the order `QueryPlan::result_labels` states, so the
        // aggregate's output fields line up with the labels position for position.
        let mut grouping = Vec::with_capacity(plan.keys().len().saturating_add(1));
        for key in plan.keys() {
            grouping.push(column(key.column()));
        }
        grouping.push(bucket_expression(plan.bucket().grain(), plan.bucket().column()));
        let group_count = grouping.len();
        builder = builder
            .aggregate(grouping, vec![measure_expression(plan.measure())?])
            .map_err(|cause| DataFusionError::Build { cause })?;

        let labels = plan.result_labels();
        let (projection, ordering) = outputs(builder.schema(), &labels, group_count)?;
        builder
            .project(projection)
            .and_then(|projected| projected.sort_by(ordering))
            .and_then(|sorted| sorted.limit(0, Some(usize::try_from(plan.row_limit()).unwrap_or(usize::MAX))))
            .and_then(LogicalPlanBuilder::build)
            .map_err(|cause| DataFusionError::Build { cause })
    }

    /// One registered table, as a plan to build on.
    ///
    /// `into_unoptimized_plan` rather than the optimized one: this is an input to a builder, and
    /// optimizing a fragment that is about to be joined, filtered and aggregated is work thrown
    /// away the moment the whole plan is optimized.
    async fn scan(&self, table: &TableName) -> Result<LogicalPlan, DataFusionError> {
        let frame = self
            .context
            .table(table_reference(table))
            .await
            .map_err(|cause| DataFusionError::Analyze { cause })?;
        Ok(frame.into_unoptimized_plan())
    }

    /// Runs the plan and collects its rows.
    async fn rows(&self, plan: &QueryPlan) -> Result<RowSet, DataFusionError> {
        let logical = self.logical_plan(plan).await?;
        let frame = self
            .context
            .execute_logical_plan(logical)
            .await
            .map_err(|cause| DataFusionError::Analyze { cause })?;

        // The columns come from the frame's own schema rather than from the labels we asked for, and
        // then the two are compared. Building the result set from `result_labels` directly would
        // make a projection that came back a different shape look correct.
        let expected = plan.result_labels();
        let actual: Vec<String> = frame
            .schema()
            .fields()
            .iter()
            .map(|f| String::from(f.name().as_str()))
            .collect();
        if actual != expected {
            return Err(DataFusionError::SchemaMismatch { expected, actual });
        }

        let batches = frame.collect().await.map_err(|cause| DataFusionError::Execute { cause })?;
        let mut out: Vec<Vec<Value>> = Vec::new();
        for batch in &batches {
            for row in 0..batch.num_rows() {
                let mut cells = Vec::with_capacity(batch.num_columns());
                for (array, label) in batch.columns().iter().zip(actual.iter()) {
                    cells.push(cell(label, array.as_ref(), row)?);
                }
                out.push(cells);
            }
        }
        RowSet::new(actual, out).map_err(|cause| DataFusionError::Shape { cause })
    }
}

impl Warehouse for DataFusionWarehouse {
    type Error = DataFusionError;

    /// **This engine cannot impersonate anybody, and saying so is the point of the declaration.** It
    /// is one process reading local files under one operating-system identity, and there is no place
    /// in that path for a subject to arrive - not a connection to authenticate, not a session to
    /// switch, not a token to present. A file engine is the easiest source in the world to assume
    /// nothing about, and "nobody declared anything for the engine" is how a deployment ends up
    /// believing its whole surface impersonates because its *network* source does.
    ///
    /// The boot check reads this against the configured posture, so a source declared
    /// `impersonation-at-source` on this adapter does not start.
    const IMPERSONATION: ImpersonationCapability = ImpersonationCapability::NoPlaceForASubject;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn posture(&self) -> &SourcePosture {
        &self.posture
    }

    // No `dry_run`, and the omission is the point. This adapter is the engine, in this process:
    // checking a plan means building the logical plan and running the analyzer and the optimizer,
    // and executing it means doing all of that and then reading. So a pre-flight here is not a
    // cheaper question than the answer - it is most of the answer, run twice, once per question.
    // `Warehouse::dry_run` is defaulted for exactly this case, and taking the default is how an
    // adapter says so. Nothing is given up: every name and every argument type is still resolved
    // during analysis, so a plan naming a table that was never attached is an error out of
    // `execute` before a single row comes back - which is what the pre-flight was for.

    fn execute(&self, executable: Executable<'_>, presented: &Presented) -> Result<RowSet, Self::Error> {
        // What this leg runs as, matched exhaustively before anything is executed. There is exactly
        // one shape this adapter can honour, and the other two are a wiring defect rather than a
        // question anybody may retry - see `DataFusionError::NoPlaceForASubject`.
        match *presented {
            Presented::SharedServiceUser { .. } => {}
            Presented::SubjectToken { .. } | Presented::SubjectPrincipal { .. } => {
                return Err(DataFusionError::NoPlaceForASubject {
                    at: String::from(self.source.as_str()),
                    presented: presented.as_str(),
                });
            }
        }
        // And the second check, which is a different one: does the leg AGREE with the posture this
        // adapter was handed? The match above compares what arrived against what this code can carry
        // and reads `self.posture` not at all - so a shared leg carrying another source's
        // acknowledgement got past it. The capability check runs first deliberately: it is the one
        // whose message names what a broker did wrong, and after it the only shape left is the shared
        // one, so this call reduces to comparing the two witnesses.
        presented
            .agrees_with(&self.posture, &self.source)
            .map_err(|cause| DataFusionError::PresentedDisagreesWithPosture { cause })?;
        match executable {
            Executable::Query(plan) => self.runtime.block_on(self.rows(plan)),
            // Stated rather than defaulted. This adapter is the engine and it belongs ABOVE the
            // port once federation lands, so a leg arriving here would mean the composition is
            // wrong - not that the leg is unanswerable. Nothing reaches this today: there is no
            // splitter to build a leg.
            Executable::Leg(leg) => Err(DataFusionError::LegWithoutCombiner {
                table: String::from(leg.table().as_str()),
            }),
        }
    }

    /// Re-runs an anchor's plan, under this process's own identity.
    ///
    /// The same execution path `execute` takes for a whole plan, and it takes no credential because
    /// there is none at boot - which for this adapter is not a limitation but the only truth
    /// available: one process, one operating-system identity, nowhere for a subject to arrive.
    /// [`AnchorRows`] is what keeps the result from being handed back to a caller as an answer.
    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        self.runtime.block_on(self.rows(plan.plan())).map(AnchorRows::of)
    }

    /// The one question the domain asks about this adapter's error, answered from the one variant
    /// that means it. [`pool::refused_a_reservation`] is the exhaustive match; this is the ceiling.
    ///
    /// `u64` because the refusal is a domain value and the domain does not know how wide this
    /// target's pointers are. `try_from` cannot fail on any target this ships to; the fallback is
    /// `u64::MAX` rather than `None`, because losing the refusal would put the caller back on the
    /// `503` this whole variant exists to get them off.
    fn working_set_exhausted(&self, error: &Self::Error) -> Option<u64> {
        pool::refused_a_reservation(error).then(|| u64::try_from(self.working_set.bytes()).unwrap_or(u64::MAX))
    }
}

/// The half of the value mapping that is shared with the data source, in its own file.
///
/// Split out for the file-length gate rather than for taste: this one is already close to the
/// 1000-line limit, and the gate's answer to that is to split the file, not to shorten the fix.
#[cfg(test)]
mod value_mapping_tests;

/// How wide the engine runs, in its own file for the same reason.
#[cfg(test)]
mod width_tests;

/// The posture this crate's own tests open the engine with.
///
/// One definition shared by four test files, so a fixture cannot drift from the capability the adapter
/// declares. Shared is the honest value rather than a convenient one: one process, one
/// operating-system identity, and `IMPERSONATION` says there is nowhere for a subject to arrive.
#[cfg(test)]
pub(crate) fn test_posture() -> SourcePosture {
    SourcePosture::SharedServiceUser {
        declared: sutura_domain::source::SharedIdentityDeclared::of(
            sutura_domain::source::AcknowledgementReason::parse("one process reading local files as one identity")
                .expect("a fixture reason is a reason"),
        ),
    }
}

/// What this crate's own tests execute a leg as.
///
/// The deployment's own identity for the source, carrying the same acknowledgement
/// [`test_posture`] declares - because that is the one shape this adapter can honour, and a fixture
/// that presented anything else would be testing the refusal rather than the execution. The refusal
/// has its own test.
#[cfg(test)]
pub(crate) fn test_leg() -> Presented {
    match test_posture() {
        SourcePosture::SharedServiceUser { declared } => Presented::SharedServiceUser { declared },
        SourcePosture::ImpersonationAtSource => panic!("the fixture posture is shared, one function above"),
    }
}

/// The adapter's own suite - attaching a file, executing a plan, and the translation helpers - in
/// its own file for the same reason.
#[cfg(test)]
mod execute_tests;
