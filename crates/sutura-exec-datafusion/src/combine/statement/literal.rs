//! String literals and names written as the source they are sent to reads them back.

use core::fmt::Display;
use std::ops::ControlFlow;

use datafusion::common::plan_datafusion_err;
use datafusion::error::{DataFusionError, Result};
use datafusion::sql::sqlparser::ast::{Value, ValueWithSpan, VisitMut, VisitorMut};
use sutura_domain::plan::StatementTable;
use sutura_domain::warehouse::schema::TypeDialect;

/// `text` quoted so `dialect` reads it back unchanged. `BigQuery` and `ClickHouse` read a backslash in a
/// string as an escape, so there the backslash and the quote are each escaped with one; every other
/// dialect reads a backslash as itself and a doubled quote as one.
pub(super) fn literal(text: &str, dialect: TypeDialect) -> String {
    let escapes = matches!(dialect, TypeDialect::BigQuery | TypeDialect::ClickHouse);
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('\'');
    for character in text.chars() {
        match character {
            '\\' if escapes => quoted.push_str(r"\\"),
            '\'' if escapes => quoted.push_str(r"\'"),
            '\'' => quoted.push_str("''"),
            other => quoted.push(other),
        }
    }
    quoted.push('\'');
    quoted
}

/// Whether `name` holds no backslash and no quote character. `sqlparser` quotes a name by the same
/// rule as a literal, which does not read back exactly once a name holds one, so such a name is
/// refused; the vendored unparser refuses every name it builds by the same test.
pub(super) fn quotable(name: &str) -> bool {
    !name.contains(['\\', '"', '`', '\''])
}

/// Whether `table`'s path and every column name are [`quotable`].
pub(super) fn quotable_table(table: &StatementTable) -> bool {
    quotable(&table.table().to_string()) && table.schema().fields().iter().all(|field| quotable(field.name()))
}

/// `node`'s text with every string literal written by [`literal`] for `dialect`. `sqlparser` never
/// escapes a backslash and leaves a quote as it stands after a backslash or beside another quote, so
/// its own quoting does not read back as the value it holds. A string form other than a plain or a
/// national literal is refused rather than written.
pub(super) fn spelled<T>(node: &mut T, dialect: TypeDialect) -> Result<String>
where
    T: VisitMut + Display,
{
    if let ControlFlow::Break(cause) = node.visit(&mut Literals(dialect)) {
        return Err(cause);
    }
    Ok(node.to_string())
}

struct Literals(TypeDialect);

impl VisitorMut for Literals {
    type Break = DataFusionError;

    fn post_visit_value(&mut self, value: &mut ValueWithSpan) -> ControlFlow<Self::Break> {
        let written = match &value.value {
            Value::SingleQuotedString(text) => literal(text, self.0),
            Value::NationalStringLiteral(text) => format!("N{}", literal(text, self.0)),
            Value::HexStringLiteral(hex) if hex.chars().all(|digit| digit.is_ascii_hexdigit()) => {
                return ControlFlow::Continue(());
            }
            Value::Number(..) | Value::Boolean(_) | Value::Null | Value::Placeholder(_) => {
                return ControlFlow::Continue(());
            }
            Value::DollarQuotedString(_)
            | Value::TripleSingleQuotedString(_)
            | Value::TripleDoubleQuotedString(_)
            | Value::EscapedStringLiteral(_)
            | Value::UnicodeStringLiteral(_)
            | Value::SingleQuotedByteStringLiteral(_)
            | Value::DoubleQuotedByteStringLiteral(_)
            | Value::TripleSingleQuotedByteStringLiteral(_)
            | Value::TripleDoubleQuotedByteStringLiteral(_)
            | Value::SingleQuotedRawStringLiteral(_)
            | Value::DoubleQuotedRawStringLiteral(_)
            | Value::TripleSingleQuotedRawStringLiteral(_)
            | Value::TripleDoubleQuotedRawStringLiteral(_)
            | Value::QuoteDelimitedStringLiteral(_)
            | Value::NationalQuoteDelimitedStringLiteral(_)
            | Value::HexStringLiteral(_)
            | Value::DoubleQuotedString(_) => {
                return ControlFlow::Break(plan_datafusion_err!("a string literal form the source is not written in"));
            }
        };
        // A placeholder is the one value `sqlparser` writes out as it holds it.
        value.value = Value::Placeholder(written);
        ControlFlow::Continue(())
    }
}
