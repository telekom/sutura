//! String literals and names written as the source they are sent to reads them back.

use core::fmt::Display;
use std::ops::ControlFlow;

use datafusion::common::plan_datafusion_err;
use datafusion::error::{DataFusionError, Result};
use datafusion::sql::sqlparser::ast::{Value, ValueWithSpan, VisitMut, VisitorMut};
use datafusion::sql::sqlparser::dialect::{
    BigQueryDialect, ClickHouseDialect, Dialect, DuckDbDialect, GenericDialect, OracleDialect, PostgreSqlDialect,
};
use datafusion::sql::sqlparser::tokenizer::{Token, Tokenizer, Word};
use sutura_domain::plan::StatementTable;
use sutura_domain::warehouse::schema::TypeDialect;

/// `text` quoted so `dialect` reads it back unchanged. `BigQuery` and `ClickHouse` read a backslash in a
/// string as an escape, so there the backslash and the quote are each escaped with one, and so are a
/// newline and a carriage return, which `BigQuery` refuses inside a quoted string, and NUL, as `\x00`
/// at `BigQuery` and `\0` at `ClickHouse`;
/// every other control character is written as it stands. Every other dialect reads a backslash as
/// itself and a doubled quote as one.
///
/// | Limit | Why |
/// | --- | --- |
/// | Postgres reads it exactly only under `standard_conforming_strings = on` | it is the server default; sutura neither sets nor checks it |
/// | Oracle reads `''` as `NULL` | an empty literal is written as `''`, and Oracle has no empty string |
/// | a control character other than newline, carriage return and NUL is written raw | not measured at `BigQuery` or `ClickHouse` |
/// | Cube is written and read as Postgres | its SQL interface is taken to quote as Postgres does; not measured |
/// | the read-back cells use `sqlparser`'s model of each dialect | only `DataFusion` executes a quote-doubled literal in a cell, no cell runs a live `BigQuery` or `ClickHouse`, and `sqlparser` does not read `\x00`, so NUL at `BigQuery` is pinned by spelling only |
pub(super) fn literal(text: &str, dialect: TypeDialect) -> String {
    let escapes = matches!(dialect, TypeDialect::BigQuery | TypeDialect::ClickHouse);
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('\'');
    for character in text.chars() {
        match character {
            '\\' if escapes => quoted.push_str(r"\\"),
            '\'' if escapes => quoted.push_str(r"\'"),
            '\n' if escapes => quoted.push_str(r"\n"),
            '\r' if escapes => quoted.push_str(r"\r"),
            // `ClickHouse` reads any number of hex digits after `\x`; `BigQuery` reads exactly two.
            '\0' if dialect == TypeDialect::ClickHouse => quoted.push_str(r"\0"),
            '\0' if escapes => quoted.push_str(r"\x00"),
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

/// Whether every quoted name in `sql`, read as `dialect` reads it, is [`quotable`]: a name holding a
/// quote character or a backslash reads back holding one of them, however `sqlparser` quoted it.
pub(super) fn quotable_names(sql: &str, dialect: TypeDialect) -> bool {
    // Text that does not tokenize is the source guard's to refuse, not a name's.
    let Ok(tokens) = Tokenizer::new(reader(dialect), sql).tokenize() else {
        return true;
    };
    tokens.iter().all(|token| match token {
        Token::Word(Word {
            quote_style: Some(_),
            value,
            ..
        }) => quotable(value),
        _ => true,
    })
}

/// `sqlparser`'s model of how `dialect` reads a statement.
pub(super) fn reader(dialect: TypeDialect) -> &'static dyn Dialect {
    match dialect {
        TypeDialect::BigQuery => &BigQueryDialect {},
        TypeDialect::ClickHouse => &ClickHouseDialect {},
        TypeDialect::Postgres | TypeDialect::Cube => &PostgreSqlDialect {},
        TypeDialect::DuckDb => &DuckDbDialect {},
        TypeDialect::Oracle => &OracleDialect {},
        TypeDialect::DataFusion => &GenericDialect {},
    }
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

/// Declared here rather than beside the other statement cells: this module is new, so the base run
/// removes it and this cell with it, instead of failing to compile a cell over a function it lacks.
#[cfg(test)]
mod tests {
    use sutura_domain::warehouse::schema::TypeDialect;

    use super::literal;
    use crate::combine::statement::tests::literals::{DIALECTS, read_back};

    #[test]
    fn a_literal_reads_back_as_its_value_in_every_dialect() {
        for (dialect, value, written) in [
            (TypeDialect::BigQuery, r"a\", r"'a\\'"),
            (TypeDialect::BigQuery, r"b\'c", r"'b\\\'c'"),
            (TypeDialect::ClickHouse, r"a\", r"'a\\'"),
            (TypeDialect::ClickHouse, r"b\'c", r"'b\\\'c'"),
            (TypeDialect::Postgres, r"a\", r"'a\'"),
            (TypeDialect::Postgres, r"b\'c", r"'b\''c'"),
            (TypeDialect::Oracle, "d''e", "'d''''e'"),
            (TypeDialect::BigQuery, "a\nb\rc", r"'a\nb\rc'"),
            (TypeDialect::ClickHouse, "a\nb\rc", r"'a\nb\rc'"),
            (TypeDialect::BigQuery, "\0", r"'\x00'"),
            (TypeDialect::ClickHouse, "\0a", r"'\0a'"),
            (TypeDialect::BigQuery, "\0a", r"'\x00a'"),
            (TypeDialect::Postgres, "a\nb\rc", "'a\nb\rc'"),
        ] {
            assert_eq!(literal(value, dialect), written, "{dialect:?}");
        }
        assert_eq!(
            read_back(&literal("\0a", TypeDialect::ClickHouse), TypeDialect::ClickHouse),
            ["\0a"]
        );
        for dialect in DIALECTS {
            for value in [r"a\", r"b\'c", "d''e", r"\\'", "'", "a\nb\rc"] {
                assert_eq!(read_back(&literal(value, dialect), dialect), [value], "{dialect:?}");
            }
        }
    }
}
