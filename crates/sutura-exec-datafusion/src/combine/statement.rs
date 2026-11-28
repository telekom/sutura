//! A `run_sql` statement cut into supported source parts, each run through the planned port,
//! and the rest planned here over the rows that came back: Decision 2 of
//! `docs/adr/20261010140559-datafusion-federates-the-sql-tool-and-the-metrics.md`.
//!
//! The session is the call's own and registers only the question's tables, which are the caller's
//! models on listed sources, each typed by its pinned schema. `datafusion-federation`'s rule finds
//! the largest sub-plans one source answers, and `Cutter` makes each one a leaf; the vendored copy
//! (`VENDOR.md`) also sends a plan that is only a scan, at the root, to the same cut.
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
//! | a leg's rows are not charged against the working set | the source port capped them by count; the pool charges what the local part builds over them |
//! | planning or the local part stops at the first yield past the deadline, not at the instant | every node yields after each batch, so one batch's work, a constant folded while planning and a sort's final in-memory pass run to their end |
//! | the certified metrics' combines wait while a statement's local part runs | they share the combiner's one current-thread runtime |
//! | a value built outside the working set can still be large: a string doubled through each common table expression, or a `CAST` to a fixed-size list of a length the caller spells | the pool charges only what operators reserve, and a constant is folded while planning; the session leaves out only the functions whose output length the caller spells or multiplies, and neither an operator nor a SQL type is a function |
//! | a statement of more than `MOST_OPERATORS` operator tokens is refused, however shallow it is | the count is taken over the text, before any tree is built, because `recursive_protection` grows the stack for the parser, planner and optimizer but not for an expression's `Display`, `Clone` or `Drop` |
//! | a path two listed sources both carry resolves only under the one whose source the caller names, `source.schema.table` | an unprefixed `schema.table` would not say which source the caller meant |
//! | a reference that only resolved by naming its source refuses `answer_sql`'s fallback to the caller's own text against a single source | that text is not admission-checked - the source would see `source.X` literally |
//! | an `EXISTS` outside a filter over a second source's table is refused | the rule does not cut inside it, and the part holding it would read two sources |
//! | Oracle's scalar function and cast vocabulary differs from the engine's | unsupported spellings are refused by the source; this is no complete Oracle expression translator |
//! | value-dependent uses of an explicit NUMBER-to-Utf8 mapping run unchanged at one Oracle source or are refused before credentials open | no local lexical answer is returned; source scalar syntax still belongs to Oracle |
//! | Cube text policies run locally before query operators because its SQL planner lacks regex replacement | scans still run at Cube; raw values enter this process and existing row, memory and deadline caps apply |
//! | Cube joins run locally, including joins over one source | supported child plans still run at Cube; extra source rows can exhaust the existing per-leg row cap, working set or deadline |
//! | Cube scalar result fields of UInt16, UInt32 or UInt64 are refused before credentials open | its pinned wire serializer narrows them to signed widths; the guard reads planned output types, not actual engine values, and establishes no general numeric conformance |
//! | a statement that reads no table is refused | there is no source to run it at under the caller |
//! | unquoted identifiers fold per source | Oracle names fold up, other sources down; quoted metadata and SQL aliases remain exact |
//! | the cut runs after every default rule | `SessionStateBuilder::with_optimizer_rule` appends |

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Instant;

use datafusion::arrow::array::RecordBatch;
use datafusion::arrow::compute::{CastOptions, cast_with_options};
use datafusion::arrow::datatypes::{Schema, SchemaRef};
use datafusion::common::tree_node::TreeNodeRecursion;
use datafusion::common::{DataFusionError, TableReference, plan_datafusion_err, plan_err};
use datafusion::datasource::memory::{MemoryCatalogProvider, MemorySchemaProvider};
use datafusion::error::Result as Planned;
use datafusion::execution::{SessionStateBuilder, SessionStateDefaults};
use datafusion::logical_expr::{Expr, LogicalPlan, Projection, ScalarUDF};
use datafusion::optimizer::Optimizer;
use datafusion::prelude::{SQLOptions, SessionConfig, SessionContext};
use datafusion::sql::parser::Statement as EngineStatement;
use datafusion::sql::sqlparser::dialect::GenericDialect;
use datafusion::sql::sqlparser::keywords::Keyword;
use datafusion::sql::sqlparser::parser::Parser;
use datafusion::sql::sqlparser::tokenizer::{Token, Tokenizer, Word};
use datafusion_federation::{FederatedQueryPlanner, FederatedTableProviderAdaptor, FederationOptimizerRule};
use sutura_domain::model::{ProjectName, QualifiedTable, SourceName};
use sutura_domain::plan::{StatementLegs, StatementQuestion, StatementStopped, StatementTable};
use sutura_domain::raw::{RawStatement, SqlConstruct};
use sutura_domain::warehouse::RawRows;
use sutura_domain::warehouse::arrow::of_rows;
use sutura_domain::warehouse::deadline::Deadline;
use sutura_domain::warehouse::schema::TypeDialect;
use tokio::runtime::Runtime;

use super::{CombineError, DataFusionCombiner, collected, working_set_of};
use crate::pool::{self, WorkingSet};

/// `T`, planned, alongside why a raw prefilter could not push it down - never both at once.
type UnparsedWith<T> = Planned<(T, Option<sutura_domain::catalog::PrefilterIneligible>)>;

mod cut;
mod literal;
mod oracle;
mod projection;
mod pushdown;
mod resolve;
mod rewrite;
mod text;
mod text_prefilter;
mod unparse;
mod unparse_error;

use cut::{Cutter, Fetched, Source, Table, Yielding};
use unparse::written;
use unparse_error::{field_not_found, unprojected_agg, unquotable};

/// Answers `question`: its statement planned over its tables, each source's part run by `legs`.
///
/// `prefixed` is computed once, by this function's own tracked body, and read only after it
/// returns: a `StatementStopped::NotPlanned` it produced then reads as
/// `StatementStopped::SourcePrefixRefused` instead whenever any reference resolved only by naming
/// its source explicitly, so a planning failure past that point can never fall back to the
/// caller's own, not-admission-checked text against a single source.
///
/// `answer_tracked` returns rows from one of three exits - the whole statement, a federated
/// combine, or the numeric Oracle path - and any of them can still carry a collision's mangled
/// registration leaf in a column's own name, baked in by an unaliased computed column's
/// auto-generated display text rather than kept as separate, rewritable relation metadata. This is
/// the one place that leaf is swept from a row answer's column labels, read after `answer_tracked`
/// has already picked whichever exit it took.
pub(super) fn answer<L>(combiner: &DataFusionCombiner, question: StatementQuestion<'_>, legs: &mut L) -> Answered<L::Stop>
where
    L: StatementLegs,
{
    let renames = rewrite::renames_of(question.tables());
    let mut prefixed = false;
    answer_tracked(combiner, question, legs, &mut prefixed)
        .map(|rows| unmangled_rows(rows, &renames))
        .map_err(|stopped| {
            if prefixed && matches!(stopped, StatementStopped::NotPlanned) {
                StatementStopped::SourcePrefixRefused
            } else {
                stopped
            }
        })
}

/// `rows`' own column labels, with every mangled registration leaf `renames` carries replaced by
/// the physical name it mangles.
fn unmangled_rows(rows: RawRows, renames: &[(String, TableReference)]) -> RawRows {
    if renames.is_empty() {
        return rows;
    }
    let (columns, rows) = rows.into_parts();
    let columns = columns
        .into_iter()
        .map(|name| rewrite::unmangled_name(&name, renames))
        .collect();
    RawRows::of(columns, rows)
}

fn answer_tracked<L>(
    combiner: &DataFusionCombiner,
    question: StatementQuestion<'_>,
    legs: &mut L,
    prefixed: &mut bool,
) -> Answered<L::Stop>
where
    L: StatementLegs,
{
    let runtime = combiner.runtime().map_err(|cause| unplanned("no runtime", &cause))?;
    let working_set = working_set_of(question.working_set_bytes());
    let mut legs = projection::Guarded::new(legs);
    let fetched = Arc::new(Fetched::default());
    let tables = registered_tables(question.tables()).map_err(|cause| unplanned("no admitted schema", &cause))?;
    let context = session(&tables, &fetched, working_set).map_err(|cause| unplanned("no session", &cause))?;
    let (plan, numeric_operations, native_schema) = within(
        runtime,
        question.deadline(),
        planned(&context, question.statement(), &tables, prefixed),
    )
    .ok_or(StatementStopped::DeadlineExceeded)?
    .map_err(|cause| match cause {
        PlanningStopped::Refused(refused) => stopped(refused),
        PlanningStopped::Engine(cause) if unquotable(&cause) => stopped(resolve::Refused::UnquotableName),
        PlanningStopped::Engine(cause) => {
            let fallback = unplanned("the statement did not plan", &cause);
            if field_not_found(&cause) {
                StatementStopped::UnknownColumn
            } else {
                fallback
            }
        }
    })?;
    let parts = cut::parts(&plan, &tables).map_err(|cause| unplanned("the plan did not cut", &cause))?;
    let sources: BTreeSet<SourceName> = parts.iter().map(|part| part.source.clone()).collect();
    if sources.len() > 1 && !question.federation_allowed() {
        return Err(StatementStopped::SpansMoreThanOneSource);
    }
    if numeric_operations {
        let Some(source) = sources.first().filter(|_| sources.len() == 1) else {
            return Err(StatementStopped::UnsupportedSyntax(SqlConstruct::NumberText));
        };
        let native_tables: Vec<_> = tables.iter().filter(|table| table.source() == source).cloned().collect();
        if native_tables.iter().any(|table| table.dialect() != TypeDialect::Oracle) {
            return Err(StatementStopped::UnsupportedSyntax(SqlConstruct::NumberText));
        }
        // Re-resolve the admitted input against only what the validated plan actually scanned at
        // this source, not every table merely admitted there: optimization can prune a reference
        // whole (a dead common table expression, a second source), and an admitted-but-unscanned
        // table must never be re-admitted through this second, source-only pass.
        let scanned: BTreeSet<QualifiedTable> = parts
            .iter()
            .filter(|part| &part.source == source)
            .map(|part| part.scan_tables(&tables))
            .collect::<Planned<Vec<_>>>()
            .map_err(|cause| unplanned("a part's scan did not resolve", &cause))?
            .into_iter()
            .flatten()
            .collect();
        let native_tables: Vec<_> = native_tables
            .into_iter()
            .filter(|table| scanned.contains(table.table()))
            .collect();
        let mut native = Parser::parse_sql(&GenericDialect, question.statement().as_str())
            .map_err(|cause| unplanned("the numeric statement did not parse", &cause))?;
        let [native] = native.as_mut_slice() else {
            return Err(StatementStopped::UnsupportedSyntax(SqlConstruct::NumberText));
        };
        let (scans, native_prefixed) = resolve::explicit_statement(native, &native_tables)
            .map_err(|_cause| StatementStopped::UnsupportedSyntax(SqlConstruct::NumberText))?;
        *prefixed |= native_prefixed;
        let read: Vec<_> = native_tables
            .into_iter()
            .filter(|table| scans.contains(table.table()))
            .collect();
        let ineligible = oracle::native(native)
            .and_then(|()| text::apply_to(native, &read, TypeDialect::Oracle))
            .map_err(|cause| unwritten("the numeric statement did not unparse", &cause))?;
        let text = if read.iter().any(text::oracle_lobs) || projection::duplicate_names(&native_schema) {
            text::native_oracle_rows(native, &native_schema, &read)
        } else {
            literal::spelled(native, TypeDialect::Oracle)
        }
        .map_err(|cause| unwritten("the numeric statement did not unparse", &cause))?;
        // The caller's own names were read at its tokens; this reads those the resolver wrote.
        if !literal::quotable_names(&text, TypeDialect::Oracle) {
            return Err(stopped(resolve::Refused::UnquotableName));
        }
        let native = RawStatement::parse(text).map_err(|cause| unplanned("the numeric statement was not bounded", &cause))?;
        legs.open(&sources, [(&native, TypeDialect::Oracle)])?;
        let rows = legs.run_planned(source, &native, &scans, cut::policy_of(&parts), ineligible)?;
        return projection::output(&native_schema, rows)
            .map_err(|cause| unplanned("the numeric source output did not fit", &cause));
    }
    // A bare marked scan still needs fitting to its declared text representation.
    let whole = cut::part_of(&plan).is_some_and(|part| !cut::number_text(part.plan().schema().inner()));
    let statements = parts
        .iter()
        .map(|part| {
            let (statement, ineligible) = part.statement(whole, &tables, &native_schema)?;
            Ok((statement, ineligible, part.scan_tables(&tables)?))
        })
        .collect::<Planned<Vec<_>>>()
        .map_err(|cause| unwritten("a part did not unparse", &cause))?;
    legs.open(
        &sources,
        parts
            .iter()
            .zip(&statements)
            .map(|(part, (statement, _ineligible, _tables))| (statement, part.dialect)),
    )?;
    let mut leaves = Vec::with_capacity(parts.len());
    for (part, (statement, ineligible, scan_tables)) in parts.into_iter().zip(&statements) {
        let rows = legs
            .run_planned(&part.source, statement, scan_tables, part.policy, *ineligible)
            .inspect_err(|stopped| {
                if matches!(stopped, StatementStopped::Leg(_)) {
                    tracing::debug!(source = %part.source, statement = statement.as_str(), "run_sql: a source part did not run");
                }
            })?;
        if whole {
            // `part.plan`'s own schema still carries a collision's mangled registration leaf; an
            // unaliased computed column's auto-generated name bakes that qualifier into the field
            // name itself, so the caller must read it off the rewritten plan, the same one its text
            // was unparsed from, rather than the plan as planned.
            let physical = rewrite::physically_qualified(&part.plan, &tables)
                .map_err(|cause| unplanned("the source output did not rewrite", &cause))?;
            return projection::output(physical.schema().inner(), rows)
                .map_err(|cause| unplanned("the source output did not fit", &cause));
        }
        let batches =
            fitted(&rows, part.plan.schema().inner()).map_err(|cause| unplanned("a leg did not fit its part", &cause))?;
        leaves.push((part.plan, batches));
    }
    if fetched.set(leaves).is_err() {
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

/// One caller-scoped registration view, shared by the session, resolver and source-part lookup.
///
/// Distinct by `(source, table)`, not by the physical path alone: a path two listed sources both
/// carry is kept once per source, each still reachable through that source's `session` catalog slot
/// via [`resolve::registered_leaf`]'s mangled registration - only a model sharing both its table and
/// its source merges here, exactly as before.
fn registered_tables(tables: &[StatementTable]) -> Planned<Vec<StatementTable>> {
    let mut registered = BTreeSet::new();
    let mut admitted = Vec::new();
    for table in tables {
        if !registered.insert((table.source(), table.table())) {
            continue;
        }
        let visible = || {
            tables
                .iter()
                .filter(|other| other.table() == table.table() && other.source() == table.source())
        };
        if visible().any(|other| other.dialect() != table.dialect()) {
            return plan_err!("models of one physical table disagree on its dialect");
        }
        for first in visible() {
            for second in visible() {
                for field in first.schema().fields() {
                    if let Ok(other) = second.schema().field_with_name(field.name())
                        && field.metadata().get("sutura:text_policy") != other.metadata().get("sutura:text_policy")
                    {
                        return plan_err!("models disagree on a physical column's text policy");
                    }
                }
            }
        }
        let columns = Schema::try_merge(visible().map(|other| other.schema().as_ref().clone()))?;
        admitted.push(StatementTable::new(
            table.source().clone(),
            table.table().clone(),
            table.dialect(),
            Arc::new(columns),
        ));
    }
    Ok(admitted)
}

/// The call's session over its normalized tables, with no information schema or table function.
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
    for table in tables {
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
            cut: Arc::new(Optimizer::with_rules(vec![Arc::new(Cutter(
                Arc::clone(fetched),
                table.dialect(),
                tables.iter().any(|other| {
                    other.source() == table.source() && other.dialect() == TypeDialect::Cube && text::protected(other)
                }),
            ))])),
        });
        let provider = FederatedTableProviderAdaptor::new(Arc::new(Table {
            source,
            schema: Arc::clone(table.schema()),
        }));
        schema.register_table(resolve::registered_leaf(tables, table), Arc::new(provider))?;
    }
    Ok(context)
}

/// The scalar functions a statement may call: `datafusion`'s defaults less each one whose output a
/// caller can grow past a constant factor of its input in one call, because a constant call is
/// folded while planning, outside the pool. `repeat`, `lpad` and `rpad` take the length as an
/// argument; `replace` and `regexp_replace` insert at every position of an empty pattern; `encode`
/// doubles per nesting; `arrow_cast` and `arrow_try_cast` spell a type, and a null fixed-size list
/// of `i32::MAX` items is reserved whole; `with_metadata` writes the `sutura:` field metadata the
/// text bound reads, so a caller could mark a LOB as unbounded or free of one. A function a later
/// `datafusion` adds is out until named.
const ADMITTED: &str = "
    arrow_field arrow_metadata arrow_typeof cast_to_type coalesce file_row_index get_field greatest input_file_name least
    named_struct nullif nvl nvl2 overlay struct try_cast_to_type union_extract union_tag version
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
/// a cell measures that a constant chain just under it plans on a test thread's stack in the test
/// profile, whose dependencies are optimised further than a shipped build's. The constant folds while
/// planning; a chain over a column, which a part's text carries to its source, is not measured. A
/// long statement of many keywords, commas or qualified names is refused though it is shallow.
const MOST_OPERATORS: usize = 1024;

/// `statement`'s optimized plan, refused unless it is one query of at most [`MOST_OPERATORS`].
///
/// `prefixed` is set once resolution matches any reference only by naming its source explicitly,
/// even when a later step in this function still fails - the caller must know that before it
/// decides whether a planning failure may fall back to the statement's own text.
async fn planned(
    context: &SessionContext,
    statement: &RawStatement,
    tables: &[StatementTable],
    prefixed: &mut bool,
) -> Result<(LogicalPlan, bool, SchemaRef), PlanningStopped> {
    let tokens = Tokenizer::new(&GenericDialect {}, statement.as_str())
        .tokenize()
        .map_err(|cause| {
            tracing::debug!(%cause, "run_sql: the statement did not tokenize");
            PlanningStopped::Refused(resolve::Refused::Unsupported(SqlConstruct::Query))
        })?;
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
    if tokens
        .iter()
        .any(|token| matches!(token, Token::Word(Word { quote_style: Some(_), value, .. }) if !literal::quotable(value)))
    {
        return Err(PlanningStopped::Refused(resolve::Refused::UnquotableName));
    }
    if operators > MOST_OPERATORS {
        return Err(PlanningStopped::Engine(plan_datafusion_err!(
            "the statement has {operators} operators, more than {MOST_OPERATORS}"
        )));
    }
    let mut statements = Parser::new(&GenericDialect {})
        .with_tokens(tokens)
        .parse_statements()
        .map_err(|cause| {
            tracing::debug!(%cause, "run_sql: the statement did not parse");
            PlanningStopped::Refused(resolve::Refused::Unsupported(SqlConstruct::Query))
        })?;
    let [statement] = statements.as_mut_slice() else {
        return Err(PlanningStopped::Refused(resolve::Refused::Unsupported(SqlConstruct::Query)));
    };
    let (_, statement_prefixed) = resolve::statement(statement, tables).map_err(PlanningStopped::Refused)?;
    *prefixed = statement_prefixed;
    let Some(statement) = statements.pop() else {
        return Err(PlanningStopped::Refused(resolve::Refused::Unsupported(SqlConstruct::Query)));
    };
    let options = SQLOptions::new()
        .with_allow_ddl(false)
        .with_allow_dml(false)
        .with_allow_statements(false);
    let local: Vec<_> = tables
        .iter()
        .filter(|table| table.dialect() == TypeDialect::Cube && text::protected(table))
        .cloned()
        .collect();
    // Cube's text policies rewrite the resolved statement itself; no text is parsed a second time.
    let policy = (!local.is_empty()).then(|| statement.clone());
    let plan = context
        .state()
        .statement_to_plan(EngineStatement::Statement(Box::new(statement)))
        .await?;
    options.verify_plan(&plan)?;
    let mut numeric_operations = false;
    plan.apply_with_subqueries(|node| {
        numeric_operations |= !cut::number_text_safe(node)?;
        Ok(TreeNodeRecursion::Continue)
    })?;
    // Validate caller functions before registering the policy-only regex replacement.
    let plan = if let Some(mut policy) = policy {
        let replace = SessionStateDefaults::default_scalar_functions()
            .into_iter()
            .find(|function| function.name() == "regexp_replace")
            .ok_or_else(|| plan_datafusion_err!("the text policy function is unavailable"))?;
        context.register_udf(replace.as_ref().clone());
        text::cube_local_to(&mut policy, &local)?;
        context
            .state()
            .statement_to_plan(EngineStatement::Statement(Box::new(policy)))
            .await?
    } else {
        plan
    };
    let plan = if numeric_operations && tables.iter().any(text::oracle_lobs) {
        text::native_number_outputs(plan)?
    } else {
        plan
    };
    let plan = if tables.iter().any(text::oracle_lobs) {
        text::oracle_lob_outputs(plan)?
    } else {
        plan
    };
    // Whole-source outputs keep their markers even when an optimizer rule drops field metadata.
    let native_schema = Arc::clone(plan.schema().inner());
    let frame = context.execute_logical_plan(plan).await?;
    Ok((frame.into_optimized_plan()?, numeric_operations, native_schema))
}

/// What the caller reads for a statement refused before planning.
const fn stopped<S>(refused: resolve::Refused) -> StatementStopped<S> {
    match refused {
        resolve::Refused::UnknownTable => StatementStopped::UnknownTable,
        resolve::Refused::UnknownColumn => StatementStopped::UnknownColumn,
        resolve::Refused::Unsupported(construct) => StatementStopped::UnsupportedSyntax(construct),
        resolve::Refused::UnquotableName => StatementStopped::UnsupportedSyntax(SqlConstruct::Query),
    }
}

#[derive(Debug, thiserror::Error)]
enum PlanningStopped {
    #[error("the admitted statement did not plan")]
    Engine(#[from] DataFusionError),
    #[error("the statement was refused before planning: {0:?}")]
    Refused(resolve::Refused),
}

/// Why a source statement was not written: a name it could not quote, the unparser's own gap
/// against one of its aggregate rewrites, or else `why`.
fn unwritten<S>(why: &str, cause: &DataFusionError) -> StatementStopped<S> {
    if unquotable(cause) {
        stopped(resolve::Refused::UnquotableName)
    } else if let Some(column) = unprojected_agg(cause) {
        tracing::debug!(column, "run_sql: the unparser could not name an aggregate rewrite's column");
        StatementStopped::UnsupportedSyntax(SqlConstruct::AggregateRewrite)
    } else {
        unplanned(why, cause)
    }
}

/// `part` with output columns labelled `c0..cn`, adding a projection when the cut owns an alias or filter.
fn labelled(part: &LogicalPlan) -> Planned<LogicalPlan> {
    let (expressions, input) = match part {
        LogicalPlan::Projection(projection) => (projection.expr.clone(), Arc::clone(&projection.input)),
        _ => (
            part.schema().columns().into_iter().map(Expr::Column).collect(),
            Arc::new(part.clone()),
        ),
    };
    let columns = expressions
        .into_iter()
        .enumerate()
        .map(|(index, column)| column.unalias().alias(label(index)))
        .collect();
    Ok(LogicalPlan::Projection(Projection::try_new(columns, input)?))
}

fn label(index: usize) -> String {
    format!("c{index}")
}

/// `plan` written as a part over tables with no text policy is.
#[cfg(test)]
fn unparsed(plan: &LogicalPlan, dialect: TypeDialect) -> Planned<String> {
    written(plan, dialect, &[]).map(|(text, _ineligible)| text)
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

/// The caller reads only that the statement did not plan, and the log at info only which step
/// stopped it. The engine's words can name a column or quote a value a leg read, so they are written
/// at debug, which a deployment that logs at debug still writes.
///
/// [`answer`]'s own wrapper reads this as `StatementStopped::SourcePrefixRefused` instead, once any
/// reference in the statement resolved only by naming its source explicitly: a reference that
/// resolved that way must never let the application fall back to the caller's original,
/// not-admission-checked text against a single source.
fn unplanned<S>(why: &str, cause: &dyn core::fmt::Display) -> StatementStopped<S> {
    tracing::info!("run_sql: {why}");
    tracing::debug!(%cause, "run_sql: {why}");
    StatementStopped::NotPlanned
}

/// Declared here rather than in `combine::tests`, because this file is new: the base run removes it,
/// and with it every cell below, instead of compiling cells over a port it does not have.
#[cfg(test)]
mod tests;
