//! A `run_sql` statement cut into one part per listed source, each part run through the raw port,
//! and the rest planned here over the rows that came back: Decision 2 of
//! `docs/adr/20261010140559-datafusion-federates-the-sql-tool-and-the-metrics.md`.
//!
//! The session is the call's own and registers only the question's tables, which are the caller's
//! models on listed sources, each typed by its pinned schema. `datafusion-federation`'s rule finds
//! the largest sub-plans one source answers, and `Cutter` makes each one a leaf.
//! [`answer`](crate::combine::statement::answer) unparses every leaf in its source's dialect before
//! the one `open` over every source the plan names, runs each through
//! [`StatementLegs`](sutura_domain::plan::StatementLegs), and plans the leaf over the rows that came back. The
//! engine holds no runner and no credential. A plan one source answers whole is that source's
//! statement, and its rows are the answer. Planning and the local part stop at the request's
//! deadline, which the legs check before each one runs, and the session holds no table function.
//!
//! | Limit | Why |
//! | --- | --- |
//! | `compute_context` is `None`, not per subject | the session is one call's, over one caller's tables, so no two subjects' scans meet in it |
//! | a leg's rows are not charged against the working set | the raw port capped them by count; the pool charges what the local part builds over them |
//! | planning or the local part stops at the first yield past the deadline, not at the instant | every node yields after each batch, so one batch's work, a constant folded while planning and a sort's final in-memory pass run to their end |
//! | the certified metrics' combines wait while a statement's local part runs | they share the combiner's one current-thread runtime |
//! | a value built outside the working set can still be large: a string doubled through each common table expression, or a `CAST` to a fixed-size list of a length the caller spells | the pool charges only what operators reserve, and a constant is folded while planning; the session leaves out only the functions whose output length the caller spells or multiplies, and neither an operator nor a SQL type is a function |
//! | a statement of more than `MOST_OPERATORS` operator tokens is refused, however shallow it is | the count is taken over the text, before any tree is built, because `recursive_protection` grows the stack for the parser, planner and optimizer but not for an expression's `Display`, `Clone` or `Drop` |
//! | a path two listed sources both carry is a table of neither | the text does not say which source a caller meant |
//! | an `EXISTS` outside a filter over a second source's table is refused | the rule does not cut inside it, and the part holding it would read two sources |
//! | only `DuckDB` and `Postgres` sources plan a part | they are the adapters that take raw statements |
//! | a statement that reads no table is refused | there is no source to run it at under the caller |
//! | unquoted identifiers are lowercased | the engine's own normalization, before the source sees the text |
//! | the cut runs after every default rule | `SessionStateBuilder::with_optimizer_rule` appends |

use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use datafusion::arrow::array::RecordBatch;
use datafusion::arrow::compute::{CastOptions, cast_with_options};
use datafusion::arrow::datatypes::SchemaRef;
use datafusion::catalog::Session;
use datafusion::common::tree_node::{Transformed, TreeNode as _, TreeNodeRecursion};
use datafusion::common::{plan_datafusion_err, plan_err};
use datafusion::config::ConfigOptions;
use datafusion::datasource::memory::{MemoryCatalogProvider, MemorySchemaProvider, MemorySourceConfig};
use datafusion::error::Result as Planned;
use datafusion::execution::{SessionStateBuilder, SessionStateDefaults};
use datafusion::logical_expr::{Extension, LogicalPlan, Projection, ScalarUDF, TableSource};
use datafusion::optimizer::{Optimizer, OptimizerConfig, OptimizerRule};
use datafusion::physical_optimizer::PhysicalOptimizerRule;
use datafusion::physical_plan::ExecutionPlan;
use datafusion::physical_plan::coop::CooperativeExec;
use datafusion::prelude::{SQLOptions, SessionConfig, SessionContext};
use datafusion::sql::sqlparser::dialect::GenericDialect;
use datafusion::sql::sqlparser::keywords::Keyword;
use datafusion::sql::sqlparser::tokenizer::{Token, Tokenizer, Word};
use datafusion::sql::unparser::Unparser;
use datafusion::sql::unparser::dialect::{Dialect, DuckDBDialect, PostgreSqlDialect};
use datafusion_federation::{
    FederatedPlanNode, FederatedQueryPlanner, FederatedTableProviderAdaptor, FederatedTableSource, FederationOptimizerRule,
    FederationPlanner, FederationProvider, get_table_source,
};
use sutura_domain::model::{ProjectName, SourceName};
use sutura_domain::plan::{StatementLegs, StatementQuestion, StatementStopped, StatementTable};
use sutura_domain::raw::RawStatement;
use sutura_domain::warehouse::RawRows;
use sutura_domain::warehouse::arrow::of_rows;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::schema::TypeDialect;
use tokio::runtime::Runtime;

use super::{CombineError, DataFusionCombiner, collected, working_set_of};
use crate::pool::{self, WorkingSet};

/// Answers `question`: its statement planned over its tables, each source's part run by `legs`.
pub(super) fn answer<L>(combiner: &DataFusionCombiner, question: StatementQuestion<'_>, legs: &mut L) -> Answered<L::Stop>
where
    L: StatementLegs,
{
    let runtime = combiner.runtime().map_err(|cause| unplanned("no runtime", &cause))?;
    let working_set = working_set_of(question.working_set_bytes());
    let fetched = Arc::new(Fetched::default());
    let context = session(question.tables(), &fetched, working_set).map_err(|cause| unplanned("no session", &cause))?;
    let plan = within(runtime, question.deadline(), planned(&context, question.statement()))
        .ok_or(StatementStopped::DeadlineExceeded)?
        .map_err(|cause| unplanned("the statement did not plan", &cause))?;
    let parts = parts(&plan, question.tables()).map_err(|cause| unplanned("the plan did not cut", &cause))?;
    let sources: BTreeSet<SourceName> = parts.iter().map(|part| part.source.clone()).collect();
    if sources.len() > 1 && !question.federation_allowed() {
        return Err(StatementStopped::SpansMoreThanOneSource);
    }
    let whole = part_of(&plan).is_some();
    let statements = parts
        .iter()
        .map(|part| part.statement(whole))
        .collect::<Planned<Vec<_>>>()
        .map_err(|cause| unplanned("a part did not unparse", &cause))?;
    legs.open(&sources).map_err(StatementStopped::Leg)?;
    let mut leaves = Vec::with_capacity(parts.len());
    for (part, statement) in parts.into_iter().zip(&statements) {
        let rows = legs.run(&part.source, statement).map_err(StatementStopped::Leg)?;
        if whole {
            return Ok(rows);
        }
        let batches =
            fitted(&rows, part.plan.schema().inner()).map_err(|cause| unplanned("a leg did not fit its part", &cause))?;
        leaves.push((part.plan, batches));
    }
    if fetched.0.set(leaves).is_err() {
        return Err(StatementStopped::NotPlanned);
    }
    let local = within(runtime, question.deadline(), async {
        let frame = context
            .execute_logical_plan(plan)
            .await
            .map_err(|cause| CombineError::Execute { cause })?;
        collected(frame, question.working_set_bytes(), working_set).await
    })
    .ok_or(StatementStopped::DeadlineExceeded)?;
    let answered = match local {
        Ok(answered) => answered,
        Err(CombineError::Exhausted { .. }) => return Err(StatementStopped::AnswerTooLarge),
        Err(cause) => return Err(unplanned("the local part did not run", &cause)),
    };
    let (columns, rows) = answered
        .to_rows()
        .map_err(|cause| unplanned("the answer did not read as rows", &cause))?
        .into_parts();
    Ok(RawRows::of(columns, rows))
}

/// The statement's rows, or why it got none.
type Answered<S> = Result<RawRows, StatementStopped<S>>;

/// `work` on `runtime` until it ends or `deadline` passes: `None` past it, `work` not started when
/// the budget is already spent.
fn within<F>(runtime: &Runtime, deadline: Deadline, work: F) -> Option<F::Output>
where
    F: Future,
{
    let left = deadline.remaining_at(Instant::now())?;
    runtime.block_on(async { tokio::time::timeout(left, work).await.ok() })
}

/// Wraps every node, not only the leaves the default rule wraps, so each batch any operator yields
/// spends the task's budget and the deadline's timer is polled between them.
#[derive(Debug)]
struct Yielding;

impl PhysicalOptimizerRule for Yielding {
    fn optimize(&self, plan: Arc<dyn ExecutionPlan>, _: &ConfigOptions) -> Planned<Arc<dyn ExecutionPlan>> {
        plan.transform_up(|node| {
            let wrapped: Arc<dyn ExecutionPlan> = Arc::new(CooperativeExec::new(node));
            Ok(Transformed::yes(wrapped))
        })
        .map(|wrapped| wrapped.data)
    }

    fn name(&self) -> &'static str {
        "sutura_statement_yielding"
    }

    fn schema_check(&self) -> bool {
        true
    }
}

/// Each part's plan beside the rows its leg came back with.
type Leaves = Vec<(LogicalPlan, Vec<RecordBatch>)>;

/// One listed source, as the federation rule sees it: two tables' scans fuse when their sources do.
struct Source {
    name: SourceName,
    cut: Arc<Optimizer>,
}

impl FederationProvider for Source {
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn compute_context(&self) -> Option<String> {
        None
    }

    fn optimizer(&self) -> Option<Arc<Optimizer>> {
        Some(Arc::clone(&self.cut))
    }
}

/// A table of a listed source, typed by its pinned schema. It is never scanned here.
struct Table {
    source: Arc<Source>,
    schema: SchemaRef,
}

impl TableSource for Table {
    fn schema(&self) -> SchemaRef {
        Arc::clone(&self.schema)
    }
}

impl FederatedTableSource for Table {
    fn federation_provider(&self) -> Arc<dyn FederationProvider> {
        Arc::<Source>::clone(&self.source)
    }
}

/// Each part's plan beside the rows its leg came back with, set once every leg has run.
#[derive(Debug, Default)]
struct Fetched(OnceLock<Leaves>);

#[async_trait::async_trait]
impl FederationPlanner for Fetched {
    async fn plan_federation(&self, node: &FederatedPlanNode, _: &dyn Session) -> Planned<Arc<dyn ExecutionPlan>> {
        let Some((_, batches)) = self
            .0
            .get()
            .and_then(|parts| parts.iter().find(|(plan, _)| plan == node.plan()))
        else {
            return plan_err!("a part of the plan was planned before its leg ran");
        };
        let leaf: Arc<dyn ExecutionPlan> =
            MemorySourceConfig::try_new_exec(core::slice::from_ref(batches), Arc::clone(node.plan().schema().inner()), None)?;
        Ok(leaf)
    }
}

/// The rule a source's sub-plan is handed to: it makes the sub-plan a leaf, planned over its leg's rows.
#[derive(Debug)]
struct Cutter(Arc<Fetched>);

impl OptimizerRule for Cutter {
    fn name(&self) -> &'static str {
        "sutura_statement_cut"
    }

    fn rewrite(&self, plan: LogicalPlan, _: &dyn OptimizerConfig) -> Planned<Transformed<LogicalPlan>> {
        if part_of(&plan).is_some() {
            return Ok(Transformed::no(plan));
        }
        let planner: Arc<dyn FederationPlanner> = Arc::<Fetched>::clone(&self.0);
        Ok(Transformed::yes(LogicalPlan::Extension(Extension {
            node: Arc::new(FederatedPlanNode::new(plan, planner)),
        })))
    }
}

/// A sub-plan one source answers.
struct Part {
    plan: LogicalPlan,
    source: SourceName,
    dialect: TypeDialect,
}

impl Part {
    /// The part in its source's dialect: as planned when it is the `whole` statement, otherwise
    /// with its columns labelled `c0..cn`, so the rows can be read by position and checked by name.
    fn statement(&self, whole: bool) -> Planned<RawStatement> {
        let text = if whole {
            unparsed(&self.plan, self.dialect)?
        } else {
            unparsed(&labelled(&self.plan)?, self.dialect)?
        };
        RawStatement::parse(text).map_err(|cause| plan_datafusion_err!("{cause}"))
    }
}

fn part_of(plan: &LogicalPlan) -> Option<&FederatedPlanNode> {
    let LogicalPlan::Extension(extension) = plan else {
        return None;
    };
    extension.node.as_any().downcast_ref()
}

/// Every distinct part of `plan`, subqueries included, each with the source and dialect it runs in.
fn parts(plan: &LogicalPlan, tables: &[StatementTable]) -> Planned<Vec<Part>> {
    let mut parts: Vec<Part> = Vec::new();
    plan.apply_with_subqueries(|node| {
        if let Some(cut) = part_of(node)
            && !parts.iter().any(|part| &part.plan == cut.plan())
        {
            let name = source_of(cut.plan())?;
            let Some(table) = tables.iter().find(|table| table.source().as_str() == name) else {
                return plan_err!("a part of the plan reads a source the question does not carry");
            };
            parts.push(Part {
                plan: cut.plan().clone(),
                source: table.source().clone(),
                dialect: table.dialect(),
            });
        }
        Ok(TreeNodeRecursion::Continue)
    })?;
    if parts.is_empty() {
        return plan_err!("the statement reads no table of a listed source");
    }
    Ok(parts)
}

/// The one source `part` reads, subqueries included. The rule does not look inside an `EXISTS`, so
/// a part can hold a second source's table, and such a part is refused rather than sent to either.
fn source_of(part: &LogicalPlan) -> Planned<String> {
    let mut names = BTreeSet::new();
    part.apply_with_subqueries(|node| {
        if let LogicalPlan::TableScan(scan) = node
            && let Some(table) = get_table_source(&scan.source)?
        {
            names.insert(table.federation_provider().name().to_owned());
        }
        Ok(TreeNodeRecursion::Continue)
    })?;
    let mut names = names.into_iter();
    match (names.next(), names.next()) {
        (Some(name), None) => Ok(name),
        (None, _) => plan_err!("a part of the plan reads no table"),
        (Some(_), Some(_)) => plan_err!("a part of the plan reads more than one source"),
    }
}

/// The call's session: `tables` registered at their paths, the cut installed, the working set as
/// its pool. No information schema, so a caller lists nothing, no table function, so a caller
/// generates no rows, and only the scalar functions `ADMITTED` names.
fn session(tables: &[StatementTable], fetched: &Arc<Fetched>, working_set: WorkingSet) -> Planned<SessionContext> {
    let state = SessionStateBuilder::new()
        .with_config(SessionConfig::new().with_default_catalog_and_schema("", ""))
        .with_runtime_env(
            pool::environment(working_set)
                .map_err(|cause| plan_datafusion_err!("{cause}"))?
                .into_runtime(),
        )
        .with_default_features()
        .with_scalar_functions(admitted())
        .with_table_function_list(Vec::new())
        .with_optimizer_rule(Arc::new(FederationOptimizerRule::default()))
        .with_physical_optimizer_rule(Arc::new(Yielding))
        .with_query_planner(Arc::new(FederatedQueryPlanner::new()))
        .build();
    let context = SessionContext::new_with_state(state);
    let cut = Arc::new(Optimizer::with_rules(vec![Arc::new(Cutter(Arc::clone(fetched)))]));
    for table in tables {
        if tables
            .iter()
            .any(|other| other.table() == table.table() && other.source() != table.source())
        {
            continue;
        }
        let (catalog, schema) = table.table().qualifier().map_or(("", ""), |qualifier| {
            (
                qualifier.project().map_or("", ProjectName::as_str),
                qualifier.dataset().as_str(),
            )
        });
        if context.catalog(catalog).is_none() {
            context.register_catalog(catalog, Arc::new(MemoryCatalogProvider::new()));
        }
        let Some(catalog) = context.catalog(catalog) else {
            return plan_err!("a table's catalog did not register");
        };
        if catalog.schema(schema).is_none() {
            catalog.register_schema(schema, Arc::new(MemorySchemaProvider::new()))?;
        }
        let Some(schema) = catalog.schema(schema) else {
            return plan_err!("a table's schema did not register");
        };
        let source = Arc::new(Source {
            name: table.source().clone(),
            cut: Arc::clone(&cut),
        });
        let provider = FederatedTableProviderAdaptor::new(Arc::new(Table {
            source,
            schema: Arc::clone(table.schema()),
        }));
        schema.register_table(table.table().name().as_str().to_owned(), Arc::new(provider))?;
    }
    Ok(context)
}

/// The scalar functions a statement may call: `datafusion`'s defaults less each one whose output a
/// caller can grow past a constant factor of its input in one call, because a constant call is
/// folded while planning, outside the pool. `repeat`, `lpad` and `rpad` take the length as an
/// argument; `replace` and `regexp_replace` insert at every position of an empty pattern; `encode`
/// doubles per nesting; `arrow_cast` and `arrow_try_cast` spell a type, and a null fixed-size list
/// of `i32::MAX` items is reserved whole. A function a later `datafusion` adds is out until named.
const ADMITTED: &str = "
    arrow_field arrow_metadata arrow_typeof cast_to_type coalesce file_row_index get_field greatest input_file_name least
    named_struct nullif nvl nvl2 overlay struct try_cast_to_type union_extract union_tag version with_metadata
    current_date current_time date_bin date_part date_trunc from_unixtime make_date make_time now to_char to_date
    to_local_time to_time to_timestamp to_timestamp_micros to_timestamp_millis to_timestamp_nanos to_timestamp_seconds
    to_unixtime decode
    abs acos acosh asin asinh atan atan2 atanh cbrt ceil cos cosh cot degrees exp factorial floor gcd isnan iszero lcm ln
    log log10 log2 nanvl pi power radians random round signum sin sinh sqrt tan tanh trunc
    regexp_count regexp_instr regexp_like regexp_match
    character_length find_in_set initcap left reverse right strpos substr substr_index translate
    ascii bit_length btrim chr concat concat_ws contains ends_with levenshtein lower ltrim octet_length rtrim split_part
    starts_with to_hex upper uuid
";

fn admitted() -> Vec<Arc<ScalarUDF>> {
    SessionStateDefaults::default_scalar_functions()
        .into_iter()
        .filter(|function| ADMITTED.split_ascii_whitespace().any(|name| name == function.name()))
        .collect()
}

/// The most tokens a statement may spend on anything but a name, a literal or a parenthesis. A chain
/// such as `1 + 1 + ..` nests one level per operator without passing the parser's depth limit, and
/// `datafusion` displays, clones and drops an expression by a recursion `recursive_protection` does
/// not reach, so the text is held to what can only build a shallow tree. The number is a judgement:
/// a cell measures that a chain just under it plans on a test thread's stack in the test profile,
/// whose dependencies are optimised further than a shipped build's. A long statement of many
/// keywords, commas or qualified names is refused though it is shallow.
const MOST_OPERATORS: usize = 1024;

/// `statement`'s optimized plan, refused unless it is one query of at most [`MOST_OPERATORS`].
async fn planned(context: &SessionContext, statement: &RawStatement) -> Planned<LogicalPlan> {
    let tokens = Tokenizer::new(&GenericDialect {}, statement.as_str())
        .tokenize()
        .map_err(|cause| plan_datafusion_err!("{cause}"))?;
    let operators = tokens
        .iter()
        .filter(|token| {
            !matches!(
                token,
                Token::Whitespace(_)
                    | Token::LParen
                    | Token::RParen
                    | Token::Number(..)
                    | Token::SingleQuotedString(_)
                    | Token::Word(Word {
                        keyword: Keyword::NoKeyword,
                        ..
                    })
            )
        })
        .count();
    if operators > MOST_OPERATORS {
        return plan_err!("the statement has {operators} operators, more than {MOST_OPERATORS}");
    }
    let options = SQLOptions::new()
        .with_allow_ddl(false)
        .with_allow_dml(false)
        .with_allow_statements(false);
    let frame = context.sql_with_options(statement.as_str(), options).await?;
    if matches!(
        frame.logical_plan(),
        LogicalPlan::Explain(_) | LogicalPlan::Analyze(_) | LogicalPlan::DescribeTable(_)
    ) {
        return plan_err!("the statement is not a query");
    }
    frame.into_optimized_plan()
}

/// `part` with its projection's columns renamed `c0..cn`. The federation rule roots every part it
/// cuts from a larger plan at a projection.
fn labelled(part: &LogicalPlan) -> Planned<LogicalPlan> {
    let LogicalPlan::Projection(projection) = part else {
        return plan_err!("a part of the plan is not rooted at a projection");
    };
    let columns = projection
        .expr
        .iter()
        .enumerate()
        .map(|(index, column)| column.clone().unalias().alias(label(index)))
        .collect();
    Ok(LogicalPlan::Projection(Projection::try_new(
        columns,
        Arc::clone(&projection.input),
    )?))
}

fn label(index: usize) -> String {
    format!("c{index}")
}

fn unparsed(plan: &LogicalPlan, dialect: TypeDialect) -> Planned<String> {
    let text = |spelling: &dyn Dialect| {
        Unparser::new(spelling)
            .plan_to_sql(plan)
            .map(|statement| statement.to_string())
    };
    match dialect {
        TypeDialect::DuckDb => text(&DuckDBDialect::new()),
        TypeDialect::Postgres => text(&PostgreSqlDialect {}),
        TypeDialect::BigQuery | TypeDialect::ClickHouse | TypeDialect::Oracle => {
            plan_err!("only a source that takes raw statements has a part")
        }
    }
}

/// A leg's rows as batches of its part's schema: its columns the part's labels in order, each cast
/// with `safe: false`, so a value that does not read as its column's type refuses the answer.
fn fitted(rows: &RawRows, schema: &SchemaRef) -> Planned<Vec<RecordBatch>> {
    let fields = schema.fields();
    if rows.columns().len() != fields.len()
        || rows
            .columns()
            .iter()
            .enumerate()
            .any(|(index, column)| *column != label(index))
    {
        return plan_err!("a leg answered other columns than its part asked for");
    }
    let options = CastOptions {
        safe: false,
        ..CastOptions::default()
    };
    of_rows(rows.columns(), rows.rows())
        .map_err(|cause| plan_datafusion_err!("{cause}"))?
        .into_batches()
        .into_iter()
        .map(|batch| {
            let columns = batch
                .columns()
                .iter()
                .zip(fields)
                .map(|(column, field)| {
                    cast_with_options(column, field.data_type(), &options).map_err(|_names_the_value| {
                        plan_datafusion_err!("a leg's {} did not read as {}", field.name(), field.data_type())
                    })
                })
                .collect::<Planned<Vec<_>>>()?;
            Ok(RecordBatch::try_new(Arc::clone(schema), columns)?)
        })
        .collect()
}

/// The engine's words name columns, so they go to the log and the caller reads only that the
/// statement did not plan.
fn unplanned<S>(why: &str, cause: &dyn core::fmt::Display) -> StatementStopped<S> {
    tracing::info!(%cause, "run_sql: {why}");
    StatementStopped::NotPlanned
}

/// Declared here rather than in `combine::tests`, because this file is new: the base run removes it,
/// and with it every cell below, instead of compiling cells over a port it does not have.
#[cfg(test)]
mod tests;
