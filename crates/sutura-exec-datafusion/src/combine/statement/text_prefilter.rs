//! Raw necessary conditions; the original predicate still searches the transformed value.

use std::ops::ControlFlow;

use datafusion::common::plan_datafusion_err;
use datafusion::error::Result;
use datafusion::sql::sqlparser::ast::{
    BinaryOperator, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, Ident, Query, SelectItem, SetExpr, Statement,
    TableFactor, UnaryOperator, Value, Visit as _, Visitor,
};
use sutura_domain::catalog::TextPolicy;
use sutura_domain::plan::StatementTable;
use sutura_domain::warehouse::schema::TypeDialect;

pub(super) struct Prefilter {
    table: String,
    condition: String,
}

impl Prefilter {
    pub(super) fn for_table(&self, table: &StatementTable) -> Option<&str> {
        (self.table == table.table().to_string()).then_some(self.condition.as_str())
    }
}

/// A scope with more than one query, a collation override or negative/disjunctive logic is unknown.
#[derive(Default)]
struct Scope {
    queries: usize,
}

impl Visitor for Scope {
    type Break = ();

    fn pre_visit_query(&mut self, _query: &Query) -> ControlFlow<Self::Break> {
        self.queries += 1;
        if self.queries > 1 {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }

    fn pre_visit_expr(&mut self, expression: &Expr) -> ControlFlow<Self::Break> {
        if matches!(
            expression,
            Expr::Collate { .. }
                | Expr::BinaryOp {
                    op: BinaryOperator::Or,
                    ..
                }
                | Expr::UnaryOp {
                    op: UnaryOperator::Not,
                    ..
                }
        ) {
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    }
}

pub(super) fn discover(statement: &Statement, tables: &[StatementTable], dialect: TypeDialect) -> Result<Option<Prefilter>> {
    // Other sources may bind a linguistic collation to a column. No pinned evidence says otherwise.
    if !matches!(dialect, TypeDialect::DataFusion | TypeDialect::Oracle) {
        return Ok(None);
    }
    let Statement::Query(query) = statement else {
        return Ok(None);
    };
    let SetExpr::Select(select) = query.body.as_ref() else {
        return Ok(None);
    };
    if query.with.is_some()
        || !query.pipe_operators.is_empty()
        || query.settings.is_some()
        || !select.lateral_views.is_empty()
        || select.prewhere.is_some()
        || !select.connect_by.is_empty()
        || statement.visit(&mut Scope::default()).is_break()
    {
        return Ok(None);
    }
    let [from] = select.from.as_slice() else {
        return Ok(None);
    };
    if !from.joins.is_empty() {
        return Ok(None);
    }
    let TableFactor::Table {
        name,
        alias,
        args: None,
        with_hints,
        version: None,
        with_ordinality: false,
        partitions,
        json_path: None,
        sample: None,
        index_hints,
    } = &from.relation
    else {
        return Ok(None);
    };
    if !with_hints.is_empty()
        || !partitions.is_empty()
        || !index_hints.is_empty()
        || alias.as_ref().is_some_and(|alias| !alias.columns.is_empty())
    {
        return Ok(None);
    }
    let Some(path) = name
        .0
        .iter()
        .map(|part| part.as_ident().map(|part| part.value.as_str()))
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(None);
    };
    let address = path.join(".");
    let mut matching = tables.iter().filter(|table| table.table().to_string() == address);
    let Some(table) = matching.next() else {
        return Ok(None);
    };
    if matching.next().is_some() {
        return Ok(None);
    }
    let Some(selection) = &select.selection else {
        return Ok(None);
    };
    let mut selected = None;
    let aliases = select
        .projection
        .iter()
        .filter_map(|item| match item {
            SelectItem::ExprWithAlias { alias, .. } => Some(alias.value.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    conjuncts(selection, &mut |term| {
        if selected.is_none() {
            selected = condition(term, table, alias.as_ref().map(|alias| &alias.name), &aliases, dialect)?;
        }
        Ok(())
    })?;
    Ok(selected.map(|condition| Prefilter {
        table: address,
        condition,
    }))
}

fn conjuncts(expression: &Expr, visit: &mut impl FnMut(&Expr) -> Result<()>) -> Result<()> {
    match expression {
        Expr::Nested(expression) => conjuncts(expression, visit),
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            conjuncts(left, visit)?;
            conjuncts(right, visit)
        }
        other => visit(other),
    }
}

fn condition(
    term: &Expr,
    table: &StatementTable,
    alias: Option<&Ident>,
    aliases: &[&str],
    dialect: TypeDialect,
) -> Result<Option<String>> {
    let Expr::Like {
        negated: false,
        any: false,
        expr,
        pattern,
        escape_char: None,
    } = term
    else {
        return Ok(None);
    };
    let Expr::Value(value) = pattern.as_ref() else {
        return Ok(None);
    };
    let Value::SingleQuotedString(pattern) = &value.value else {
        return Ok(None);
    };
    let Some(needle) = pattern.strip_prefix('%').and_then(|text| text.strip_suffix('%')) else {
        return Ok(None);
    };
    if needle.is_empty() || needle.len() > 128 || !needle.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Ok(None);
    }
    let (input, upper) = match expr.as_ref() {
        Expr::Function(function) if dialect == TypeDialect::Oracle => {
            let [name] = function.name.0.as_slice() else {
                return Ok(None);
            };
            if !name.as_ident().is_some_and(|name| name.value.eq_ignore_ascii_case("upper"))
                || function.uses_odbc_syntax
                || !matches!(function.parameters, FunctionArguments::None)
                || function.filter.is_some()
                || function.null_treatment.is_some()
                || function.over.is_some()
                || !function.within_group.is_empty()
            {
                return Ok(None);
            }
            let FunctionArguments::List(arguments) = &function.args else {
                return Ok(None);
            };
            let [FunctionArg::Unnamed(FunctionArgExpr::Expr(input))] = arguments.args.as_slice() else {
                return Ok(None);
            };
            if arguments.duplicate_treatment.is_some() || !arguments.clauses.is_empty() {
                return Ok(None);
            }
            (input, true)
        }
        other => (other, false),
    };
    let column = match input {
        Expr::Identifier(column) => column,
        Expr::CompoundIdentifier(path) => {
            let Some((column, qualifier)) = path.split_last() else {
                return Ok(None);
            };
            let qualifier: Vec<_> = qualifier.iter().map(|part| part.value.as_str()).collect();
            let qualifier = qualifier.join(".");
            let permitted = alias.map_or_else(
                || {
                    let physical = table.table().to_string();
                    qualifier == physical || physical.rsplit('.').next() == Some(qualifier.as_str())
                },
                |alias| qualifier == alias.value,
            );
            if !permitted {
                return Ok(None);
            }
            column
        }
        _ => return Ok(None),
    };
    if aliases.contains(&column.value.as_str()) {
        return Ok(None);
    }
    let Ok(field) = table.schema().field_with_name(&column.value) else {
        return Ok(None);
    };
    let Some(encoded) = field.metadata().get("sutura:text_policy") else {
        return Ok(None);
    };
    let policy: TextPolicy =
        serde_json::from_str(encoded).map_err(|cause| plan_datafusion_err!("invalid pinned text policy: {cause}"))?;
    if policy.raw_prefilter().is_none() || policy.redactions().is_empty() {
        return Ok(None);
    }
    let lob = field
        .metadata()
        .get("sutura:oracle_lob")
        .is_some_and(|kind| matches!(kind.as_str(), "clob" | "nclob"));
    if dialect == TypeDialect::Oracle && !lob {
        return Ok(None);
    }
    if !policy
        .redactions()
        .iter()
        .all(|rule| preserves(needle, rule.replacement(), upper))
        || (!lob && policy.max_text_bytes().is_some() && !preserves(needle, "...[truncated]", upper))
    {
        return Ok(None);
    }
    let column = Ident::with_quote('"', field.name());
    let input = if upper {
        format!("UPPER({column})")
    } else {
        column.to_string()
    };
    let raw = format!("{input} LIKE '%{needle}%'");
    let condition = match dialect {
        TypeDialect::Oracle => format!(
            "CASE WHEN SYS_CONTEXT('USERENV', 'NLS_SORT') COLLATE BINARY = 'BINARY' THEN CASE WHEN {raw} THEN 1 ELSE 0 END ELSE 1 END = 1"
        ),
        TypeDialect::DataFusion => format!("({raw})"),
        _ => return Ok(None),
    };
    Ok(Some(condition))
}

/// No occurrence may live inside a replacement or cross either of its nonempty boundaries.
fn preserves(needle: &str, replacement: &str, upper: bool) -> bool {
    if replacement.is_empty() || !replacement.is_ascii() {
        return false;
    }
    let normalized = if upper {
        replacement.to_ascii_uppercase()
    } else {
        String::from(replacement)
    };
    let mut bytes = normalized.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    let last = bytes.next_back().unwrap_or(first);
    !needle.as_bytes().contains(&first) && !needle.as_bytes().contains(&last) && !normalized.contains(needle)
}
