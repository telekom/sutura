//! Raw necessary conditions; the original predicate still searches the transformed value.

use std::ops::ControlFlow;

use datafusion::arrow::datatypes::{DataType, Field};
use datafusion::common::plan_datafusion_err;
use datafusion::error::Result;
use datafusion::sql::sqlparser::ast::helpers::attached_token::AttachedToken;
use datafusion::sql::sqlparser::ast::{
    BinaryOperator, CaseWhen, Expr, Function, FunctionArg, FunctionArgExpr, FunctionArgumentList, FunctionArguments, GroupByExpr,
    Ident, ObjectName, Query, Select, SelectItem, SetExpr, Statement, TableFactor, UnaryOperator, Value, Visit as _, Visitor,
};
use sutura_domain::catalog::{Collation, PrefilterCapability, TextPolicy};
use sutura_domain::plan::StatementTable;
use sutura_domain::warehouse::schema::TypeDialect;

use super::text::raw_column;

pub(super) struct Prefilter {
    table: String,
    condition: Expr,
}

impl Prefilter {
    pub(super) fn is_for(&self, table: &StatementTable) -> bool {
        self.table == table.table().to_string()
    }

    pub(super) fn into_condition(self) -> Expr {
        self.condition
    }
}

/// The single level, among a chain of unparser-introduced derived-table wrapping, that carries a
/// `WHERE` - borrowed in place rather than re-matched once the chain walk has found it.
struct Owner<'a> {
    selection: &'a Expr,
    projection: &'a [SelectItem],
    alias: Option<&'a Ident>,
}

/// A collation override or negative/disjunctive logic anywhere is unknown; so is a `Query` node
/// anywhere in the statement that the chain walk did not itself step through.
///
/// The caller checks `queries` against the chain's own depth for exact equality, never merely an
/// upper bound - a position-blind bound would let an attacker trade one chain hop for one injected
/// subquery elsewhere (a scalar subquery inside a derived level's projection, say) and still clear
/// a bare count ceiling. Counting here only; the caller compares.
#[derive(Default)]
struct Scope {
    queries: usize,
}

impl Visitor for Scope {
    type Break = ();

    fn pre_visit_query(&mut self, _query: &Query) -> ControlFlow<Self::Break> {
        self.queries += 1;
        ControlFlow::Continue(())
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

/// Every plain column name an owner's `WHERE` names, bare identifier or compound identifier alike.
///
/// Purely syntactic and deliberately over-inclusive: it does not resolve qualifiers against the
/// schema, so it may collect a name that turns out irrelevant, which only makes a later passthrough
/// check stricter than it needs to be, never laxer.
#[derive(Default)]
struct ColumnNames {
    names: Vec<String>,
}

impl Visitor for ColumnNames {
    type Break = ();

    fn pre_visit_expr(&mut self, expression: &Expr) -> ControlFlow<Self::Break> {
        match expression {
            Expr::Identifier(ident) => self.names.push(ident.value.clone()),
            Expr::CompoundIdentifier(path) => {
                if let Some(ident) = path.last() {
                    self.names.push(ident.value.clone());
                }
            }
            _ => {}
        }
        ControlFlow::Continue(())
    }
}

/// A window function anywhere in a projection - a row-changing device as much as `LIMIT` or
/// `GROUP BY`, just not held by a dedicated `Select`/`Query` field.
struct WindowProbe;

impl Visitor for WindowProbe {
    type Break = ();

    fn pre_visit_expr(&mut self, expression: &Expr) -> ControlFlow<Self::Break> {
        if matches!(expression, Expr::Function(function) if function.over.is_some()) {
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    }
}

/// `name` reaches this level's base unchanged: a bare identifier or compound identifier of that
/// exact name, or a wildcard that passes every column through untouched. An alias, a cast, a
/// function call or any other expression under that name is a rename or a transform, not a
/// passthrough - the raw condition built later resolves `name` straight against the physical
/// schema, so anything else would let that resolution land on a different column than the one the
/// redacted predicate actually tested.
fn passes_through(name: &str, projection: &[SelectItem]) -> bool {
    projection.iter().any(|item| match item {
        SelectItem::UnnamedExpr(Expr::Identifier(ident)) => ident.value == name,
        SelectItem::UnnamedExpr(Expr::CompoundIdentifier(path)) => path.last().is_some_and(|ident| ident.value == name),
        SelectItem::Wildcard(_) | SelectItem::QualifiedWildcard(..) => true,
        _ => false,
    })
}

/// A level that only ever narrows which columns are visible, never how many rows there are or in
/// what order - the shape every level strictly below the owner must hold, since none of them is
/// the one the redacted predicate's own row count was already checked against.
fn pure_projection(select: &Select) -> bool {
    select.distinct.is_none()
        && select.select_modifiers.is_none()
        && select.top.is_none()
        && select.exclude.is_none()
        && select.into.is_none()
        && matches!(&select.group_by, GroupByExpr::Expressions(exprs, modifiers) if exprs.is_empty() && modifiers.is_empty())
        && select.cluster_by.is_empty()
        && select.distribute_by.is_empty()
        && select.sort_by.is_empty()
        && select.having.is_none()
        && select.named_window.is_empty()
        && select.qualify.is_none()
        && select.value_table_mode.is_none()
        && select.optimizer_hints.is_empty()
        && !select.projection.iter().any(|item| item.visit(&mut WindowProbe).is_break())
}

/// The `Query`-level counterpart of [`pure_projection`]: no clause that reorders, bounds or locks
/// the rows a level below the owner passes up.
const fn pure_query(query: &Query) -> bool {
    query.order_by.is_none()
        && query.limit_clause.is_none()
        && query.fetch.is_none()
        && query.locks.is_empty()
        && query.for_clause.is_none()
        && query.format_clause.is_none()
}

pub(super) fn discover(statement: &Statement, tables: &[StatementTable], dialect: TypeDialect) -> Result<Option<Prefilter>> {
    let Some(capability) = PrefilterCapability::of(dialect) else {
        return Ok(None);
    };
    let Statement::Query(query) = statement else {
        return Ok(None);
    };

    // The unparser wraps one derived table per plan node it cannot flatten into a single `SELECT`
    // - a redaction projection below a filter, an aggregate above it, the per-row LOB bound - none
    // of which carries a predicate of its own. Walk down through that wrapping to the physical
    // table at the bottom, remembering the single level, if any, that does carry one, and the exact
    // column names its predicate names, so every level below it can be held to passing them through
    // unchanged (see `passes_through`) rather than renaming or transforming the very column the raw
    // condition built later will resolve straight against the physical schema.
    let mut current = query.as_ref();
    let mut levels: usize = 0;
    let mut owner: Option<Owner<'_>> = None;
    let mut owner_names: Vec<String> = Vec::new();
    let address = loop {
        levels += 1;
        let SetExpr::Select(select) = current.body.as_ref() else {
            return Ok(None);
        };
        if current.with.is_some()
            || !current.pipe_operators.is_empty()
            || current.settings.is_some()
            || !select.lateral_views.is_empty()
            || select.prewhere.is_some()
            || !select.connect_by.is_empty()
        {
            return Ok(None);
        }
        let [from] = select.from.as_slice() else {
            return Ok(None);
        };
        if !from.joins.is_empty() {
            return Ok(None);
        }
        let (TableFactor::Table {
            alias: relation_alias, ..
        }
        | TableFactor::Derived {
            alias: relation_alias, ..
        }) = &from.relation
        else {
            return Ok(None);
        };
        if relation_alias.as_ref().is_some_and(|alias| !alias.columns.is_empty()) {
            return Ok(None);
        }
        // Strictly below the owner: a pure, row-preserving projection, and every name the owner's
        // predicate depends on must still cross unchanged.
        if owner.is_some()
            && (!pure_query(current)
                || !pure_projection(select)
                || owner_names.iter().any(|name| !passes_through(name, &select.projection)))
        {
            return Ok(None);
        }
        if let Some(selection) = &select.selection {
            if owner.is_some() {
                // More than one level filters: not the benign unparser-wrapping shape this walk
                // recognizes, and scope across two independent predicates is unknown.
                return Ok(None);
            }
            let mut names = ColumnNames::default();
            let _never_breaks = selection.visit(&mut names);
            owner_names = names.names;
            owner = Some(Owner {
                selection,
                projection: &select.projection,
                alias: relation_alias.as_ref().map(|alias| &alias.name),
            });
        }
        match &from.relation {
            TableFactor::Table {
                name,
                args: None,
                with_hints,
                version: None,
                with_ordinality: false,
                partitions,
                json_path: None,
                sample: None,
                index_hints,
                ..
            } => {
                if !with_hints.is_empty() || !partitions.is_empty() || !index_hints.is_empty() {
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
                break path.join(".");
            }
            TableFactor::Derived {
                lateral: false,
                subquery,
                sample: None,
                ..
            } => current = subquery.as_ref(),
            _ => return Ok(None),
        }
    };

    let mut matching = tables.iter().filter(|table| table.table().to_string() == address);
    let Some(table) = matching.next() else {
        return Ok(None);
    };
    if matching.next().is_some() {
        return Ok(None);
    }
    let Some(Owner {
        selection,
        projection,
        alias,
    }) = owner
    else {
        return Ok(None);
    };
    // Any `Query` node anywhere in the statement besides the ones the chain walk above itself
    // stepped through - a correlated subquery, a UNION, a CTE - is unknown, as is a collation
    // override or negative/disjunctive logic anywhere. The count must equal the chain's own depth
    // exactly, not merely stay under it; see `Scope`'s own doc for why a bound alone is not enough.
    let mut scope = Scope::default();
    if statement.visit(&mut scope).is_break() || scope.queries != levels {
        return Ok(None);
    }
    let aliases = projection
        .iter()
        .filter_map(|item| match item {
            SelectItem::ExprWithAlias { alias, .. } => Some(alias.value.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut terms = Vec::new();
    conjuncts(selection, &mut terms);
    let mut chosen = None;
    for (index, term) in terms.iter().enumerate() {
        if let Some(condition) = condition(term, table, alias, &aliases, (dialect, capability))? {
            chosen = Some((index, condition));
            break;
        }
    }
    let Some((chosen, condition)) = chosen else {
        return Ok(None);
    };
    // The source may test the raw text before any sibling, so a sibling that can raise on one row
    // would turn "error or empty answer" into a signal about that row's unredacted text.
    if terms
        .iter()
        .enumerate()
        .any(|(index, term)| index != chosen && !cannot_raise(term, table, alias, &aliases, capability))
    {
        return Ok(None);
    }
    Ok(Some(Prefilter {
        table: address,
        condition,
    }))
}

fn conjuncts<'a>(expression: &'a Expr, terms: &mut Vec<&'a Expr>) {
    match expression {
        Expr::Nested(expression) => conjuncts(expression, terms),
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => {
            conjuncts(left, terms);
            conjuncts(right, terms);
        }
        other => terms.push(other),
    }
}

/// Only a plain column compared with literals of its own type: no function, cast, arithmetic,
/// subquery or column on both sides, so no row can make the source raise.
///
/// On PostgreSQL a policy column is read through the view's regex, which a nondeterministic
/// collation refuses. Such a sibling can raise on one row, so it counts only where its column is
/// pinned binary too.
fn cannot_raise(
    term: &Expr,
    table: &StatementTable,
    alias: Option<&Ident>,
    aliases: &[&str],
    capability: PrefilterCapability,
) -> bool {
    let column = |expression: &Expr| {
        plain(expression, table, alias, aliases).filter(|field| {
            capability != PrefilterCapability::Postgres
                || !field.metadata().contains_key("sutura:text_policy")
                || Collation::of_field(field).is_binary()
        })
    };
    let against = |expression: &Expr, literal: &Expr| column(expression).is_some_and(|field| literal_of(literal, field));
    match term {
        Expr::BinaryOp {
            left,
            op:
                BinaryOperator::Eq
                | BinaryOperator::NotEq
                | BinaryOperator::Lt
                | BinaryOperator::LtEq
                | BinaryOperator::Gt
                | BinaryOperator::GtEq,
            right,
        } => against(left, right) || against(right, left),
        Expr::IsNull(expression) | Expr::IsNotNull(expression) => column(expression).is_some(),
        Expr::InList { expr, list, .. } => list.iter().all(|literal| against(expr, literal)),
        Expr::Between { expr, low, high, .. } => against(expr, low) && against(expr, high),
        _ => false,
    }
}

fn plain<'a>(expression: &Expr, table: &'a StatementTable, alias: Option<&Ident>, aliases: &[&str]) -> Option<&'a Field> {
    let column = column_of(expression, table, alias)?;
    if aliases.contains(&column.value.as_str()) {
        return None;
    }
    table.schema().field_with_name(&column.value).ok()
}

/// A literal the source compares with this column without converting a stored value.
///
/// An Arrow `Utf8` field alone proves nothing: a type the dialect does not list falls back to
/// `Utf8`, and an unbounded Oracle `NUMBER` is `Utf8` too, so a string literal could make the source
/// convert stored values and raise on one row. Pinning marks declared scalar text explicitly;
/// a text policy also proves declared text. Neither proof admits an Oracle LOB comparison.
fn literal_of(expression: &Expr, field: &Field) -> bool {
    let kind = field.data_type();
    let unsigned = matches!(kind, DataType::UInt8 | DataType::UInt16 | DataType::UInt32 | DataType::UInt64);
    let number = unsigned
        || matches!(
            kind,
            DataType::Int8
                | DataType::Int16
                | DataType::Int32
                | DataType::Int64
                | DataType::Float16
                | DataType::Float32
                | DataType::Float64
                | DataType::Decimal128(..)
                | DataType::Decimal256(..)
        );
    let metadata = field.metadata();
    let text = matches!(kind, DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View)
        && (metadata.contains_key("sutura:text_policy")
            || metadata.get("sutura:scalar_text").is_some_and(|value| value == "true"))
        && !metadata.contains_key("sutura:unbounded_number")
        && !metadata.contains_key("sutura:oracle_lob");
    match expression {
        Expr::Value(value) => match &value.value {
            Value::SingleQuotedString(_) => text,
            Value::Number(..) => number,
            Value::Boolean(_) => *kind == DataType::Boolean,
            _ => false,
        },
        Expr::UnaryOp { op, expr } => {
            let signed = match op {
                UnaryOperator::Minus => !unsigned,
                UnaryOperator::Plus => true,
                _ => false,
            };
            signed && number && matches!(expr.as_ref(), Expr::Value(value) if matches!(value.value, Value::Number(..)))
        }
        _ => false,
    }
}

fn column_of<'a>(input: &'a Expr, table: &StatementTable, alias: Option<&Ident>) -> Option<&'a Ident> {
    match input {
        Expr::Identifier(column) => Some(column),
        Expr::CompoundIdentifier(path) => {
            let (column, qualifier) = path.split_last()?;
            let qualifier: Vec<_> = qualifier.iter().map(|part| part.value.as_str()).collect();
            let qualifier = qualifier.join(".");
            let permitted = alias.map_or_else(
                || {
                    let physical = table.table().to_string();
                    qualifier == physical || physical.rsplit('.').next() == Some(qualifier.as_str())
                },
                |alias| qualifier == alias.value,
            );
            permitted.then_some(column)
        }
        _ => None,
    }
}

fn condition(
    term: &Expr,
    table: &StatementTable,
    alias: Option<&Ident>,
    aliases: &[&str],
    (dialect, capability): (TypeDialect, PrefilterCapability),
) -> Result<Option<Expr>> {
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
        Expr::Function(function) if capability == PrefilterCapability::Oracle => {
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
    let Some(column) = column_of(input, table, alias) else {
        return Ok(None);
    };
    if aliases.contains(&column.value.as_str()) {
        return Ok(None);
    }
    let Ok(field) = table.schema().field_with_name(&column.value) else {
        return Ok(None);
    };
    if !capability.admits(&Collation::of_field(field)) {
        return Ok(None);
    }
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
    if capability == PrefilterCapability::Oracle && !lob {
        return Ok(None);
    }
    // A wrong authored binary pin must not let case folding create a replacement match.
    let folded = capability == PrefilterCapability::DuckDb;
    if !policy
        .redactions()
        .iter()
        .all(|rule| preserves(needle, rule.replacement(), upper, folded))
        || (!lob && policy.max_text_bytes().is_some() && !preserves(needle, "...[truncated]", upper, folded))
    {
        return Ok(None);
    }
    let column = raw_column(dialect, table.table().name().as_str(), field.name());
    Ok(Some(raw_condition(capability, column, needle, upper)))
}

fn string(value: &str) -> Expr {
    Expr::value(Value::SingleQuotedString(String::from(value)))
}

/// The necessary condition on the stored `column`; the original predicate still decides on the redacted value.
pub(super) fn raw_condition(capability: PrefilterCapability, column: Expr, needle: &str, upper: bool) -> Expr {
    let input = if upper {
        Expr::Function(Function {
            name: ObjectName::from(vec![Ident::new("UPPER")]),
            uses_odbc_syntax: false,
            parameters: FunctionArguments::None,
            args: FunctionArguments::List(FunctionArgumentList {
                duplicate_treatment: None,
                args: vec![FunctionArg::Unnamed(FunctionArgExpr::Expr(column))],
                clauses: Vec::new(),
            }),
            filter: None,
            null_treatment: None,
            over: None,
            within_group: Vec::new(),
        })
    } else {
        column
    };
    let raw = Expr::Like {
        negated: false,
        any: false,
        expr: Box::new(input),
        pattern: Box::new(string(&format!("%{needle}%"))),
        escape_char: None,
    };
    match capability {
        PrefilterCapability::DataFusion
        | PrefilterCapability::Postgres
        | PrefilterCapability::DuckDb
        | PrefilterCapability::BigQuery
        | PrefilterCapability::ClickHouse => Expr::Nested(Box::new(raw)),
        PrefilterCapability::Oracle => {
            let sort = Expr::Function(Function {
                name: ObjectName::from(vec![Ident::new("SYS_CONTEXT")]),
                uses_odbc_syntax: false,
                parameters: FunctionArguments::None,
                args: FunctionArguments::List(FunctionArgumentList {
                    duplicate_treatment: None,
                    args: [string("USERENV"), string("NLS_SORT")]
                        .into_iter()
                        .map(|argument| FunctionArg::Unnamed(FunctionArgExpr::Expr(argument)))
                        .collect(),
                    clauses: Vec::new(),
                }),
                filter: None,
                null_treatment: None,
                over: None,
                within_group: Vec::new(),
            });
            let binary = Expr::BinaryOp {
                left: Box::new(Expr::Collate {
                    expr: Box::new(sort),
                    collation: ObjectName::from(vec![Ident::new("BINARY")]),
                }),
                op: BinaryOperator::Eq,
                right: Box::new(string("BINARY")),
            };
            let number = |digit: &str| Expr::value(Value::Number(String::from(digit), false));
            let case = |condition, result, otherwise| Expr::Case {
                case_token: AttachedToken::empty(),
                end_token: AttachedToken::empty(),
                operand: None,
                conditions: vec![CaseWhen { condition, result }],
                else_result: Some(Box::new(otherwise)),
            };
            Expr::BinaryOp {
                left: Box::new(case(binary, case(raw, number("1"), number("0")), number("1"))),
                op: BinaryOperator::Eq,
                right: Box::new(number("1")),
            }
        }
    }
}

/// No occurrence may live inside a replacement or cross either of its nonempty boundaries.
///
/// `folded` compares ASCII case-insensitively, so the proof also holds under a case-insensitive collation.
fn preserves(needle: &str, replacement: &str, upper: bool, folded: bool) -> bool {
    if replacement.is_empty() || !replacement.is_ascii() {
        return false;
    }
    let normalized = if upper {
        replacement.to_ascii_uppercase()
    } else {
        String::from(replacement)
    };
    let (needle, normalized) = if folded {
        (needle.to_ascii_lowercase(), normalized.to_ascii_lowercase())
    } else {
        (String::from(needle), normalized)
    };
    let mut bytes = normalized.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    let last = bytes.next_back().unwrap_or(first);
    !needle.as_bytes().contains(&first) && !needle.as_bytes().contains(&last) && !normalized.contains(&needle)
}
