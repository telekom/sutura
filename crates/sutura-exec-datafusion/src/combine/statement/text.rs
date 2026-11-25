//! Source-side views: every operator above a table sees the same transformed text.

use std::collections::BTreeMap;
use std::ops::ControlFlow;
use std::sync::Arc;

use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::common::metadata::FieldMetadata;
use datafusion::common::tree_node::{Transformed, TreeNode as _, TreeNodeRecursion};
use datafusion::common::{DFSchema, ExprSchema as _, plan_datafusion_err, plan_err};
use datafusion::error::{DataFusionError, Result};
use datafusion::functions_aggregate::min_max::{Max, Min};
use datafusion::logical_expr::utils::grouping_set_to_exprlist;
use datafusion::logical_expr::{Distinct, Expr as PlannedExpr, ExprSchemable as _, LogicalPlan};
use datafusion::sql::sqlparser::ast::{Expr, Ident, SetExpr, Statement, TableFactor, VisitMut as _, VisitorMut};
use datafusion::sql::sqlparser::dialect::GenericDialect;
use datafusion::sql::sqlparser::parser::Parser;
use datafusion::sql::unparser::UnquotableName;
use sutura_domain::catalog::TextPolicy;
use sutura_domain::plan::StatementTable;
use sutura_domain::warehouse::schema::TypeDialect;

use super::literal::{literal, quotable_table, spelled};
use super::text_prefilter::Prefilter;

pub(super) const KEY: &str = "sutura:text_policy";
const LOB_KEY: &str = "sutura:oracle_lob";
/// On a field that derives from no LOB; written by `oracle_lob_outputs` alone, and read back only
/// where `derived` has already dropped every `sutura:` key a function computed.
const FREE_KEY: &str = "sutura:oracle_lob_free";

pub(super) fn oracle_lobs(table: &StatementTable) -> bool {
    table.dialect() == TypeDialect::Oracle
        && table
            .schema()
            .fields()
            .iter()
            .any(|field| field.metadata().contains_key(LOB_KEY))
}

/// MIN/MAX keep the source NUMBER type, although their Arrow Utf8 field loses its marker.
/// Carry it through typed aggregate arguments before parent schemas are rebuilt.
pub(super) fn native_number_outputs(plan: LogicalPlan) -> Result<LogicalPlan> {
    plan.transform_up_with_subqueries(|mut node| {
        if let LogicalPlan::Aggregate(aggregate) = &mut node {
            let input = aggregate.input.schema();
            aggregate.aggr_expr = std::mem::take(&mut aggregate.aggr_expr)
                .into_iter()
                .map(|expression| {
                    let aggregate = match &expression {
                        PlannedExpr::Alias(alias) => alias.expr.as_ref(),
                        other => other,
                    };
                    let PlannedExpr::AggregateFunction(function) = aggregate else {
                        return Ok(expression);
                    };
                    let builtin = function.func.inner();
                    if builtin.downcast_ref::<Min>().is_none() && builtin.downcast_ref::<Max>().is_none() {
                        return Ok(expression);
                    }
                    let [argument] = function.params.args.as_slice() else {
                        return Ok(expression);
                    };
                    let metadata = argument.metadata(input.as_ref())?;
                    if metadata.inner().get("sutura:unbounded_number").map(String::as_str) != Some("utf8") {
                        return Ok(expression);
                    }
                    Ok(match expression {
                        PlannedExpr::Alias(alias) => PlannedExpr::Alias(alias.with_metadata(Some(metadata))),
                        other => {
                            let name = other.schema_name().to_string();
                            other.alias_with_metadata(name, Some(metadata))
                        }
                    })
                })
                .collect::<Result<_>>()?;
        }
        node.recompute_schema().map(Transformed::yes)
    })
    .map(|transformed| transformed.data)
}

/// What a value is known to be derived from. Only a positive proof narrows it: anything this pass
/// cannot trace is `Unknown`, and an unknown text output is bounded as a LOB would be.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Lineage {
    Free,
    Clob,
    Nclob,
    Unknown,
}

impl Lineage {
    /// The widest of two lineages: `Free < Clob < Nclob < Unknown`.
    fn join(self, other: Self) -> Self {
        self.max(other)
    }
}

const fn is_text(data_type: &DataType) -> bool {
    matches!(data_type, DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View)
}

fn field_lineage(field: &Field) -> Lineage {
    let metadata = field.metadata();
    match metadata.get(LOB_KEY).map(String::as_str) {
        Some("clob") => Lineage::Clob,
        Some("nclob") => Lineage::Nclob,
        None if metadata.contains_key(FREE_KEY) => Lineage::Free,
        _ => Lineage::Unknown,
    }
}

/// The marks `lineage` leaves on a field's metadata: none of its own for `Unknown`.
fn marked(mut metadata: BTreeMap<String, String>, lineage: Lineage) -> BTreeMap<String, String> {
    drop(metadata.remove(LOB_KEY));
    drop(metadata.remove(FREE_KEY));
    let mark = match lineage {
        Lineage::Free => Some((FREE_KEY, "true")),
        Lineage::Clob => Some((LOB_KEY, "clob")),
        Lineage::Nclob => Some((LOB_KEY, "nclob")),
        Lineage::Unknown => None,
    };
    if let Some((key, value)) = mark {
        drop(metadata.insert(key.to_owned(), value.to_owned()));
    }
    metadata
}

/// What `expression` leaves on its output field: the metadata the planner derived, less every
/// `sutura:` key where a function computed the value, since no function of the admitted set writes
/// one and a mark a function carried is not this pass's; then the marks its lineage proves.
fn derived(expression: &PlannedExpr, schema: &DFSchema, metadata: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut metadata = metadata.clone();
    let computed = expression
        .exists(|part| {
            Ok(matches!(
                part,
                PlannedExpr::ScalarFunction(_) | PlannedExpr::WindowFunction(_)
            ))
        })
        .unwrap_or(true);
    if computed {
        metadata.retain(|key, _| !key.starts_with("sutura:"));
    }
    marked(metadata, lineage(expression, schema))
}

/// `schema` with the metadata of field `index` replaced by what `edit` answers for it, if it does.
fn edited(schema: &DFSchema, edit: impl Fn(usize, &Field) -> Option<BTreeMap<String, String>>) -> Result<Arc<DFSchema>> {
    let fields = schema
        .iter()
        .enumerate()
        .map(|(index, (qualifier, field))| {
            let field = edit(index, field).map_or_else(
                || Arc::clone(field),
                |metadata| Arc::new(field.as_ref().clone().with_metadata(metadata.into_iter().collect())),
            );
            (qualifier.cloned(), field)
        })
        .collect();
    let rebuilt = DFSchema::new_with_metadata(fields, DFSchema::metadata(schema).clone())?
        .with_functional_dependencies(schema.functional_dependencies().clone())?;
    Ok(Arc::new(rebuilt))
}

fn metadata_of(field: &Field) -> BTreeMap<String, String> {
    field
        .metadata()
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Carry LOB lineage through every output before optimization drops metadata, so a text output
/// derived from a LOB, however many operators or numeric steps away, is bounded.
///
/// Every computed value inherits the join of all its operands' lineage, conditions and orderings
/// included; the only `Free` leaves are a literal and a column of a scanned table that is no LOB.
/// An output this pass cannot trace stays unmarked, which the output bound reads as a LOB.
pub(super) fn oracle_lob_outputs(plan: LogicalPlan) -> Result<LogicalPlan> {
    plan.transform_up_with_subqueries(|mut node| {
        if let LogicalPlan::TableScan(scan) = &mut node {
            scan.projected_schema = edited(&scan.projected_schema, |_, field| {
                (field_lineage(field) == Lineage::Unknown).then(|| marked(metadata_of(field), Lineage::Free))
            })?;
        }
        let outputs = match &mut node {
            LogicalPlan::Projection(projection) => Some((vec![&mut projection.expr], projection.input.as_ref())),
            LogicalPlan::Aggregate(aggregate) => Some((
                vec![&mut aggregate.group_expr, &mut aggregate.aggr_expr],
                aggregate.input.as_ref(),
            )),
            LogicalPlan::Window(window) => Some((vec![&mut window.window_expr], window.input.as_ref())),
            LogicalPlan::Distinct(Distinct::On(distinct)) => Some((vec![&mut distinct.select_expr], distinct.input.as_ref())),
            _ => None,
        };
        if let Some((expressions, input)) = outputs {
            for expressions in expressions {
                *expressions = std::mem::take(expressions)
                    .into_iter()
                    .map(|expression| {
                        let schema = input.schema().as_ref();
                        let Ok(metadata) = expression.metadata(schema) else {
                            return Ok(expression);
                        };
                        if matches!(expression, PlannedExpr::GroupingSet(_)) {
                            return Ok(expression);
                        }
                        let wanted = derived(&expression, schema, metadata.inner());
                        if &wanted == metadata.inner() {
                            return Ok(expression);
                        }
                        let metadata = FieldMetadata::new(wanted);
                        Ok(match expression {
                            PlannedExpr::Alias(alias) => PlannedExpr::Alias(alias.with_metadata(Some(metadata))),
                            other => {
                                let name = other.schema_name().to_string();
                                other.alias_with_metadata(name, Some(metadata))
                            }
                        })
                    })
                    .collect::<Result<_>>()?;
            }
        }
        let mut node = node.recompute_schema()?;
        match &mut node {
            // A grouping set's expressions cannot carry an alias, so its output fields are marked directly.
            LogicalPlan::Aggregate(aggregate)
                if aggregate
                    .group_expr
                    .iter()
                    .any(|group| matches!(group, PlannedExpr::GroupingSet(_))) =>
            {
                let input = aggregate.input.schema();
                let grouped = grouping_set_to_exprlist(&aggregate.group_expr)?;
                aggregate.schema = edited(&aggregate.schema, |index, field| {
                    grouped
                        .get(index)
                        .map(|expression| derived(expression, input.as_ref(), &metadata_of(field)))
                })?;
            }
            LogicalPlan::Union(union) => {
                let inputs = &union.inputs;
                union.schema = edited(&union.schema, |index, field| {
                    let lineage = inputs
                        .iter()
                        .map(|input| field_lineage(input.schema().field(index)))
                        .fold(Lineage::Free, Lineage::join);
                    Some(marked(metadata_of(field), lineage))
                })?;
            }
            _ => {}
        }
        Ok(Transformed::yes(node))
    })
    .map(|transformed| transformed.data)
}

/// The lineage of `expression`'s value in `schema`. A literal is free; a column is what its field's
/// mark says; any other expression is the join of every operand it reads, whatever their types and
/// whether they select or supply the value; and one with no operand to trace is unknown.
fn lineage(expression: &PlannedExpr, schema: &DFSchema) -> Lineage {
    match expression {
        PlannedExpr::Literal(..) => Lineage::Free,
        PlannedExpr::Column(column) => schema
            .field_from_column(column)
            .map_or(Lineage::Unknown, |field| field_lineage(field)),
        // A scalar subquery is what its own plan's output field was marked, which this pass did
        // before its parent; an unmarked field is unknown, as is a column of the query around it.
        PlannedExpr::ScalarSubquery(subquery) => subquery
            .subquery
            .schema()
            .fields()
            .first()
            .map_or(Lineage::Unknown, |field| field_lineage(field)),
        PlannedExpr::OuterReferenceColumn(..) | PlannedExpr::Exists(_) | PlannedExpr::InSubquery(_) => Lineage::Unknown,
        // A call with no argument reads nothing.
        PlannedExpr::ScalarFunction(function) if function.args.is_empty() => Lineage::Free,
        other => {
            let mut operands = Vec::new();
            if other
                .apply_children(|child| {
                    operands.push(child);
                    Ok(TreeNodeRecursion::Continue)
                })
                .is_err()
                || operands.is_empty()
            {
                return Lineage::Unknown;
            }
            operands
                .into_iter()
                .fold(Lineage::Free, |joined, operand| joined.join(lineage(operand, schema)))
        }
    }
}

/// A LOB is redacted at the source without clipping. Only a whole-source answer may return it:
/// clipping a federation leaf would make a later local predicate silently search a prefix.
pub(super) fn bounded_oracle_rows(sql: &str, schema: &Schema, tables: &[StatementTable]) -> Result<String> {
    let parsed = Parser::parse_sql(&GenericDialect, sql)?;
    let ordered = matches!(parsed.first(), Some(Statement::Query(query)) if query.order_by.is_some());
    bounded_oracle_output(sql, schema, tables, ordered)
}

/// Label the caller's original query with a CTE column list: its numeric expressions, aliases,
/// and set operations stay native. Existing CTEs remain at the top level for Oracle.
pub(super) fn native_oracle_rows(statement: &mut Statement, schema: &Schema, tables: &[StatementTable]) -> Result<String> {
    let Statement::Query(query) = statement else {
        return plan_err!("expected one native Oracle query");
    };
    let ordered = query.order_by.is_some();
    let existing = match query.with.take() {
        Some(mut with) => format!("{}, ", spelled(&mut with, TypeDialect::Oracle)?),
        None => String::from("WITH "),
    };
    let query = spelled(query.as_mut(), TypeDialect::Oracle)?;
    let mut name = String::from("sutura_lob_values");
    let folded = format!("{existing}{query}").to_ascii_lowercase();
    while folded.contains(&name) {
        name.push('_');
    }
    let labels = (0..schema.fields().len())
        .map(|index| format!("\"c{index}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let labelled = format!("SELECT {labels} FROM \"{name}\"");
    let bounded = bounded_oracle_output(&labelled, schema, tables, ordered)?;
    Ok(format!("{existing}\"{name}\"({labels}) AS ({query}) {bounded}"))
}

fn bounded_oracle_output(sql: &str, schema: &Schema, tables: &[StatementTable], ordered: bool) -> Result<String> {
    let mut bytes = 4000;
    for field in tables
        .iter()
        .filter(|table| oracle_lobs(table))
        .flat_map(|table| table.schema().fields())
    {
        if let Some(encoded) = field.metadata().get(KEY) {
            let policy: TextPolicy =
                serde_json::from_str(encoded).map_err(|cause| plan_datafusion_err!("invalid pinned text policy: {cause}"))?;
            if let Some(bound) = policy.max_text_bytes() {
                bytes = bytes.min(u32::from(bound));
            }
        }
    }
    let national = tables
        .iter()
        .filter(|table| oracle_lobs(table))
        .flat_map(|table| table.schema().fields())
        .any(|field| field.metadata().get(LOB_KEY).is_some_and(|kind| kind == "nclob"));
    let lobs = tables.iter().any(oracle_lobs);
    let columns = schema
        .fields()
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let column = format!("\"sutura_lob_output\".\"c{index}\"");
            let value = match lobs.then(|| output_ceiling(field, bytes, national)).flatten() {
                Some(ceiling) => bounded_lob(&column, ceiling),
                None => column,
            };
            // A label, not the planned name: `projection::output` restores the names from the schema.
            format!("{value} AS \"c{index}\"")
        })
        .collect::<Vec<_>>()
        .join(", ");
    if ordered {
        // ROWNUM pins the inner order before the output expressions run; the outer ORDER BY
        // restores that order without sorting on a clipped value or returning the ordinal.
        let inner = (0..schema.fields().len())
            .map(|index| format!("\"sutura_lob_inner\".\"c{index}\""))
            .collect::<Vec<_>>()
            .join(", ");
        Ok(format!(
            "SELECT {columns} FROM (SELECT {inner}, ROWNUM AS \"sutura_lob_ordinal\" FROM ({sql}) \"sutura_lob_inner\") \"sutura_lob_output\" ORDER BY \"sutura_lob_output\".\"sutura_lob_ordinal\""
        ))
    } else {
        Ok(format!("SELECT {columns} FROM ({sql}) \"sutura_lob_output\""))
    }
}

/// The byte ceiling of a text output of a LOB-bearing part, or `None` where the output is proven
/// to derive from no LOB. An output whose lineage is unknown is bounded, under the national ceiling
/// where any LOB of the part is a NCLOB. A `sutura:unbounded_number` key does not lift the bound:
/// only a proven-free lineage does.
pub(super) fn output_ceiling(field: &Field, bytes: u32, national: bool) -> Option<u32> {
    if !is_text(field.data_type()) {
        return None;
    }
    match field_lineage(field) {
        Lineage::Free => None,
        Lineage::Clob => Some(bytes),
        Lineage::Nclob => Some(bytes.min(2000)),
        Lineage::Unknown => Some(if national { bytes.min(2000) } else { bytes }),
    }
}

fn bounded_lob(value: &str, bytes: u32) -> String {
    let payload = bytes - 14;
    // Enumerate literals, not a linguistic range. A large ASCII window is safe even when the
    // database's character set is multibyte; other text keeps the four-byte UTF-8 reservation.
    let alphabet: String = (b' '..=b'~')
        .filter(|byte| !matches!(byte, b']' | b'-' | b'\\'))
        .map(char::from)
        .collect();
    // Preserve the ASCII semicolon without triggering the raw reader's delimiter refusal.
    let pattern = literal(&format!("^[]\t\r\n{alphabet}\\\\-]*$"), TypeDialect::Oracle).replace(';', "' || CHR(59) || '");
    let chars = format!(
        "CASE WHEN REGEXP_LIKE(SUBSTR({value}, 1, {payload}), {pattern}, 'c') THEN {payload} ELSE {} END",
        payload.div_euclid(4)
    );
    format!(
        "CASE WHEN {value} IS NULL THEN NULL ELSE DBMS_LOB.\"SUBSTR\"({value}, {chars}, 1) || CASE WHEN LENGTH({value}) > {chars} THEN '...[truncated]' END END"
    )
}

/// Whether `schema` holds a column under a text policy.
pub(super) fn reads_policy(schema: &Schema) -> bool {
    schema.fields().iter().any(|field| field.metadata().contains_key(KEY))
}

pub(super) fn protected(table: &StatementTable) -> bool {
    table.schema().fields().iter().any(|field| field.metadata().contains_key(KEY))
}

pub(super) fn apply_to(
    statement: &mut Statement,
    tables: &[StatementTable],
    dialect: TypeDialect,
) -> Result<Option<sutura_domain::catalog::PrefilterIneligible>> {
    rewrite(statement, tables, dialect, true)
}

/// The Cube local policy path: Cube is not an eligible comparison arm, so it keeps the redacted-only plan.
pub(super) fn cube_local_to(statement: &mut Statement, tables: &[StatementTable]) -> Result<()> {
    rewrite(statement, tables, TypeDialect::DataFusion, false).map(|_ineligible| ())
}

/// `text` through [`apply_to`], its literals written for `dialect`: the policy cells' entry.
#[cfg(test)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "the policy cells pass the text as the production entry once took it"
)]
pub(super) fn apply(text: String, tables: &[StatementTable], dialect: TypeDialect) -> Result<String> {
    on_text(&text, dialect, |statement| {
        apply_to(statement, tables, dialect).map(|_ineligible| ())
    })
}

/// `text` through [`cube_local_to`]: the policy cells' entry.
#[cfg(test)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "the policy cells pass the text as the production entry once took it"
)]
pub(super) fn apply_cube_local(text: String, tables: &[StatementTable]) -> Result<String> {
    on_text(&text, TypeDialect::DataFusion, |statement| cube_local_to(statement, tables))
}

#[cfg(test)]
fn on_text(text: &str, dialect: TypeDialect, policed: impl FnOnce(&mut Statement) -> Result<()>) -> Result<String> {
    let mut statements = Parser::parse_sql(&GenericDialect, text)?;
    let [statement] = statements.as_mut_slice() else {
        return plan_err!("expected one source statement");
    };
    policed(statement)?;
    spelled(statement, dialect)
}

fn rewrite(
    statement: &mut Statement,
    tables: &[StatementTable],
    dialect: TypeDialect,
    prefiltering: bool,
) -> Result<Option<sutura_domain::catalog::PrefilterIneligible>> {
    if !tables.iter().any(protected) {
        return Ok(None);
    }
    let (prefilter, ineligible) = if prefiltering {
        super::text_prefilter::discover(statement, tables, dialect)?
    } else {
        (None, None)
    };
    if let ControlFlow::Break(cause) = statement.visit(&mut Views {
        tables,
        dialect,
        prefilter,
    }) {
        return Err(cause);
    }
    Ok(ineligible)
}

struct Views<'a> {
    tables: &'a [StatementTable],
    dialect: TypeDialect,
    prefilter: Option<Prefilter>,
}

impl VisitorMut for Views<'_> {
    type Break = DataFusionError;

    fn post_visit_expr(&mut self, expr: &mut Expr) -> ControlFlow<Self::Break> {
        if let Expr::CompoundIdentifier(path) = expr
            && path.len() >= 2
        {
            let qualifier = path
                .iter()
                .take(path.len() - 1)
                .map(|part| part.value.as_str())
                .collect::<Vec<_>>()
                .join(".");
            if let Some(table) = self
                .tables
                .iter()
                .find(|table| protected(table) && table.table().to_string() == qualifier)
            {
                let column = path.pop();
                if let Some(column) = column {
                    *path = vec![Ident::with_quote(quote(self.dialect), table.table().name().as_str()), column];
                }
            }
        }
        ControlFlow::Continue(())
    }

    fn post_visit_table_factor(&mut self, factor: &mut TableFactor) -> ControlFlow<Self::Break> {
        let TableFactor::Table { name, alias, .. } = factor else {
            return ControlFlow::Continue(());
        };
        let address = name
            .0
            .iter()
            .filter_map(|part| part.as_ident())
            .map(|part| part.value.as_str())
            .collect::<Vec<_>>()
            .join(".");
        let Some(table) = self
            .tables
            .iter()
            .find(|table| protected(table) && table.table().to_string() == address)
        else {
            return ControlFlow::Continue(());
        };
        // The view lists every column of the table, read or not, so each name is checked here.
        if !quotable_table(table) {
            return ControlFlow::Break(DataFusionError::External(Box::new(UnquotableName)));
        }
        let projection = table
            .schema()
            .fields()
            .iter()
            .map(|field| {
                let name = Ident::with_quote(quote(self.dialect), field.name()).to_string();
                let Some(encoded) = field.metadata().get(KEY) else {
                    return Ok(name);
                };
                let policy: TextPolicy =
                    serde_json::from_str(encoded).map_err(|cause| plan_datafusion_err!("invalid pinned text policy: {cause}"))?;
                // The alias beside it is this column's own name: qualified, the raw read cannot be taken for it.
                let raw = raw_column(self.dialect, table.table().name().as_str(), field.name()).to_string();
                Ok(format!(
                    "{} AS {name}",
                    expression(&raw, &policy, self.dialect, !field.metadata().contains_key(LOB_KEY))
                ))
            })
            .collect::<Result<Vec<_>>>();
        let projection = match projection {
            Ok(columns) => columns.join(", "),
            Err(cause) => return ControlFlow::Break(cause),
        };
        let alias_name = Ident::with_quote(quote(self.dialect), table.table().name().as_str());
        let keyword = if self.dialect == TypeDialect::Oracle { "" } else { "AS " };
        let condition = self
            .prefilter
            .take_if(|prefilter| prefilter.is_for(table))
            .map(Prefilter::into_condition);
        let sql = format!("SELECT * FROM (SELECT {projection} FROM {name}) {keyword}{alias_name}");
        let mut parsed = match Parser::parse_sql(&GenericDialect, &sql) {
            Ok(parsed) => parsed,
            Err(cause) => return ControlFlow::Break(cause.into()),
        };
        let Some(Statement::Query(query)) = parsed.pop() else {
            return ControlFlow::Break(plan_datafusion_err!("text view did not parse"));
        };
        let SetExpr::Select(mut select) = *query.body else {
            return ControlFlow::Break(plan_datafusion_err!("text view is not a select"));
        };
        let Some(mut view) = select.from.pop() else {
            return ControlFlow::Break(plan_datafusion_err!("text view has no table"));
        };
        if let TableFactor::Derived {
            subquery, alias: target, ..
        } = &mut view.relation
        {
            if alias.is_some() {
                *target = alias.take();
            }
            // The typed condition joins the parsed view, so the statement's one final write covers its literals.
            if let (Some(condition), SetExpr::Select(inner)) = (condition, subquery.body.as_mut()) {
                inner.selection = Some(condition);
            }
        }
        *factor = view.relation;
        ControlFlow::Continue(())
    }
}

const fn quote(dialect: TypeDialect) -> char {
    match dialect {
        TypeDialect::BigQuery | TypeDialect::ClickHouse => '`',
        _ => '"',
    }
}

/// The stored column of a source table, qualified so that a projection alias of the same name never shadows it.
pub(super) fn raw_column(dialect: TypeDialect, table: &str, column: &str) -> Expr {
    Expr::CompoundIdentifier(vec![
        Ident::with_quote(quote(dialect), table),
        Ident::with_quote(quote(dialect), column),
    ])
}

/// The view is parsed here before the statement holding it is written for `dialect`, so its literals
/// are written as this process reads them.
fn expression(column: &str, policy: &TextPolicy, dialect: TypeDialect, bound_output: bool) -> String {
    let parsed = TypeDialect::DataFusion;
    let mut value = column.to_owned();
    for rule in policy.redactions() {
        let pattern = rule.pattern();
        let replacement = literal(rule.replacement(), parsed);
        value = match dialect {
            TypeDialect::Oracle => {
                let (pattern, flags) = pattern.strip_prefix("(?i)").map_or((pattern, "c"), |pattern| (pattern, "i"));
                format!(
                    "REGEXP_REPLACE({value}, {}, {replacement}, 1, 0, '{flags}')",
                    literal(pattern, parsed)
                )
            }
            TypeDialect::ClickHouse => format!("replaceRegexpAll({value}, {}, {replacement})", literal(pattern, parsed)),
            TypeDialect::BigQuery => format!("REGEXP_REPLACE({value}, {}, {replacement})", literal(pattern, parsed)),
            TypeDialect::Postgres | TypeDialect::Cube | TypeDialect::DuckDb | TypeDialect::DataFusion | TypeDialect::Foundry => {
                format!("REGEXP_REPLACE({value}, {}, {replacement}, 'g')", literal(pattern, parsed))
            }
        };
    }
    if let Some(bound) = policy.max_text_bytes().filter(|_| bound_output) {
        // ponytail: reserve four UTF-8 bytes per character; Oracle SQL also caps VARCHAR2 at 4000.
        // A byte-exact prefix can replace this if retaining more text becomes necessary.
        let bytes = u32::from(bound);
        let bytes = if dialect == TypeDialect::Oracle {
            bytes.min(4000)
        } else {
            bytes
        };
        let chars = (bytes - 14).div_euclid(4);
        let (length, prefix) = match dialect {
            TypeDialect::Oracle => (
                format!("LENGTH({value})"),
                format!("DBMS_LOB.\"SUBSTR\"({value}, {chars}, 1)"),
            ),
            TypeDialect::ClickHouse => (format!("lengthUTF8({value})"), format!("substringUTF8({value}, 1, {chars})")),
            _ => (format!("CHAR_LENGTH({value})"), format!("SUBSTR({value}, 1, {chars})")),
        };
        value = format!(
            "CASE WHEN {value} IS NULL THEN NULL WHEN {length} > {chars} THEN {prefix} || '...[truncated]' ELSE {prefix} END"
        );
    }
    value
}
