//! Source-side views: every operator above a table sees the same transformed text.

use std::ops::ControlFlow;

use datafusion::common::{plan_datafusion_err, plan_err};
use datafusion::error::{DataFusionError, Result};
use datafusion::sql::sqlparser::ast::{Expr, Ident, SetExpr, Statement, TableFactor, VisitMut as _, VisitorMut};
use datafusion::sql::sqlparser::dialect::GenericDialect;
use datafusion::sql::sqlparser::parser::Parser;
use sutura_domain::catalog::TextPolicy;
use sutura_domain::plan::StatementTable;
use sutura_domain::warehouse::schema::TypeDialect;

const KEY: &str = "sutura:text_policy";

pub(super) fn protected(table: &StatementTable) -> bool {
    table.schema().fields().iter().any(|field| field.metadata().contains_key(KEY))
}

pub(super) fn apply(text: String, tables: &[StatementTable], dialect: TypeDialect) -> Result<String> {
    if !tables.iter().any(protected) {
        return Ok(text);
    }
    let mut statements = Parser::parse_sql(&GenericDialect, &text)?;
    let [statement] = statements.as_mut_slice() else {
        return plan_err!("expected one source statement");
    };
    if let ControlFlow::Break(cause) = statement.visit(&mut Views { tables, dialect }) {
        return Err(cause);
    }
    Ok(statement.to_string())
}

struct Views<'a> {
    tables: &'a [StatementTable],
    dialect: TypeDialect,
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
                Ok(format!("{} AS {name}", expression(&name, &policy, self.dialect)))
            })
            .collect::<Result<Vec<_>>>();
        let projection = match projection {
            Ok(columns) => columns.join(", "),
            Err(cause) => return ControlFlow::Break(cause),
        };
        let alias_name = Ident::with_quote(quote(self.dialect), table.table().name().as_str());
        let keyword = if self.dialect == TypeDialect::Oracle { "" } else { "AS " };
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
        if let TableFactor::Derived { alias: target, .. } = &mut view.relation
            && alias.is_some()
        {
            *target = alias.take();
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

fn literal(text: &str, dialect: TypeDialect) -> String {
    let text = text.replace('\'', "''");
    let text = if matches!(dialect, TypeDialect::BigQuery | TypeDialect::ClickHouse) {
        text.replace('\\', "\\\\")
    } else {
        text
    };
    format!("'{text}'")
}

fn expression(column: &str, policy: &TextPolicy, dialect: TypeDialect) -> String {
    let mut value = column.to_owned();
    for rule in policy.redactions() {
        let pattern = rule.pattern();
        let replacement = literal(rule.replacement(), dialect);
        value = match dialect {
            TypeDialect::Oracle => {
                let (pattern, flags) = pattern.strip_prefix("(?i)").map_or((pattern, "c"), |pattern| (pattern, "i"));
                format!(
                    "REGEXP_REPLACE({value}, {}, {replacement}, 1, 0, '{flags}')",
                    literal(pattern, dialect)
                )
            }
            TypeDialect::ClickHouse => format!("replaceRegexpAll({value}, {}, {replacement})", literal(pattern, dialect)),
            TypeDialect::BigQuery => format!("REGEXP_REPLACE({value}, {}, {replacement})", literal(pattern, dialect)),
            TypeDialect::Postgres | TypeDialect::Cube | TypeDialect::DuckDb | TypeDialect::DataFusion => {
                format!("REGEXP_REPLACE({value}, {}, {replacement}, 'g')", literal(pattern, dialect))
            }
        };
    }
    if let Some(bound) = policy.max_text_bytes() {
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
            TypeDialect::Oracle => (format!("LENGTH({value})"), format!("DBMS_LOB.SUBSTR({value}, {chars}, 1)")),
            TypeDialect::ClickHouse => (format!("lengthUTF8({value})"), format!("substringUTF8({value}, 1, {chars})")),
            _ => (format!("CHAR_LENGTH({value})"), format!("SUBSTR({value}, 1, {chars})")),
        };
        value = format!(
            "CASE WHEN {value} IS NULL THEN NULL WHEN {length} > {chars} THEN {prefix} || '...[truncated]' ELSE {prefix} END"
        );
    }
    value
}
