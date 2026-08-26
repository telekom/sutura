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

use std::path::Path;

use datafusion::arrow::array::{
    Array, BooleanArray, Date32Array, Decimal128Array, Float64Array, Int8Array, Int16Array, Int32Array, Int64Array, StringArray,
    StringViewArray, UInt8Array, UInt16Array, UInt32Array, UInt64Array,
};
use datafusion::arrow::datatypes::DataType;
use datafusion::common::{Column, DFSchema, JoinType as EngineJoin};
use datafusion::logical_expr::{Expr, LogicalPlan, LogicalPlanBuilder};
use datafusion::prelude::{CsvReadOptions, ParquetReadOptions, SessionContext};
use sutura_domain::calendar::Date;
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

/// A plan becomes expressions in [`crate::translate`], which is the half of this adapter that never
/// reads a result. What is left in this file is the other half: the session, the schema work, and
/// turning Arrow arrays back into domain rows.
mod translate;

use crate::translate::{bucket_expression, column, measure_expression, predicate, table_reference};

/// The aliased projection, and the expressions to order the result by.
///
/// Named rather than written out, because the two lists are produced together and consumed one line
/// apart: separating them into two passes over the same schema is how they would come to disagree
/// about which position is a grouped one.
type Projected = (Vec<Expr>, Vec<Expr>);

/// The final projection, and the expressions to order by.
///
/// The projection references the aggregate's own output fields rather than re-stating the grouped
/// expressions, because after aggregating there is no `orders.order_date` left to truncate - the
/// truncated value *is* a field. Each is aliased so the result labels are exactly
/// `QueryPlan::result_labels` in order: the keys, then the time bucket, then the measure.
///
/// Ordering is by the projected label columns, unaliased, for the grouped positions only. That is
/// the SQL path's "order by what it grouped by", and it is what makes two runs of one question
/// return rows in one order - which a differential test over row order depends on.
fn outputs(schema: &DFSchema, labels: &[String], group_count: usize) -> Result<Projected, DataFusionError> {
    if schema.fields().len() != labels.len() {
        return Err(DataFusionError::SchemaMismatch {
            expected: labels.to_vec(),
            actual: schema.fields().iter().map(|f| String::from(f.name().as_str())).collect(),
        });
    }
    let mut projection = Vec::with_capacity(labels.len());
    let mut ordering = Vec::with_capacity(group_count);
    for (index, ((qualifier, field), label)) in schema.iter().zip(labels.iter()).enumerate() {
        let reference = Expr::Column(Column::new(qualifier.cloned(), field.name().as_str()));
        projection.push(reference.alias(label.as_str()));
        if index < group_count {
            // Unqualified: an aliased projection field carries no qualifier, so this is the name the
            // sort resolves against.
            ordering.push(Expr::Column(Column::new_unqualified(label.as_str())));
        }
    }
    Ok((projection, ordering))
}

/// One array, downcast to the type its schema declares.
fn typed<'array, A>(label: &str, array: &'array dyn Array) -> Result<&'array A, DataFusionError>
where
    A: 'static,
{
    array.as_any().downcast_ref::<A>().ok_or_else(|| DataFusionError::Downcast {
        column: String::from(label),
        arrow_type: core::any::type_name::<A>(),
    })
}

/// One cell, as a domain value.
///
/// **One mapping, two adapters, and the set is decided once.** This engine and the `DuckDB` data
/// source are the two implementors of the [`Warehouse`] port, so a type that answers there and
/// errors here means an anchor certified against one adapter does not reproduce against the other.
/// `cell` in `crates/sutura-exec-duckdb/src/lib.rs` is the other half, and
/// `value_mapping_tests.rs` beside this file is the table both are held to.
///
/// Every integer width answers, because every one of them fits an `i64` losslessly - a Parquet
/// `INT32` column under a `min` or a `max` used to answer through the data source and error here. A
/// 64-bit unsigned value that does not fit is rendered as text rather than wrapped: a silently
/// truncated total is a wrong number.
///
/// An unmapped type is [`DataFusionError::UnsupportedType`] naming the column and the Arrow type
/// rather than a `Debug` rendering. Widening a 32-bit float to an `f64` would be the tempting one
/// and is exactly wrong: `0.1_f32` as an `f64` prints as `0.10000000149011612`, and the two adapters
/// would then disagree about a number neither of them got wrong.
///
/// `Date64` is deliberately NOT here, and not for symmetry's sake either: the data source has no
/// counterpart to be symmetric with - `DuckDB`'s `DATE` is a day count - and nothing on this path
/// produces one, because `date_trunc` over a `Date32` stays a `Date32` and a Parquet `DATE` logical
/// type reads as `Date32`. Mapping it would mean choosing what a millisecond count that is not a
/// whole number of days means, in an arm no question can reach. An unreachable arm holding a
/// semantic choice nobody reviewed is worse than an error naming the type.
fn cell(label: &str, array: &dyn Array, row: usize) -> Result<Value, DataFusionError> {
    if array.is_null(row) {
        return Ok(Value::Null);
    }
    match *array.data_type() {
        DataType::Int64 => Ok(Value::Integer(typed::<Int64Array>(label, array)?.value(row))),
        // Every narrower width, because `i64::from` is lossless for all of them. Written out rather
        // than reached through a cast so that the conversion is the compiler's business.
        DataType::Int8 => Ok(Value::Integer(i64::from(typed::<Int8Array>(label, array)?.value(row)))),
        DataType::Int16 => Ok(Value::Integer(i64::from(typed::<Int16Array>(label, array)?.value(row)))),
        DataType::Int32 => Ok(Value::Integer(i64::from(typed::<Int32Array>(label, array)?.value(row)))),
        DataType::UInt8 => Ok(Value::Integer(i64::from(typed::<UInt8Array>(label, array)?.value(row)))),
        DataType::UInt16 => Ok(Value::Integer(i64::from(typed::<UInt16Array>(label, array)?.value(row)))),
        DataType::UInt32 => Ok(Value::Integer(i64::from(typed::<UInt32Array>(label, array)?.value(row)))),
        // The one width that does not fit. Text when it overflows rather than wrapped, which is what
        // the data source does with its own `UBIGINT`: a total that came back correct must not
        // become a negative number on the way into an answer.
        DataType::UInt64 => {
            let value = typed::<UInt64Array>(label, array)?.value(row);
            Ok(i64::try_from(value).map_or_else(|_| Value::Text(value.to_string()), Value::Integer))
        }
        DataType::Float64 => Ok(Value::Real(typed::<Float64Array>(label, array)?.value(row))),
        DataType::Utf8 => Ok(Value::Text(String::from(typed::<StringArray>(label, array)?.value(row)))),
        // The Parquet default for a string column in this version, so the affordance that reads one
        // is not broken on arrival. CSV inference gives the owned form above.
        DataType::Utf8View => Ok(Value::Text(String::from(typed::<StringViewArray>(label, array)?.value(row)))),
        // ISO text, exactly as the `DuckDB` adapter converts its own day numbers, which is what lets
        // a differential test compare the time bucket of the two at all. Converted here rather than
        // cast to text inside the plan, so the domain's calendar stays the one definition of what a
        // day number means.
        DataType::Date32 => {
            let days = typed::<Date32Array>(label, array)?.value(row);
            Date::from_days_since_epoch(days)
                .map(|date| Value::Text(date.to_iso()))
                .map_err(|cause| DataFusionError::NotADate {
                    column: String::from(label),
                    cause,
                })
        }
        // 0 or 1, matching the `DuckDB` adapter, which maps a boolean to an integer for the same
        // reason: the domain's `Value` has no boolean, and a text "true" would compare unequal to
        // the other adapter's 1.
        DataType::Boolean => Ok(Value::Integer(i64::from(typed::<BooleanArray>(label, array)?.value(row)))),
        // Text, so an exact decimal stays exact. Turning it into an `f64` here is how a total that
        // was correct in the engine stops being correct in an answer.
        DataType::Decimal128(..) => Ok(Value::Text(typed::<Decimal128Array>(label, array)?.value_as_string(row))),
        ref other => Err(DataFusionError::UnsupportedType {
            column: String::from(label),
            arrow_type: format!("{other:?}"),
        }),
    }
}

/// An in-process engine, behind the [`Warehouse`] port.
pub struct DataFusionWarehouse {
    source: SourceName,
    context: SessionContext,
    /// **One runtime, built once and kept.** The engine is async from the first table lookup to the
    /// last batch collected, and a runtime built per query is a reactor created and torn down for
    /// every question. `new_current_thread` because the `rt` feature is all this crate enables: no
    /// `enable_all`, so no timer and no I/O driver, neither of which a plan over a local file needs.
    runtime: tokio::runtime::Runtime,
}

impl core::fmt::Debug for DataFusionWarehouse {
    /// Hand-written because neither the session context nor the runtime is `Debug`, and because a
    /// session context's own `Debug` would be the sort of thing that prints every registered path
    /// into a log for no benefit.
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
    pub fn new(source: SourceName) -> Result<Self, DataFusionError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .map_err(|cause| DataFusionError::Runtime { cause })?;
        Ok(Self {
            source,
            context: SessionContext::new(),
            runtime,
        })
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

    /// Resolves and type-checks the plan without reading anything.
    async fn check(&self, plan: &QueryPlan) -> Result<(), DataFusionError> {
        let logical = self.logical_plan(plan).await?;
        // `execute_logical_plan` runs the analyzer and the optimizer, which is where every table
        // name, every column name and every argument type is resolved. What it returns is still only
        // a plan: dropping the frame without collecting starts no execution and reads no data. That
        // is what makes this a real dry run rather than a stub.
        drop(
            self.context
                .execute_logical_plan(logical)
                .await
                .map_err(|cause| DataFusionError::Analyze { cause })?,
        );
        Ok(())
    }
}

impl Warehouse for DataFusionWarehouse {
    type Error = DataFusionError;

    fn source(&self) -> &SourceName {
        &self.source
    }

    fn dry_run(&self, plan: &QueryPlan) -> Result<(), Self::Error> {
        self.runtime.block_on(self.check(plan))
    }

    fn execute(&self, plan: &QueryPlan) -> Result<RowSet, Self::Error> {
        self.runtime.block_on(self.rows(plan))
    }
}

/// The half of the value mapping that is shared with the data source, in its own file.
///
/// Split out for the file-length gate rather than for taste: this one is already close to the
/// 1000-line limit, and the gate's answer to that is to split the file, not to shorten the fix.
#[cfg(test)]
mod value_mapping_tests;

#[cfg(test)]
mod tests {
    use super::translate::{aggregate_expr, literal, measure_expression, unit};
    use super::{DataFusionError, DataFusionWarehouse, cell, column};
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
    use sutura_domain::warehouse::{ParamValue, Value, Warehouse as _};

    fn day(iso: &str) -> Date {
        Date::parse(iso).expect("a test date is a date")
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
        let adapter = DataFusionWarehouse::new(SourceName::parse("local").expect("a test source is a source"))
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
        let whole = format!("{:?}", literal(&ParamValue::Integer(42)));
        assert!(whole.contains("42"), "{whole}");
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
        adapter.dry_run(&query).expect("the plan resolves");
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
        assert_eq!(result.cell(0, 2), Some(&Value::Real(3.5)));
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
        adapter.dry_run(&query).expect("the plan resolves");
        let result = adapter.execute(&query).expect("the plan runs");
        assert_eq!(result.cell(0, 2), Some(&Value::Real(0.5)));
        assert_eq!(result.cell(1, 2), Some(&Value::Real(0.0)));
    }

    #[test]
    fn a_plan_naming_a_table_that_was_never_attached_is_refused_before_anything_is_read() {
        // What `dry_run` is for. The engine resolves every name during analysis, so an unattached
        // table is an error rather than an empty answer - and an empty answer is the failure mode
        // that reads as "there was no revenue in June".
        let adapter = DataFusionWarehouse::new(SourceName::parse("local").expect("a test source is a source"))
            .expect("a current-thread runtime builds");
        let query = plan(simple(Aggregate::Sum, "amount"), "revenue", region_key());
        let error = adapter.dry_run(&query).expect_err("an unattached table does not resolve");
        assert!(matches!(error, DataFusionError::Analyze { .. }), "{error:?}");
        assert_eq!(adapter.source().as_str(), "local");
    }
}
