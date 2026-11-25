//! `written`: a plan unparsed to one dialect's SQL text, with its text policies applied.

use std::ops::ControlFlow;

use datafusion::arrow::datatypes::DataType;
use datafusion::common::DataFusionError;
use datafusion::common::plan_err;
use datafusion::logical_expr::LogicalPlan;
use datafusion::sql::sqlparser::ast::{Expr as SqlExpr, Ident, ObjectName, VisitMut as _, VisitorMut};
use datafusion::sql::unparser::Unparser;
use datafusion::sql::unparser::dialect::{
    BigQueryDialect, CharacterLengthStyle, CustomDialectBuilder, DateFieldExtractStyle, DefaultDialect, Dialect, DuckDBDialect,
    PostgreSqlDialect,
};
use sutura_domain::plan::StatementTable;
use sutura_domain::warehouse::schema::TypeDialect;

use super::{UnparsedWith, literal, oracle, text};

/// Text functions that work on characters in the plan and on bytes or ASCII in `ClickHouse`, with
/// the UTF-8 function that works on characters there. Each of these exists in `ClickHouse`.
pub(super) const CHARACTER_FUNCTIONS: [(&str, &str); 12] = [
    ("strpos", "positionUTF8"),
    ("length", "lengthUTF8"),
    ("char_length", "lengthUTF8"),
    ("substr", "substringUTF8"),
    ("substring", "substringUTF8"),
    ("left", "leftUTF8"),
    ("right", "rightUTF8"),
    ("reverse", "reverseUTF8"),
    ("lower", "lowerUTF8"),
    ("upper", "upperUTF8"),
    ("initcap", "initcapUTF8"),
    ("translate", "translateUTF8"),
];

/// `plan` as `dialect`'s text: `read`'s text policies applied, except at Cube, which applies them
/// before its query operators, and then every string literal written as `dialect` reads it.
pub(super) fn written(plan: &LogicalPlan, dialect: TypeDialect, read: &[StatementTable]) -> UnparsedWith<String> {
    struct ClickHouseFunctions;

    impl VisitorMut for ClickHouseFunctions {
        type Break = DataFusionError;

        fn post_visit_expr(&mut self, expression: &mut SqlExpr) -> ControlFlow<Self::Break> {
            if let SqlExpr::Function(function) = expression
                && let [part] = function.name.0.as_slice()
                && let Some(name) = part.as_ident()
                && let Some(characters) = CHARACTER_FUNCTIONS
                    .iter()
                    .find_map(|(plain, utf8)| name.value.eq_ignore_ascii_case(plain).then_some(*utf8))
            {
                function.name = ObjectName::from(vec![Ident::new(characters)]);
            }
            ControlFlow::Continue(())
        }
    }

    if dialect == TypeDialect::Cube
        && plan
            .schema()
            .fields()
            .iter()
            .any(|field| matches!(field.data_type(), DataType::UInt16 | DataType::UInt32 | DataType::UInt64))
    {
        return plan_err!("Cube cannot preserve this unsigned result type");
    }
    let unparse = |spelling: &dyn Dialect| Unparser::new(spelling).plan_to_sql(plan);
    let mut statement = match dialect {
        TypeDialect::DuckDb => unparse(&DuckDBDialect::new()),
        TypeDialect::DataFusion => unparse(&DefaultDialect {}),
        TypeDialect::Postgres | TypeDialect::Cube => unparse(&PostgreSqlDialect {}),
        TypeDialect::Oracle => oracle::statement(plan),
        TypeDialect::BigQuery => unparse(&BigQueryDialect {}),
        TypeDialect::ClickHouse => unparse(
            &CustomDialectBuilder::new()
                .with_identifier_quote_style('`')
                .with_use_timestamp_for_date64(true)
                .with_supports_column_alias_in_table_alias(false)
                .with_date_field_extract_style(DateFieldExtractStyle::Extract)
                .with_character_length_style(CharacterLengthStyle::Length)
                .build(),
        ),
    }?;
    if dialect == TypeDialect::ClickHouse
        && let ControlFlow::Break(cause) = statement.visit(&mut ClickHouseFunctions)
    {
        return Err(cause);
    }
    let ineligible = if dialect == TypeDialect::Cube {
        None
    } else {
        text::apply_to(&mut statement, read, dialect)?
    };
    literal::spelled(&mut statement, dialect).map(|text| (text, ineligible))
}
