//! Source-side views: every operator above a table sees the same transformed text.

use std::ops::ControlFlow;

use datafusion::arrow::datatypes::{DataType, Schema};
use datafusion::common::{plan_datafusion_err, plan_err};
use datafusion::error::{DataFusionError, Result};
use datafusion::sql::sqlparser::ast::{Expr, Ident, SetExpr, Statement, TableFactor, VisitMut as _, VisitorMut};
use datafusion::sql::sqlparser::dialect::GenericDialect;
use datafusion::sql::sqlparser::parser::Parser;
use sutura_domain::catalog::TextPolicy;
use sutura_domain::plan::StatementTable;
use sutura_domain::warehouse::schema::TypeDialect;

const KEY: &str = "sutura:text_policy";
const LOB_KEY: &str = "sutura:oracle_lob";

pub(super) fn oracle_lobs(table: &StatementTable) -> bool {
    table.dialect() == TypeDialect::Oracle
        && table
            .schema()
            .fields()
            .iter()
            .any(|field| field.metadata().contains_key(LOB_KEY))
}

/// A LOB is redacted at the source without clipping. Only a whole-source answer may return it:
/// clipping a federation leaf would make a later local predicate silently search a prefix.
pub(super) fn bounded_oracle_rows(sql: &str, schema: &Schema, tables: &[StatementTable]) -> Result<String> {
    let parsed = Parser::parse_sql(&GenericDialect, sql)?;
    let ordered = matches!(parsed.first(), Some(Statement::Query(query)) if query.order_by.is_some());
    bounded_oracle_output(sql, schema, tables, ordered)
}

/// Label the caller's original query with a CTE column list: its numeric expressions, aliases,
/// wildcards and set operations stay native. Existing CTEs remain at the top level for Oracle.
pub(super) fn bounded_native_oracle_rows(sql: &str, schema: &Schema, tables: &[StatementTable]) -> Result<String> {
    let mut parsed = Parser::parse_sql(&GenericDialect, sql)?;
    let Some(Statement::Query(mut query)) = parsed.pop() else {
        return plan_err!("expected one native Oracle query");
    };
    let ordered = query.order_by.is_some();
    let existing = query
        .with
        .take()
        .map_or_else(|| String::from("WITH "), |with| format!("{with}, "));
    let mut name = String::from("sutura_lob_values");
    let folded = sql.to_ascii_lowercase();
    while folded.contains(&name) {
        name.push('_');
    }
    let labels = (0..schema.fields().len())
        .map(|index| format!("\"c{index}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let labelled = format!("SELECT * FROM \"{name}\"");
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
        .flat_map(|table| table.schema().fields())
        .any(|field| field.metadata().get(LOB_KEY).is_some_and(|kind| kind == "nclob"));
    let columns = schema
        .fields()
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let column = format!("\"sutura_lob_output\".\"c{index}\"");
            let value = if matches!(field.data_type(), DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View)
                && !field.metadata().contains_key("sutura:unbounded_number")
            {
                let clob = field.metadata().get(LOB_KEY).is_some_and(|kind| kind == "clob");
                // NCLOB scalar results have a national-character byte ceiling as well. A derived
                // expression may lose its field metadata, so keep that ceiling unless it is CLOB.
                bounded_lob(&column, if national && !clob { bytes.min(2000) } else { bytes })
            } else {
                column
            };
            format!("{value} AS {}", Ident::with_quote('"', field.name()))
        })
        .collect::<Vec<_>>()
        .join(", ");
    if ordered {
        // ROWNUM pins the inner order before the output expressions run; the outer ORDER BY
        // restores that order without sorting on a clipped value or returning the ordinal.
        Ok(format!(
            "SELECT {columns} FROM (SELECT \"sutura_lob_inner\".*, ROWNUM AS \"sutura_lob_ordinal\" FROM ({sql}) \"sutura_lob_inner\") \"sutura_lob_output\" ORDER BY \"sutura_lob_output\".\"sutura_lob_ordinal\""
        ))
    } else {
        Ok(format!("SELECT {columns} FROM ({sql}) \"sutura_lob_output\""))
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
    let pattern = literal(&format!("^[]\t\r\n{alphabet}\\\\-]*$"), TypeDialect::Oracle);
    let chars = format!(
        "CASE WHEN REGEXP_LIKE(SUBSTR({value}, 1, {payload}), {pattern}, 'c') THEN {payload} ELSE {} END",
        payload.div_euclid(4)
    );
    format!(
        "CASE WHEN {value} IS NULL THEN NULL ELSE DBMS_LOB.\"SUBSTR\"({value}, {chars}, 1) || CASE WHEN LENGTH({value}) > {chars} THEN '...[truncated]' END END"
    )
}

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
                Ok(format!(
                    "{} AS {name}",
                    expression(&name, &policy, self.dialect, !field.metadata().contains_key(LOB_KEY))
                ))
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

fn expression(column: &str, policy: &TextPolicy, dialect: TypeDialect, bound_output: bool) -> String {
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
