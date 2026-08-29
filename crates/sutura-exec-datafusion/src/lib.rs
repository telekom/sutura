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
use sutura_domain::model::{JoinType, SourceName, TableName};
use sutura_domain::plan::QueryPlan;
use sutura_domain::warehouse::{MalformedRowSet, RowSet, Value, Warehouse};

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
    pub fn new(source: SourceName, working_set: WorkingSet) -> Result<Self, DataFusionError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .map_err(|cause| DataFusionError::Runtime { cause })?;
        let (environment, pool) = pool::environment(working_set)?;
        Ok(Self {
            source,
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

    fn source(&self) -> &SourceName {
        &self.source
    }

    // No `dry_run`, and the omission is the point. This adapter is the engine, in this process:
    // checking a plan means building the logical plan and running the analyzer and the optimizer,
    // and executing it means doing all of that and then reading. So a pre-flight here is not a
    // cheaper question than the answer - it is most of the answer, run twice, once per question.
    // `Warehouse::dry_run` is defaulted for exactly this case, and taking the default is how an
    // adapter says so. Nothing is given up: every name and every argument type is still resolved
    // during analysis, so a plan naming a table that was never attached is an error out of
    // `execute` before a single row comes back - which is what the pre-flight was for.

    fn execute(&self, plan: &QueryPlan) -> Result<RowSet, Self::Error> {
        self.runtime.block_on(self.rows(plan))
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

#[cfg(test)]
mod tests {
    use super::collect::cell;
    use super::translate::{aggregate_expr, literal, measure_expression, unit};
    use super::{DataFusionError, DataFusionWarehouse, column};
    use datafusion::arrow::array::{ArrayRef, BooleanArray, Date32Array, Float32Array, Int64Array, StringArray};
    use datafusion::arrow::datatypes::{DataType, Field, Schema};
    use datafusion::arrow::record_batch::RecordBatch;
    use datafusion::prelude::col;
    use std::sync::Arc;
    use sutura_domain::calendar::{Date, TimeRange};
    use sutura_domain::measure::ZeroDenominator;
    use sutura_domain::model::{Aggregate, ColumnName, Grain, MetricName, SourceName, TableName};
    use sutura_domain::plan::{
        PlanBucket, PlanColumn, PlanFilter, PlanKey, PlanMeasure, PlanPredicate, PlanTerm, PredicateOrigin, QueryPlan,
    };
    // `Warehouse as _`: the trait is imported for `dry_run` and `execute`, and never named.
    use sutura_domain::warehouse::{ParamValue, Real, Value, Warehouse as _};

    fn day(iso: &str) -> Date {
        Date::parse(iso).expect("a test date is a date")
    }

    /// A ceiling no test in this module is meant to reach. The bound has its own suite in `pool.rs`.
    fn roomy() -> super::WorkingSet {
        super::WorkingSet::of_bytes(core::num::NonZeroUsize::new(64 * 1024 * 1024).expect("a test ceiling is positive"))
    }

    fn real(value: f64) -> Real {
        Real::parse(value).expect("a test literal is finite")
    }

    fn orders() -> TableName {
        TableName::parse("orders").expect("a test table is a table")
    }

    fn on(name: &str) -> PlanColumn {
        PlanColumn::new(orders(), ColumnName::parse(name).expect("a test column is a column"))
    }

    fn agg(aggregate: Aggregate, name: &str) -> PlanTerm {
        PlanTerm::Aggregate {
            aggregate,
            column: on(name),
        }
    }

    fn simple(aggregate: Aggregate, name: &str) -> PlanMeasure {
        PlanMeasure::Simple {
            term: agg(aggregate, name),
        }
    }

    /// One month of one table, grouped by region, over a bounded range.
    ///
    /// The two range bounds are bound as parameters at indices 0 and 1, which is what the SQL path
    /// would render as placeholders, so the predicate-by-index path is exercised by every test that
    /// executes anything.
    fn plan(measure: PlanMeasure, label: &str, keys: Vec<PlanKey>) -> QueryPlan {
        QueryPlan::new(
            SourceName::parse("local").expect("a test source is a source"),
            MetricName::parse("revenue").expect("a test metric is a metric"),
            orders(),
            Vec::new(),
            PlanBucket::new(String::from("period"), Grain::Month, on("order_date")),
            keys,
            measure,
            String::from(label),
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
            vec![ParamValue::Date(day("2026-06-01")), ParamValue::Date(day("2026-08-01"))],
            TimeRange::new(day("2026-06-01"), day("2026-08-01")).expect("a test range is a range"),
        )
    }

    /// An adapter with one in-memory table called `orders`.
    ///
    /// `register_batch` is synchronous and takes a `RecordBatch`, so a test needs no fixture file and
    /// no temporary directory - which is what lets the end-to-end cases below run in the unit suite.
    fn warehouse(batch: RecordBatch) -> DataFusionWarehouse {
        let adapter = DataFusionWarehouse::new(SourceName::parse("local").expect("a test source is a source"), roomy())
            .expect("a current-thread runtime builds");
        drop(
            adapter
                .context
                .register_batch("orders", batch)
                .expect("an in-memory batch registers"),
        );
        adapter
    }

    fn batch(fields: Vec<Field>, columns: Vec<ArrayRef>) -> RecordBatch {
        RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).expect("a test batch is rectangular")
    }

    fn region_column() -> ArrayRef {
        Arc::new(StringArray::from(vec!["north", "north", "south"]))
    }

    fn date_column() -> ArrayRef {
        Arc::new(Date32Array::from(vec![
            day("2026-06-05").days_since_epoch(),
            day("2026-06-20").days_since_epoch(),
            day("2026-07-02").days_since_epoch(),
        ]))
    }

    fn region_key() -> Vec<PlanKey> {
        vec![PlanKey::new(String::from("region"), on("region"))]
    }

    #[test]
    fn a_day_number_comes_back_as_iso_text_so_the_two_adapters_agree() {
        // The bug: leaving a `Date32` as its day count, or rendering it with `Debug`. The `DuckDB`
        // adapter converts its own day numbers to ISO text through the same calendar, and a
        // differential test compares the two textually - so a bucket that came back as `20605` here
        // and as `2026-06-01` there would fail for a formatting reason and look like a data one.
        let array = Date32Array::from(vec![day("2026-06-01").days_since_epoch()]);
        assert_eq!(
            cell("period", &array, 0).expect("a day number is a date"),
            Value::Text(String::from("2026-06-01"))
        );
        // And a null stays a null rather than becoming the epoch.
        let absent = Date32Array::from(vec![None::<i32>]);
        assert_eq!(cell("period", &absent, 0).expect("a null is a null"), Value::Null);
    }

    #[test]
    fn a_column_type_this_adapter_does_not_map_is_an_error_naming_it() {
        // The bug: a `Debug` fallback. A 32-bit float widened to an `f64` prints as
        // 0.10000000149011612, which would flow into an answer looking like data and make the two
        // adapters disagree about a number neither of them got wrong.
        let array = Float32Array::from(vec![0.1_f32]);
        let error = cell("amount", &array, 0).expect_err("a 32-bit float is not mapped");
        assert!(matches!(error, DataFusionError::UnsupportedType { .. }), "{error:?}");
        let message = error.to_string();
        assert!(message.contains("amount"), "{message}");
        assert!(message.contains("Float32"), "{message}");
    }

    #[test]
    fn a_column_reference_keeps_the_case_the_catalog_wrote() {
        // The bug this crate is most exposed to. `col("Orders.Amount")` normalises BOTH halves to
        // lowercase, so a model whose table or column carries a capital resolves against a name that
        // does not exist - or, worse, against a different one that does. The domain preserves case
        // deliberately, so every reference here is built the case-preserving way.
        let mixed = PlanColumn::new(
            TableName::parse("Orders").expect("a test table is a table"),
            ColumnName::parse("Amount").expect("a test column is a column"),
        );
        assert_eq!(format!("{}", column(&mixed)), "Orders.Amount");
        // The negative control, and the only `col` in this crate: this is what the reference would
        // have silently been.
        assert_eq!(format!("{}", col("Orders.Amount")), "orders.amount");
    }

    #[test]
    fn every_aggregate_the_domain_has_maps_to_a_distinct_engine_function() {
        // The bug: a copy-pasted match arm that maps Max to the minimum. Nothing about the plan
        // changes, the query runs, and the number is wrong. Distinctness is what catches it; the
        // name check is what catches a whole column of arms shifted by one.
        let kinds = [
            Aggregate::Sum,
            Aggregate::Count,
            Aggregate::CountDistinct,
            Aggregate::Avg,
            Aggregate::Min,
            Aggregate::Max,
        ];
        let mut rendered: Vec<String> = kinds
            .iter()
            .map(|kind| format!("{}", aggregate_expr(*kind, column(&on("amount")))).to_lowercase())
            .collect();
        for (kind, text) in kinds.iter().zip(rendered.iter()) {
            let wanted = kind.as_str().trim_start_matches("count_");
            assert!(text.contains(wanted), "{kind} rendered as {text}");
        }
        rendered.sort();
        rendered.dedup();
        assert_eq!(rendered.len(), kinds.len());
    }

    #[test]
    fn every_grain_truncates_to_the_unit_the_sql_path_names() {
        // The bug: a unit spelled differently here than in `generate.rs`, which truncates to a
        // different period and makes the two adapters answer different questions from one plan.
        // Asserted against the domain's own spelling rather than against a second literal list, so
        // the two cannot drift apart quietly.
        for grain in [Grain::Day, Grain::Week, Grain::Month, Grain::Quarter, Grain::Year] {
            assert_eq!(unit(grain), grain.as_str());
        }
        assert_eq!(unit(Grain::Month), "month");
    }

    #[test]
    fn a_parameter_becomes_a_typed_value_and_never_syntax() {
        // The claim in `literal`'s doc, asserted. A value that would be an injection in a rendered
        // statement is a `Utf8` scalar here: there is no parser and no statement for it to be part
        // of, so the engine's only option is to compare it.
        let hostile = format!("{:?}", literal(&ParamValue::Text(String::from("a' OR '1'='1"))));
        assert!(hostile.contains("Utf8"), "{hostile}");
        assert!(hostile.contains("a' OR "), "{hostile}");
        // A date is bound as the day number the column holds, not as text the engine has to parse.
        let bound = format!("{:?}", literal(&ParamValue::Date(day("2026-06-01"))));
        assert!(bound.contains("Date32"), "{bound}");
    }

    #[test]
    fn a_ratio_casts_its_numerator_so_integer_division_cannot_truncate() {
        // The bug: `sum(cents) / count(*)` over two integer columns truncates in this engine exactly
        // as it does in SQL, so a ratio of 7 to 2 answers 3. Checked on the expression as well as
        // end to end below, because the cast is the part a refactor would drop.
        let measure = PlanMeasure::Ratio {
            numerator: agg(Aggregate::Sum, "hits"),
            denominator: agg(Aggregate::Sum, "tries"),
            zero_denominator: ZeroDenominator::Fail,
        };
        let rendered = format!("{}", measure_expression(&measure).expect("a ratio is one expression"));
        assert!(rendered.contains("Float64"), "{rendered}");
    }

    #[test]
    fn a_grouped_sum_comes_back_labelled_and_ordered_the_way_the_plan_says() {
        // The bug: a result whose columns are in a different order than `result_labels`, which every
        // consumer reads by position. Row order is asserted too, because the differential test
        // against the SQL path compares rows in order and an unordered aggregate is not stable.
        let adapter = warehouse(batch(
            vec![
                Field::new("region", DataType::Utf8, false),
                Field::new("order_date", DataType::Date32, false),
                Field::new("amount", DataType::Int64, false),
            ],
            vec![
                region_column(),
                date_column(),
                Arc::new(Int64Array::from(vec![100_i64, 50_i64, 7_i64])),
            ],
        ));
        let query = plan(simple(Aggregate::Sum, "amount"), "revenue", region_key());
        let result = adapter.execute(&query).expect("the plan runs");
        assert_eq!(result.columns(), query.result_labels().as_slice());
        assert_eq!(
            result.rows(),
            [
                vec![
                    Value::Text(String::from("north")),
                    Value::Text(String::from("2026-06-01")),
                    Value::Integer(150),
                ],
                vec![
                    Value::Text(String::from("south")),
                    Value::Text(String::from("2026-07-01")),
                    Value::Integer(7),
                ],
            ]
        );
    }

    #[test]
    fn a_ratio_whose_zero_denominator_yields_null_answers_null_rather_than_failing() {
        // Two bugs at once. Integer division would answer 3 for 7 over 2, and dividing by a zero
        // denominator is a hard "Divide by zero" in this engine rather than the null that SQL's
        // NULLIF produces - which would fail the whole question because one group had no tries.
        let adapter = warehouse(batch(
            vec![
                Field::new("region", DataType::Utf8, false),
                Field::new("order_date", DataType::Date32, false),
                Field::new("hits", DataType::Int64, false),
                Field::new("tries", DataType::Int64, false),
            ],
            vec![
                region_column(),
                date_column(),
                Arc::new(Int64Array::from(vec![7_i64, 0_i64, 5_i64])),
                Arc::new(Int64Array::from(vec![2_i64, 0_i64, 0_i64])),
            ],
        ));
        let query = plan(
            PlanMeasure::Ratio {
                numerator: agg(Aggregate::Sum, "hits"),
                denominator: agg(Aggregate::Sum, "tries"),
                zero_denominator: ZeroDenominator::Null,
            },
            "hit_rate",
            region_key(),
        );
        let result = adapter.execute(&query).expect("the plan runs");
        assert_eq!(result.cell(0, 2), Some(&Value::Real(real(3.5))));
        assert_eq!(result.cell(1, 2), Some(&Value::Null));
    }

    #[test]
    fn a_count_if_answers_zero_for_a_group_with_no_matches_rather_than_nothing() {
        // The bug: a filtered count, or a CASE with no ELSE, leaves a group whose every row is false
        // with a null instead of a 0. `generate.rs` sums a CASE with an ELSE for exactly this
        // reason, and a differential test would see null against 0.
        let adapter = warehouse(batch(
            vec![
                Field::new("region", DataType::Utf8, false),
                Field::new("order_date", DataType::Date32, false),
                Field::new("paid", DataType::Boolean, true),
            ],
            vec![
                region_column(),
                date_column(),
                Arc::new(BooleanArray::from(vec![Some(false), None, Some(true)])),
            ],
        ));
        let query = plan(
            PlanMeasure::Simple {
                term: PlanTerm::CountIf { column: on("paid") },
            },
            "paid_orders",
            region_key(),
        );
        let result = adapter.execute(&query).expect("the plan runs");
        assert_eq!(result.cell(0, 2), Some(&Value::Integer(0)));
        assert_eq!(result.cell(1, 2), Some(&Value::Integer(1)));
    }

    #[test]
    fn a_conditional_count_is_usable_as_a_ratio_numerator() {
        // The metric the previous vocabulary could not express, executed. `count_if` was a sibling
        // of `ratio` rather than a term inside one, so a rate over a conditional count had every
        // ingredient present and nowhere to write it. Two groups on purpose: `north` divides 1 by 2
        // and `south` divides 0 by 1, which is the answer a `count_if` denominator would have got
        // wrong as a null.
        let adapter = warehouse(batch(
            vec![
                Field::new("region", DataType::Utf8, false),
                Field::new("order_date", DataType::Date32, false),
                Field::new("paid", DataType::Boolean, true),
                Field::new("order_id", DataType::Int64, false),
            ],
            vec![
                region_column(),
                date_column(),
                Arc::new(BooleanArray::from(vec![Some(true), Some(false), None])),
                Arc::new(Int64Array::from(vec![1_i64, 2_i64, 3_i64])),
            ],
        ));
        let query = plan(
            PlanMeasure::Ratio {
                numerator: PlanTerm::CountIf { column: on("paid") },
                denominator: agg(Aggregate::CountDistinct, "order_id"),
                zero_denominator: ZeroDenominator::Null,
            },
            "paid_share",
            region_key(),
        );
        let result = adapter.execute(&query).expect("the plan runs");
        assert_eq!(result.cell(0, 2), Some(&Value::Real(real(0.5))));
        assert_eq!(result.cell(1, 2), Some(&Value::Real(real(0.0))));
    }

    #[test]
    fn the_engine_asks_for_one_row_past_the_cap_exactly_as_the_sql_path_does() {
        // The row cap is enforced ABOVE this port, by comparing the rows that came back against
        // `QueryPlan::max_rows` - and that comparison can only tell "reached the cap" from "cut off
        // by the cap" if the adapter asked for `row_limit`, which is one more. So the `+1` is not an
        // implementation detail of the SQL renderer: it is half of the mechanism, and an adapter that
        // asked for `max_rows` instead would silently truncate at exactly the boundary while every
        // other test still passed.
        //
        // The SQL path has this pinned by a golden that reads `LIMIT 10001`. This path renders no SQL
        // and had nothing, which is the asymmetry that makes a fix in one renderer a differential
        // failure waiting to happen. Asserted on the logical plan rather than by counting ten
        // thousand rows through the engine, because the claim is about the number asked for.
        let adapter = warehouse(batch(
            vec![
                Field::new("region", DataType::Utf8, false),
                Field::new("order_date", DataType::Date32, false),
                Field::new("amount", DataType::Int64, false),
            ],
            vec![
                region_column(),
                date_column(),
                Arc::new(Int64Array::from(vec![100_i64, 50_i64, 7_i64])),
            ],
        ));
        let query = plan(simple(Aggregate::Sum, "amount"), "revenue", region_key());
        let logical = adapter
            .runtime
            .block_on(adapter.logical_plan(&query))
            .expect("the plan resolves");
        let rendered = format!("{}", logical.display_indent());
        assert!(
            rendered.contains(&format!("fetch={}", query.row_limit())),
            "the engine must fetch one past the cap:\n{rendered}"
        );
        assert!(
            !rendered.contains(&format!("fetch={}", query.max_rows())),
            "fetching exactly the cap makes a full result indistinguishable from a truncated one:\n{rendered}"
        );
    }

    #[test]
    fn a_plan_naming_a_table_that_was_never_attached_is_an_error_and_never_an_empty_answer() {
        // An unattached table must be an error and not an empty result, because an empty result
        // reads as "there was no revenue in June". The engine resolves every name during analysis,
        // so that holds on the only pass this adapter makes.
        let adapter = DataFusionWarehouse::new(SourceName::parse("local").expect("a test source is a source"), roomy())
            .expect("a current-thread runtime builds");
        let query = plan(simple(Aggregate::Sum, "amount"), "revenue", region_key());

        // And it holds WITHOUT a pre-flight, which is the other half of the claim. This adapter
        // takes the port's defaulted `dry_run` deliberately: checking here means building the
        // logical plan and running the analyzer and the optimizer, which is most of executing it,
        // so a required pre-check bought this guarantee at the price of planning every question
        // twice. Asserted rather than assumed, because "the engine does not pre-check" is exactly
        // the kind of claim that stops being true when somebody adds an override back.
        assert!(
            adapter.dry_run(&query).is_ok(),
            "the engine answers `would this work` by not asking, so a plan it cannot run still dry-runs clean"
        );

        let error = adapter.execute(&query).expect_err("an unattached table does not resolve");
        assert!(matches!(error, DataFusionError::Analyze { .. }), "{error:?}");
        assert_eq!(adapter.source().as_str(), "local");
    }
}
