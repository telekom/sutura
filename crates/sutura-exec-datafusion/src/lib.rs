#![forbid(unsafe_code)]
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
//! **The engine's memory is bounded, and the bound is not this process's memory.** Every session here
//! is built with a `RuntimeEnv` carrying a fixed-size pool, because the alternative is the engine's
//! unbounded one - and under `panic = "abort"` a large enough hash join is then process death for
//! every concurrent caller rather than an error for the one who asked. A refused reservation leaves as
//! `RefusalReason::ResourcesExhausted`. What the pool counts is operator reservations and **nothing
//! else**: not what a driver buffers, not `collect()` materialising every batch, not the row set built
//! in the conversion loop below. See [`pool`], which states the gap rather than implying it is closed.
//!
//! `translate`, `collect`, `fixture` and `pool` own narrow seams; this owns session and execution.
//!
//! **No production gauge reads the `DataFusion` pool.** Measurement-only children can opt into a
//! separate recorder; ordinary adapter construction exports no live reservation reading. The
//! `check-guidance` absence rule rejects a production `.memory_pool()` call.
use datafusion::logical_expr::{Expr, LogicalPlan, LogicalPlanBuilder};
use datafusion::prelude::{SessionConfig, SessionContext};
use sutura_domain::identity::{Presented, PresentedDisagreesWithPosture};
use sutura_domain::model::{QualifiedTable, SourceName};
use sutura_domain::plan::{AnchorPlan, Executable, LegPlan, QueryPlan};
use sutura_domain::source::{ImpersonationCapability, SourcePosture};
use sutura_domain::warehouse::cardinality::{CountsNotRead, DeclaredKey, KeyUniqueness};
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::{AnchorRows, ResultBatches, UnannouncedBatch, Warehouse};

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
    /// A path's outermost extension spells a compression codec this build cannot read.
    ///
    /// **A parse rather than a fallback, and the failure it replaces is silent.** Handing a
    /// compressed file to the engine as plain text does not fail: the schema is inferred from the
    /// codec's own header bytes, and the table resolves to columns nobody declared. So an extension
    /// that certainly names a codec and is not one this build compiled is refused by name.
    ///
    /// `crate::attach`'s `NEAR_MISSES` carries why this is a closed list rather than "anything
    /// unrecognised": `orders.txt` is a CSV and must keep reading.
    #[error("{path} names the compression suffix `{suffix}`, which this build does not read")]
    UnknownCodec { path: String, suffix: String },
    /// A path's extension names no format this engine reads.
    ///
    /// Raised by `attach_file`'s dispatch and by nothing else, so a composition root that offered a
    /// candidate name outside `crate::attach::candidates` gets a refusal rather than a CSV read of
    /// a file that is not one.
    #[error("{path} names the format `{extension}`, and this engine reads Parquet, CSV and NDJSON")]
    UnknownFormat { path: String, extension: String },
    #[error("could not register {path} as table {table}")]
    Attach {
        table: String,
        path: String,
        #[source]
        cause: datafusion::error::DataFusionError,
    },
    /// A model names a table this engine has nowhere to look for.
    ///
    /// **This engine registers one file per model in its own table registry - there is no catalog and
    /// no schema above it - so a `dataset.table` or a `project.dataset.table` path names nothing it
    /// holds.** Refused by name rather than by dropping the qualifier and reading the table of that
    /// name from the registry, which is the wrong-number failure issue #83 reports: a plausible answer
    /// under a certified metric, off a table nobody asked for.
    ///
    /// A typed error and not a `RefusalReason`, because no question a caller could ask produces one:
    /// a table path comes from a catalog document. `sutura serve` refuses the same thing at BOOT, so a
    /// deployment reaches this only if a model arrived after the engine was opened.
    ///
    /// `sutura_sql::Dialect::qualification` declares the same limit for the rendering side, where
    /// `DuckDb` is `TableOnly` for exactly this reason.
    #[error("model table {table} is qualified, and this engine registers one file per model with nothing above it")]
    QualifiedTableUnreachable { table: String },
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
    /// A result column could not be read as a domain value.
    ///
    /// **Wrapped rather than restated, and that is `docs/adr/0039`'s point.** The mapping from an
    /// Arrow array to a `Value` is `sutura_domain::warehouse::arrow`'s, shared with every adapter
    /// whose driver speaks Arrow, so the five variants this replaces - an unmapped type, a failed
    /// downcast, a non-finite double, a day count that is not a date, a result that is not
    /// rectangular - are one cause with one set of messages instead of one copy per adapter.
    ///
    /// The `#[source]` chain is what keeps the detail reachable: `sutura-app`'s bounds suite matches
    /// `UnreadableCell::NotFinite`'s own column through it, which is what
    /// `zero_denominator: fails` actually produces.
    #[error("a result column could not be read as a domain value")]
    Unreadable {
        #[source]
        cause: sutura_domain::warehouse::UnreadableCell,
    },
    /// A result batch did not carry the fields the schema it arrived under announced.
    ///
    /// Unreachable through this engine - it produces its own batches from its own plan - and kept
    /// because the check is the shared one: `Accumulating` is the same guard a foreign ADBC driver's
    /// stream goes through, and one path through it is what stops the engine's own collection being
    /// the lenient copy.
    #[error("a result batch did not match the schema it was announced under")]
    Unannounced {
        #[source]
        cause: sutura_domain::warehouse::UnannouncedBatch,
    },
    /// The result schema is not the one the plan's labels describe.
    ///
    /// Checked rather than papered over. `QueryPlan::result_labels` is the one definition both
    /// adapters build from, so a disagreement here means the projection is not what we think it is,
    /// and answering from it would return a number from a column nobody chose.
    #[error("the result columns are {actual:?}, and the plan's labels are {expected:?}")]
    SchemaMismatch { expected: Vec<String>, actual: Vec<String> },
    /// A key probe's result was not the pair of counts its aggregate projects.
    ///
    /// A defect in this crate's aliasing or in its value mapping rather than anything about the
    /// data - two aggregates over no group produce one row of two integers - and it travels as an
    /// `Err` from the port, which the boot path reads as *this declaration went unchecked*.
    #[error("the key probe did not come back as two counts")]
    KeyCounts {
        #[source]
        cause: CountsNotRead,
    },
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
    /// The deadline ran out before a call or around the whole `rows` future. Dropping that future
    /// requests abort of spawned asynchronous tasks; `DataFusion` wraps non-cooperative plan leaves
    /// so they yield. Already-running blocking work cannot be aborted and may outlive this error.
    #[error("the deadline ran out with a budget of {budget:?}")]
    DeadlineExceeded { budget: std::time::Duration },
}

/// A plan becomes expressions here. The half of this adapter that never reads a result.
mod translate;

/// A result becomes domain rows here. The mirror half, which never reads a plan except for its
/// labels.
mod collect;

/// A file becomes a table here: which formats this engine reads, and the codec parse that decides
/// whether a `.csv.gz` is text or a refusal. `docs/adr/0039` is the record.
mod attach;
pub use crate::attach::{Codec, candidates};
#[cfg(feature = "fixtures")]
mod fixture;

/// One LEG becomes expressions here, which is `translate`'s sibling rather than a part of it: what
/// differs from a whole plan is the SHAPE of the plan, not how a piece of one renders.
mod leg;

/// `top`'s own sort-and-limit, because this adapter renders no SQL for `sutura_sql::generate` to
/// share it through.
mod top;

/// Opt-in peak recording for measurement-only children, excluded from default builds.
#[cfg(feature = "measurement")]
pub mod measurement;

/// The working-set ceiling.
///
/// The pool, the never-spill policy, and how a refused reservation is recognised. Its own file
/// because it is a third seam, and because `lib.rs` is at the length gate.
pub mod pool;

pub use crate::pool::WorkingSet;

use crate::collect::outputs;
use crate::translate::{bucket_expression, column, key_counts, measure_expression, predicate, table_reference};

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
    /// `enable_time()` on both constructors now, and still no `enable_all` - so still no I/O driver, which a plan over a
    /// local file does not need. Disabled, `tokio::time::timeout` panics with "there is no timer running" - `docs/adr/0029`.
    ///
    /// *Not bounded here: this runtime has its own blocking pool, at `tokio`'s default of 512
    /// threads. Nothing in this crate sizes it, and the transport's admission bound does not reach
    /// it.*
    ///
    /// **An `Option`, and the reason is the `Drop` below.** A nested `tokio` runtime can only be
    /// dropped where blocking is allowed - dropping one inside an async context panics, and this
    /// workspace builds with `panic = "abort"`. The adapter is owned by callers that may release it
    /// on an async worker thread while it is idle (the agent surface does), so the runtime is taken
    /// out of this field and shut down by the `Drop` rather than dropped in place. Construction
    /// always sets it, and [`Self::runtime`] is the only reader - `None` is reachable only during
    /// `Drop`, which no caller reaches.
    runtime: Option<tokio::runtime::Runtime>,
    /// The configured pool ceiling.
    ///
    /// Kept alongside the session because the configured number is always knowable and is what a
    /// bounded-refusal carries.
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

impl Drop for DataFusionWarehouse {
    /// A warehouse is safe to drop where its runtime alone is not.
    ///
    /// Tokio's `Runtime::drop` panics when called in a context where blocking is not allowed, which
    /// is exactly where an agent surface is torn down: the peer's session ends on a worker thread of
    /// the CALLER's runtime, and the last handle to this adapter can be that task. Under this
    /// workspace's `panic = "abort"` that is process death, so the adapter cannot hand the runtime
    /// out raw. `shutdown_background` is tokio's documented way to drop a runtime from inside an
    /// async context: it stops the workers and returns at once, abandoning no work this adapter
    /// keeps outstanding - a question answers synchronously under `block_on`, so by the time the
    /// adapter is dropped the runtime is idle and whatever it had in flight has completed.
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
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
            .enable_time()
            .build()
            .map_err(|cause| DataFusionError::Runtime { cause })?;
        let environment = pool::environment(working_set)?;
        Ok(Self::from_bounded(
            source,
            posture,
            working_set,
            SessionConfig::new(),
            environment,
            runtime,
        ))
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
            .enable_time()
            .build()
            .map_err(|cause| DataFusionError::Runtime { cause })?;
        let environment = pool::environment(working_set)?;
        Ok(Self::from_bounded(
            source,
            posture,
            working_set,
            // The partition count follows the width. `width_tests.rs` asserts both halves.
            SessionConfig::new().with_target_partitions(workers.get()),
            environment,
            runtime,
        ))
    }

    /// Builds the session from a bounded environment minted by [`pool`].
    ///
    /// The environment's field is private to that module, so even this crate cannot hand an
    /// arbitrary `RuntimeEnv` to `SessionContext`. The measurement feature supplies a recorder
    /// wrapped around the same greedy ceiling; it does not introduce an unbounded construction path.
    pub(crate) fn from_bounded(
        source: SourceName,
        posture: SourcePosture,
        working_set: WorkingSet,
        config: SessionConfig,
        environment: pool::Bounded,
        runtime: tokio::runtime::Runtime,
    ) -> Self {
        Self {
            source,
            posture,
            context: SessionContext::new_with_config_rt(config, environment.into_runtime()),
            runtime: Some(runtime),
            working_set,
        }
    }

    /// The tokio runtime this adapter executes on.
    ///
    /// An `Option` so the `Drop` can take it out, and the only path to `None` is during that `Drop`,
    /// which nothing reaches. Answered as an error rather than unwrapped, because this workspace
    /// allows neither `unwrap` nor `expect`; the error is the one construction itself reports, and
    /// carrying it keeps a second variant nobody can provoke out of the enum.
    fn runtime(&self) -> Result<&tokio::runtime::Runtime, DataFusionError> {
        self.runtime.as_ref().ok_or_else(|| DataFusionError::Runtime {
            cause: std::io::Error::other("the runtime was already taken out by the warehouse's drop"),
        })
    }

    /// The configured pool ceiling; `MemoryPool::memory_limit` can report `Unknown` instead.
    #[inline]
    #[must_use]
    pub const fn working_set(&self) -> WorkingSet {
        self.working_set
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
            // The SHARED decision, in `leg::dimension_join`: a LEFT join, always, matching the SQL
            // path. Both plan shapes call it, so the join kind cannot be one thing for a whole
            // answer and another for one source's share of one - `a_dimension_join_does_not_change_the_measure`
            // fails on an inner join and now fails for either. That module documents what was
            // measured before the two were shared.
            builder = leg::dimension_join(builder, right, join)?;
        }

        let mut conjuncts = Vec::with_capacity(plan.filters().len());
        for filter in plan.filters() {
            conjuncts.push(predicate(plan.params(), filter.predicate())?);
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
        let (projection, tiebreak) = outputs(builder.schema(), &labels, group_count)?;
        let (sorts, limit) = top::sort_and_limit(plan.top(), &labels, group_count, tiebreak, plan.max_rows());
        builder
            .project(projection)
            .and_then(|projected| projected.sort(sorts))
            .and_then(|sorted| sorted.limit(0, Some(limit)))
            .and_then(LogicalPlanBuilder::build)
            .map_err(|cause| DataFusionError::Build { cause })
    }

    /// One registered table, as a plan to build on.
    ///
    /// `into_unoptimized_plan` rather than the optimized one: this is an input to a builder, and
    /// optimizing a fragment that is about to be joined, filtered and aggregated is work thrown
    /// away the moment the whole plan is optimized.
    async fn scan(&self, table: &QualifiedTable) -> Result<LogicalPlan, DataFusionError> {
        // Before the lookup, and it has to be before: `TableReference::bare` of the last part would
        // find the registered file and answer about it, which is a different table from the one the
        // catalog named.
        if table.qualifier().is_some() {
            return Err(DataFusionError::QualifiedTableUnreachable {
                table: table.to_string(),
            });
        }
        let frame = self
            .context
            .table(table_reference(table.name()))
            .await
            .map_err(|cause| DataFusionError::Analyze { cause })?;
        Ok(frame.into_unoptimized_plan())
    }

    /// One leg, as a logical plan.
    ///
    /// The scans are resolved here because a table lookup needs this adapter's session, and nothing
    /// else about a leg does - so everything that turns the leg's own vocabulary into nodes is
    /// [`crate::leg::logical`], which takes no session and is synchronous. The order is the one that
    /// function documents: the leg's own table first, then one per same-source hop.
    async fn leg_plan(&self, leg: &LegPlan) -> Result<LogicalPlan, DataFusionError> {
        let from = self.scan(leg.table()).await?;
        let hops = leg::joins(leg);
        let mut joined = Vec::with_capacity(hops.len());
        for join in hops {
            joined.push((join, self.scan(join.table()).await?));
        }
        leg::logical(leg, from, joined)
    }

    /// Runs whatever was handed to the port and collects its rows.
    ///
    /// **One execution and one schema check for both plan shapes**, and the two shapes differ only
    /// in the plan that is built. [`Executable::result_labels`] is the domain's own definition of
    /// what each shape projects, so a leg cannot be read back under labels a whole answer's
    /// arithmetic derived - and the comparison below is the same one, once, rather than two copies
    /// that could drift.
    async fn rows(&self, executable: Executable<'_>) -> Result<ResultBatches, DataFusionError> {
        let logical = match executable {
            Executable::Query(plan) => self.logical_plan(plan).await?,
            Executable::Leg(leg) => self.leg_plan(leg).await?,
        };
        let frame = self
            .context
            .execute_logical_plan(logical)
            .await
            .map_err(|cause| DataFusionError::Analyze { cause })?;

        // The columns come from the frame's own schema rather than from the labels we asked for, and
        // then the two are compared. Building the result set from `result_labels` directly would
        // make a projection that came back a different shape look correct.
        let expected = executable.result_labels();
        let actual = collect::labels_of(&frame);
        if actual != expected {
            return Err(DataFusionError::SchemaMismatch { expected, actual });
        }

        drop(actual);
        collect::collected(frame, self.working_set).await
    }

    /// Counts a declared join key's values and its distinct values, in one aggregate.
    ///
    /// No filter, no group and no ordering: what a `many_to_one` promises is unconditional, so a
    /// probe that narrowed itself would answer a different question than the one the join path
    /// spends. The two aggregates carry the domain's own labels - `translate::key_counts` is where -
    /// so the field names on the batch are the ones the counts are read back under.
    async fn key_uniqueness(&self, key: &DeclaredKey<'_>) -> Result<KeyUniqueness, DataFusionError> {
        let scan = self.scan(key.table()).await?;
        let logical = LogicalPlanBuilder::from(scan)
            .aggregate(Vec::<Expr>::new(), key_counts(key))
            .and_then(LogicalPlanBuilder::build)
            .map_err(|cause| DataFusionError::Build { cause })?;
        let frame = self
            .context
            .execute_logical_plan(logical)
            .await
            .map_err(|cause| DataFusionError::Analyze { cause })?;
        let rows = collect::collected(frame, self.working_set)
            .await?
            .to_rows()
            .map_err(|cause| DataFusionError::Unreadable { cause })?;
        KeyUniqueness::read(&rows).map_err(|cause| DataFusionError::KeyCounts { cause })
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

    /// **This engine runs one source's share of a two-source answer, and it is the only adapter a
    /// release links that does.** Declared rather than defaulted, and the default it overrides is
    /// documented on the port as *a missed-optimisation default rather than a missed-security one:
    /// the cost of being wrong is a refused question, never a wrong number* - which is precisely
    /// what makes opting ONE adapter in an ordinary capability statement and opting every adapter in
    /// a change of that argument.
    ///
    /// **What the port's default asks for, and why this adapter can answer it:** the default's own
    /// condition is *a combiner above it to hand a leg's rows to*, and there is one - the combine is
    /// `sutura_domain::plan::FederatedPlan::combine`, a pure domain function that no adapter is on
    /// the path of. So the engine's role here is the data source's, not the combiner's, and
    /// `docs/adr/0007`'s *the engine is also a data source* is the sentence that permits it.
    ///
    /// **The limit, next to the claim.** This is single-player federation. Two sources are not two
    /// identities: [`Warehouse::IMPERSONATION`] above is
    /// [`ImpersonationCapability::NoPlaceForASubject`], so every leg this adapter runs runs under
    /// one operating-system identity and none of them runs as the asker. A two-source answer
    /// records both legs' postures; both are the shared one.
    const EXECUTES_LEGS: bool = true;

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

    /// Deadline-aware at cooperative yield points: `tokio::time::timeout` on a runtime built with
    /// `enable_time()` drops the rows future when the budget is spent. [`DataFusionError::DeadlineExceeded`]'s
    /// doc has the mechanism, `docs/adr/0029` the limits.
    fn execute(
        &self,
        executable: Executable<'_>,
        presented: &Presented,
        deadline: Deadline,
    ) -> Result<ResultBatches, Self::Error> {
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
        let budget = deadline.budget().duration();
        let Some(remaining) = deadline.remaining_at(std::time::Instant::now()) else {
            return Err(DataFusionError::DeadlineExceeded { budget });
        };
        // **Both shapes, one path, and the match stays exhaustive.** It would read more simply as a
        // single call now that `rows` takes the `Executable` - and that is exactly what it must not
        // be: `Executable` is the port's whole vocabulary, and the arm is what makes a third plan
        // shape a compile error in this adapter rather than something it silently ran as one of
        // these two.
        let rows = match executable {
            Executable::Query(_) | Executable::Leg(_) => self.rows(executable),
        };
        // Built INSIDE this `async move`, not handed to `block_on` directly: a `Sleep` registers on
        // CONSTRUCTION, before `block_on`'s own argument is even evaluated - measured as a panic.
        self.runtime()?
            .block_on(async move { tokio::time::timeout(remaining, rows).await })
            .map_err(|_elapsed| DataFusionError::DeadlineExceeded { budget })?
    }

    /// Re-runs an anchor's plan, under this process's own identity.
    ///
    /// The same execution path `execute` takes for a whole plan, and it takes no credential because
    /// there is none at boot - which for this adapter is not a limitation but the only truth
    /// available: one process, one operating-system identity, nowhere for a subject to arrive.
    /// [`AnchorRows`] is what keeps the result from being handed back to a caller as an answer.
    fn verify_anchor(&self, plan: AnchorPlan<'_>) -> Result<AnchorRows, Self::Error> {
        // The boot path still reads rows: an anchor is a certified scalar compared against a
        // definition, and `AnchorRows` is the type that keeps it off the answer path. So the decode
        // happens here rather than at a presentation edge this call has none of.
        self.runtime()?
            .block_on(self.rows(Executable::Query(plan.plan())))?
            .to_rows()
            .map(AnchorRows::of)
            .map_err(|cause| DataFusionError::Unreadable { cause })
    }

    /// Counts a declared join key's values and its distinct values, in this process.
    ///
    /// **Overridden rather than defaulted, and this is the adapter where it matters most:** it is
    /// what a released binary links, so without it the check would exist on nothing a deployment
    /// runs. The cost argument the missing `dry_run` makes does not apply - a probe is one aggregate
    /// over one column, not most of an answer computed twice.
    ///
    /// No credential, for [`Warehouse::verify_anchor`]'s reason: there is no caller at boot, and for
    /// this adapter that is not a limitation but the only truth available.
    fn declared_key(&self, key: DeclaredKey<'_>) -> Result<KeyUniqueness, Self::Error> {
        self.runtime()?.block_on(self.key_uniqueness(&key))
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

    /// The fourth predicate: `execute`'s own [`DataFusionError::DeadlineExceeded`] and nothing else.
    fn deadline_exceeded(&self, error: &Self::Error) -> bool {
        matches!(error, DataFusionError::DeadlineExceeded { .. })
    }

    /// `true` for the MATERIALISATION BUDGET alone, which is the one failure here that is a result
    /// not fitting.
    ///
    /// **This used to be the default, and the comment that took it said there was no size a reply
    /// had to fit.** That was true of a reply and false of this process: `collected` holds the
    /// engine's own batches and `ResultBatches::to_rows` copies them again, and round 7 of
    /// `telekom/sutura#929`'s review measured that neither was bounded. Now
    /// `WorkingSet::result_budget` bounds both, and a result refused for crossing it is exactly
    /// *the result did not fit* - a governance outcome the caller cannot retry past, rather than an
    /// outage. Leaving the default would reach that caller as a `503` inviting a retry that spends
    /// the same budget in the same place.
    ///
    /// Every other [`UnannouncedBatch`](sutura_domain::warehouse::UnannouncedBatch) is `false`,
    /// exhaustively and by NAME, for the reason the `BigQuery` transport's twin gives: a mislabelled
    /// or mis-width batch is the engine disagreeing with its own announced schema, which no narrower
    /// question fixes. `OverBound` is unreachable here - `collected` passes `usize::MAX` - and is
    /// still named rather than wildcarded, so a row ceiling arriving later has to choose.
    ///
    /// **The limit, next to the claim:** this answers for the materialisation budget and not for
    /// the working-set ceiling, which keeps its own predicate and its own refusal. A caller whose
    /// operators were refused is told resources were exhausted; one whose result was refused is
    /// told the result was too large. The two are different numbers' worth of the same bytes.
    fn result_did_not_fit(&self, error: &Self::Error) -> bool {
        let DataFusionError::Unannounced { ref cause } = *error else {
            return false;
        };
        match *cause {
            UnannouncedBatch::OverBudget { .. } => true,
            UnannouncedBatch::OverBound { .. } | UnannouncedBatch::Width { .. } | UnannouncedBatch::Mislabelled { .. } => false,
        }
    }
}

/// The combiner: two legs' Arrow batches joined and re-aggregated by one `DataFusion` plan -
/// `docs/adr/0039` step 3, and `docs/adr/0007`'s second driven port arriving.
///
/// Its own module rather than part of the `Warehouse` impl, and `docs/adr/0007` is the sentence:
/// *`sutura-exec-datafusion` keeps its `Warehouse` impl for local files, because the engine is also
/// a data source, and the combiner is a separate implementor of a separate port.* One crate, two
/// ports, no shared state between them - the combiner holds no session, because its ceiling is a
/// per-question argument.
mod combine;

pub use crate::combine::{CombineError, DataFusionCombiner};

/// How wide the engine runs, in its own file for the same reason.
#[cfg(test)]
mod width_tests;

/// Fixtures shared by this crate's own tests: the posture, the leg credential and the deadline.
#[cfg(test)]
mod test_fixtures;
#[cfg(test)]
pub(crate) use test_fixtures::{test_deadline, test_leg, test_posture};

/// The plan every engine-execution question drives, and its helpers - one definition shared by
/// `width_tests.rs` and `pool/ceiling_tests.rs`, where a second copy drifted into a byte-identical
/// clone and tripped the copy/paste gate. Inline (not a `mod`) so the causality gate holds these
/// as test-only items rather than a test module that names no test.
#[cfg(test)]
use sutura_domain::calendar::{Date, TimeRange};
#[cfg(test)]
use sutura_domain::model::{Aggregate, ColumnName, DimensionName, Grain, MetricName, TableName};
#[cfg(test)]
use sutura_domain::plan::{
    PlanBindings, PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin,
    ResultLabel, StatementTables,
};
#[cfg(test)]
use sutura_domain::warehouse::ParamValue;

/// A result as domain rows, for this crate's own suite.
///
/// The port's currency is Arrow since `docs/adr/0039` step 2, and every assertion in this crate is
/// over domain values - so the decode happens here once rather than at forty call sites. The
/// `expect` is what `clippy.toml`'s `allow-expect-in-tests` permits: a result this engine built from
/// a plan this crate wrote carries only types the domain maps, and a cell that did not decode is the
/// failure the assertion would have reported anyway.
#[cfg(test)]
pub(crate) fn decoded(result: &ResultBatches) -> sutura_domain::warehouse::RowSet {
    result.to_rows().expect("the engine's own result decodes")
}

#[cfg(test)]
pub(crate) fn day(iso: &str) -> Date {
    Date::parse(iso).expect("a test date is a date")
}

#[cfg(test)]
pub(crate) fn source() -> SourceName {
    SourceName::parse("local").expect("a test source is a source")
}

#[cfg(test)]
pub(crate) fn orders() -> TableName {
    TableName::parse("orders").expect("a test table is a table")
}

#[cfg(test)]
pub(crate) fn on(name: &str) -> PlanColumn {
    PlanColumn::new(orders(), ColumnName::parse(name).expect("a test column is a column"))
}

/// Revenue by region for one month - a grouped aggregate, which is exactly the operator that
/// matters when the memory ceiling is the thing under test, because it is the one that reserves.
#[cfg(test)]
pub(crate) fn question() -> QueryPlan {
    QueryPlan::new(
        source(),
        MetricName::parse("revenue").expect("a test metric is a metric"),
        StatementTables::only(orders()),
        PlanBucket::new(ResultLabel::bucket(), Grain::Month, on("order_date")),
        vec![PlanKey::new(
            ResultLabel::dimension(&DimensionName::parse("region").expect("a test dimension is a dimension")),
            on("region"),
        )],
        PlanMeasure::Simple {
            term: PlanTerm::Aggregate {
                aggregate: Aggregate::Sum,
                column: on("amount_cents"),
            },
        },
        ResultLabel::measure(&MetricName::parse("revenue").expect("a test metric is a metric")),
        PlanBindings::parse(
            vec![
                PlanFilter::new(
                    PredicateOrigin::Definition,
                    PlanPredicate::AtOrAfter {
                        column: on("order_date"),
                        param: 0,
                    },
                ),
                PlanFilter::new(
                    PredicateOrigin::Definition,
                    PlanPredicate::Before {
                        column: on("order_date"),
                        param: 1,
                    },
                ),
            ],
            vec![ParamValue::Date(day("2026-06-01")), ParamValue::Date(day("2026-07-01"))],
        )
        .expect("a test plan binds its two range bounds in placeholder order"),
        TimeRange::new(day("2026-06-01"), day("2026-07-01")).expect("a test range is a range"),
    )
}

/// The adapter is safe to drop where its nested runtime alone is not, pinned inline rather than in
/// a separate file: registering it from this file keeps the registration beside the `Drop` it
/// tests, so reverting the one cannot orphan the other.
///
/// Tokio's `Runtime::drop` panics when called in a context where blocking is not allowed - which is
/// a worker thread of a caller's runtime, exactly where the agent surface is torn down - and this
/// workspace builds with `panic = "abort"`, so that panic is process death. The [`Drop`] on
/// [`DataFusionWarehouse`] shuts the runtime down via `shutdown_background` instead, which is
/// tokio's documented way to drop a runtime from inside an async context.
#[cfg(test)]
mod drop_tests {
    use sutura_domain::model::SourceName;

    fn adapter() -> crate::DataFusionWarehouse {
        crate::DataFusionWarehouse::new(
            SourceName::parse("local").expect("a test source is a source"),
            crate::test_posture(),
            crate::WorkingSet::of_bytes(core::num::NonZeroUsize::new(64 * 1024 * 1024).expect("a test ceiling is positive")),
        )
        .expect("a current-thread runtime builds")
    }

    /// Dropping the adapter inside the same thread's async context must not abort.
    ///
    /// Red against the previous shape (a bare `tokio::runtime::Runtime` field) and green against
    /// the `Drop`: it pins the mechanism rather than the reasoning.
    #[test]
    fn dropping_a_warehouse_inside_an_async_context_does_not_abort() {
        let adapter = adapter();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("an outer runtime builds");
        rt.block_on(async move {
            drop(adapter);
        });
    }

    /// Releasing the adapter on a worker thread, after the engine ran on the main thread, must not
    /// abort - the shape the `mcp` command's shutdown actually takes.
    #[test]
    fn dropping_a_warehouse_on_a_worker_thread_after_the_engine_was_used_on_the_main_thread_does_not_abort() {
        let adapter = adapter();
        // Use the engine's own runtime from the main thread first, as `attach` does during the CLI's
        // `open_engine`, so the core has lived on one thread before the adapter is released elsewhere.
        adapter.runtime().expect("a test runtime is present").block_on(async {});
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .build()
            .expect("an outer runtime builds");
        rt.block_on(async move {
            let handle = tokio::spawn(async move {
                drop(adapter);
            });
            handle.await.expect("the worker task finished without panicking");
        });
    }
}

/// The adapter's own suite - attaching a file, executing a plan, and the translation helpers - in
/// its own file for the same reason.
#[cfg(test)]
mod execute_tests;
